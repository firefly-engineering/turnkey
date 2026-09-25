// Package testcache runs the local test result cache for tk test.
//
// Test results are recorded in, and reused from, a local cache-only Remote
// Execution API server (bazel-remote), one per user per machine, shared by
// every checkout (docs/specs/test-result-caching.md). tk starts it on demand
// and tells turnkey's test runner, through flags after `--`, whether to use it.
package testcache

import (
	"fmt"
	"io"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"
	"time"
)

// Environment variables the turnkey dev shell sets when test result caching
// is enabled.
const (
	// ServerEnv names the bazel-remote binary.
	ServerEnv = "TURNKEY_TEST_CACHE_SERVER"
	// AddressEnv is the cache's gRPC address, as in the generated
	// .buckconfig's [buck2_re_client] section: grpc://127.0.0.1:<port>.
	AddressEnv = "TURNKEY_TEST_CACHE_ADDRESS"
	// CacheDirEnv overrides where turnkey keeps its caches.
	CacheDirEnv = "TURNKEY_CACHE_DIR"
)

// Mode is the runner's --turnkey-test-cache value.
type Mode string

const (
	// On reuses recorded results and records fresh passes.
	On Mode = "on"
	// RecordOnly runs every test and records fresh passes.
	RecordOnly Mode = "record-only"
	// Off neither reads nor records.
	Off Mode = "off"
)

// maxSizeGiB bounds the store; bazel-remote evicts least recently used
// entries beyond it.
const maxSizeGiB = 5

// Config is the local test result cache as the dev shell describes it.
type Config struct {
	Server  string // bazel-remote binary
	Address string // grpc://host:port
}

// FromEnv returns the cache configuration, or nil when the dev shell doesn't
// enable test result caching.
func FromEnv() *Config {
	server, address := os.Getenv(ServerEnv), os.Getenv(AddressEnv)
	if server == "" || address == "" {
		return nil
	}
	return &Config{Server: server, Address: address}
}

// hostPort returns the address without its grpc:// scheme.
func (c *Config) hostPort() (string, error) {
	hostPort, ok := strings.CutPrefix(c.Address, "grpc://")
	if !ok {
		return "", fmt.Errorf("test cache address %q: expected grpc://host:port", c.Address)
	}
	return hostPort, nil
}

// StoreDir is where recorded results live: one store per user per machine,
// shared by every checkout and every turnkey repo.
func StoreDir() (string, error) {
	base := os.Getenv(CacheDirEnv)
	if base == "" {
		userCache, err := os.UserCacheDir()
		if err != nil {
			return "", fmt.Errorf("locating the user cache directory: %w", err)
		}
		base = filepath.Join(userCache, "turnkey")
	}
	return filepath.Join(base, "test-results"), nil
}

// http2Preface opens an HTTP/2 connection (RFC 9113 section 3.4): the client
// preface followed by an empty SETTINGS frame.
var http2Preface = append(
	[]byte("PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n"),
	0, 0, 0, // payload length 0
	0x4,        // type SETTINGS
	0,          // flags
	0, 0, 0, 0, // stream 0
)

// Reachable reports whether a gRPC server answers at the cache's address
// within timeout. gRPC runs over HTTP/2, whose servers must answer a client
// preface with a SETTINGS frame; a listener that doesn't, such as some other
// program holding the port, is not the cache.
func (c *Config) Reachable(timeout time.Duration) bool {
	hostPort, err := c.hostPort()
	if err != nil {
		return false
	}
	conn, err := net.DialTimeout("tcp", hostPort, timeout)
	if err != nil {
		return false
	}
	defer conn.Close()
	if err := conn.SetDeadline(time.Now().Add(timeout)); err != nil {
		return false
	}
	if _, err := conn.Write(http2Preface); err != nil {
		return false
	}
	// A frame header is 9 bytes; the type is the fourth.
	header := make([]byte, 9)
	if _, err := io.ReadFull(conn, header); err != nil {
		return false
	}
	return header[3] == 0x4
}

// Ensure makes sure the cache is up, starting it if nothing answers. The
// server runs detached and outlives tk. It fails fast when the server can't
// start, so tk test can fall back to running uncached without a noticeable
// delay.
func (c *Config) Ensure() error {
	const probe = 200 * time.Millisecond
	if c.Reachable(probe) {
		return nil
	}

	// Several tk test runs (other checkouts, other terminals) may find the
	// cache down at once. Only the one holding the lock starts it; the others
	// find it up once they get the lock.
	unlock, err := lockStore()
	if err != nil {
		return err
	}
	defer unlock()
	if c.Reachable(probe) {
		return nil
	}

	exited, err := c.start()
	if err != nil {
		return err
	}
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		if c.Reachable(probe) {
			return nil
		}
		select {
		case <-exited:
			store, _ := StoreDir()
			return fmt.Errorf("the test result cache exited on startup (see %s)", filepath.Join(store, "server.log"))
		case <-time.After(50 * time.Millisecond):
		}
	}
	return fmt.Errorf("test result cache did not come up at %s", c.Address)
}

// lockStore takes an exclusive lock on the store, waiting for any other
// holder, and returns the function that releases it.
func lockStore() (func(), error) {
	store, err := StoreDir()
	if err != nil {
		return nil, err
	}
	if err := os.MkdirAll(store, 0o755); err != nil {
		return nil, fmt.Errorf("creating the test result store: %w", err)
	}
	lock, err := os.OpenFile(filepath.Join(store, "server.lock"), os.O_CREATE|os.O_RDWR, 0o644)
	if err != nil {
		return nil, fmt.Errorf("opening the test result cache lock: %w", err)
	}
	if err := syscall.Flock(int(lock.Fd()), syscall.LOCK_EX); err != nil {
		lock.Close()
		return nil, fmt.Errorf("locking the test result store: %w", err)
	}
	return func() {
		_ = syscall.Flock(int(lock.Fd()), syscall.LOCK_UN)
		lock.Close()
	}, nil
}

// start launches the server and returns a channel closed when it exits.
func (c *Config) start() (<-chan struct{}, error) {
	hostPort, err := c.hostPort()
	if err != nil {
		return nil, err
	}
	store, err := StoreDir()
	if err != nil {
		return nil, err
	}
	if err := os.MkdirAll(store, 0o755); err != nil {
		return nil, fmt.Errorf("creating the test result store: %w", err)
	}
	logFile, err := os.OpenFile(filepath.Join(store, "server.log"), os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o644)
	if err != nil {
		return nil, fmt.Errorf("opening the test result cache log: %w", err)
	}
	defer logFile.Close()

	cmd := exec.Command(c.Server,
		"--dir", filepath.Join(store, "cas"),
		"--max_size", fmt.Sprint(maxSizeGiB),
		"--grpc_address", hostPort,
		// bazel-remote always serves HTTP too; nothing uses it, so take any
		// free port rather than risk a clash.
		"--http_address", "127.0.0.1:0",
	)
	cmd.Stdout, cmd.Stderr = logFile, logFile
	// Detach from tk's session so the server survives tk and the terminal.
	cmd.SysProcAttr = &syscall.SysProcAttr{Setsid: true}
	if err := cmd.Start(); err != nil {
		return nil, fmt.Errorf("starting the test result cache: %w", err)
	}
	// Watch for an early exit (a taken port, a bad store). A server that
	// comes up keeps running after tk hands over to buck2.
	exited := make(chan struct{})
	go func() {
		_ = cmd.Wait()
		close(exited)
	}()
	return exited, nil
}

// RunnerArgs are the turnkey-test-runner flags tk passes after `--`.
func (c *Config) RunnerArgs(mode Mode) []string {
	return []string{
		"--turnkey-test-cache=" + string(mode),
		"--turnkey-test-cache-address=" + c.Address,
	}
}

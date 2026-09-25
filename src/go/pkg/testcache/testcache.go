// Package testcache runs the local test result cache for tk test.
//
// Test results are recorded in, and reused from, a local cache-only Remote
// Execution API server (bazel-remote), one per user per machine, shared by
// every checkout (docs/specs/test-result-caching.md). tk starts it on demand
// and tells turnkey's test runner, through flags after `--`, whether to use it.
package testcache

import (
	"fmt"
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

// Reachable reports whether something accepts connections at the cache's
// address within timeout.
func (c *Config) Reachable(timeout time.Duration) bool {
	hostPort, err := c.hostPort()
	if err != nil {
		return false
	}
	conn, err := net.DialTimeout("tcp", hostPort, timeout)
	if err != nil {
		return false
	}
	conn.Close()
	return true
}

// Ensure makes sure the cache is up, starting it if nothing answers. The
// server runs detached and outlives tk.
func (c *Config) Ensure() error {
	const probe = 200 * time.Millisecond
	if c.Reachable(probe) {
		return nil
	}
	if err := c.start(); err != nil {
		return err
	}
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		if c.Reachable(probe) {
			return nil
		}
		time.Sleep(50 * time.Millisecond)
	}
	return fmt.Errorf("test result cache did not come up at %s", c.Address)
}

func (c *Config) start() error {
	hostPort, err := c.hostPort()
	if err != nil {
		return err
	}
	store, err := StoreDir()
	if err != nil {
		return err
	}
	if err := os.MkdirAll(store, 0o755); err != nil {
		return fmt.Errorf("creating the test result store: %w", err)
	}
	logFile, err := os.OpenFile(filepath.Join(store, "server.log"), os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o644)
	if err != nil {
		return fmt.Errorf("opening the test result cache log: %w", err)
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
		return fmt.Errorf("starting the test result cache: %w", err)
	}
	// Don't wait for it; let init reap it.
	return cmd.Process.Release()
}

// RunnerArgs are the turnkey-test-runner flags tk passes after `--`.
func (c *Config) RunnerArgs(mode Mode) []string {
	return []string{
		"--turnkey-test-cache=" + string(mode),
		"--turnkey-test-cache-address=" + c.Address,
	}
}

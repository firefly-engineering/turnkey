// Package testcache owns tk test's reuse policy and runs the local test
// result cache.
//
// Test results are recorded in, and reused from, a local cache-only Remote
// Execution API server (bazel-remote), one per user per machine, shared by
// every checkout (docs/specs/test-result-caching.md). tk starts it on demand.
// Plan decides, for one tk test run, what turnkey's test runner does with the
// cache, and the runner only obeys the flags it gets after `--`.
package testcache

import (
	"fmt"
	"io"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"time"
)

// Environment variables the turnkey dev shell sets when test result caching
// is enabled.
const (
	// ServerEnv names the bazel-remote binary. It is unset when the shell
	// uses a remote cache, which tk doesn't manage.
	ServerEnv = "TURNKEY_TEST_CACHE_SERVER"
	// AddressEnv is the cache's gRPC address, as in the generated
	// .buckconfig's [buck2_re_client] section: grpc://127.0.0.1:<port>.
	AddressEnv = "TURNKEY_TEST_CACHE_ADDRESS"
	// CacheDirEnv overrides where turnkey keeps its caches.
	CacheDirEnv = "TURNKEY_CACHE_DIR"
	// SizeEnv overrides the store's size limit, in GiB.
	SizeEnv = "TURNKEY_TEST_CACHE_SIZE_GIB"
)

// Mode is the runner's --turnkey-test-cache value.
type Mode string

const (
	// On reuses recorded results and records fresh passes.
	On Mode = "on"
	// RecordOnly runs every test and records fresh passes.
	RecordOnly Mode = "record-only"
	// ReadOnly reuses recorded results and never records.
	ReadOnly Mode = "read-only"
	// Off neither reads nor records.
	Off Mode = "off"
)

// Origin is the runner's --turnkey-test-cache-origin value: where the cache
// lives, which the runner reports on each hit.
type Origin string

const (
	// Local is the cache tk runs on this machine.
	Local Origin = "local"
	// Remote is a shared cache the dev shell points at.
	Remote Origin = "remote"
)

// defaultMaxSizeGiB bounds the store; bazel-remote evicts least recently
// used entries beyond it.
const defaultMaxSizeGiB = 5

// MaxSizeGiB is the store's size limit: SizeEnv if set, else the default.
func MaxSizeGiB() (int, error) {
	value := os.Getenv(SizeEnv)
	if value == "" {
		return defaultMaxSizeGiB, nil
	}
	size, err := strconv.Atoi(value)
	if err != nil || size <= 0 {
		return 0, fmt.Errorf("%s=%q: expected a positive number of GiB", SizeEnv, value)
	}
	return size, nil
}

// idleTimeout stops a server nobody has used for this long, so no process is
// left behind on a machine that stopped running tk test. The next tk test
// starts it again; recorded results stay in the store.
const idleTimeout = "24h"

// Config is the test result cache as the dev shell describes it.
type Config struct {
	Server  string // bazel-remote binary; empty for a remote cache
	Address string // grpc://host:port
}

// FromEnv returns the cache configuration, or nil when the dev shell doesn't
// enable test result caching.
func FromEnv() *Config {
	address := os.Getenv(AddressEnv)
	if address == "" {
		return nil
	}
	return &Config{Server: os.Getenv(ServerEnv), Address: address}
}

// Managed reports whether tk runs this cache (a local one) itself. The dev
// shell names a server only for the local cache, so this is also where the
// cache lives.
func (c *Config) Managed() bool {
	return c.Server != ""
}

// Origin is where the cache lives.
func (c *Config) Origin() Origin {
	if c.Managed() {
		return Local
	}
	return Remote
}

// Plan is what turnkey's test runner does with the cache in one tk test run.
type Plan struct {
	Mode    Mode
	Origin  Origin
	Address string
	// Unusable says why the cache is off for this run; empty when it's in use.
	Unusable string
}

// remoteProbe bounds how long tk waits for a remote cache to accept a
// connection before running the tests uncached.
const remoteProbe = 2 * time.Second

// Plan applies the reuse policy (CONTEXT.md) to one tk test run. A forced
// re-run reads nothing. Results are recorded only into the local cache: who
// may write to a shared one isn't decided. A cache that can't be used is
// off, since buck2 would otherwise retry it for about 45 s and then fail
// every cached test without running it.
func (c *Config) Plan(forced bool) Plan {
	return c.plan(forced, c.usable)
}

// plan is Plan with the check that the cache can be used as a seam.
func (c *Config) plan(forced bool, usable func() error) Plan {
	plan := Plan{Origin: c.Origin(), Address: c.Address}
	if err := usable(); err != nil {
		plan.Mode = Off
		plan.Unusable = err.Error()
		return plan
	}
	switch {
	case plan.Origin == Local && forced:
		plan.Mode = RecordOnly
	case plan.Origin == Local:
		plan.Mode = On
	case forced:
		plan.Mode = Off
	default:
		plan.Mode = ReadOnly
	}
	return plan
}

// usable starts the local cache if needed. A remote cache only has to accept
// a connection: it may use TLS, which tk isn't told about, so it isn't asked
// to answer as a gRPC server.
func (c *Config) usable() error {
	if c.Managed() {
		return c.Ensure()
	}
	hostPort, err := c.hostPort()
	if err != nil {
		return err
	}
	conn, err := net.DialTimeout("tcp", hostPort, remoteProbe)
	if err != nil {
		return fmt.Errorf("the test result cache at %s is unreachable: %w", c.Address, err)
	}
	conn.Close()
	return nil
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
	maxSize, err := MaxSizeGiB()
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
		"--max_size", fmt.Sprint(maxSize),
		"--grpc_address", hostPort,
		// bazel-remote always serves HTTP too; nothing uses it, so take any
		// free port rather than risk a clash.
		"--http_address", "127.0.0.1:0",
		"--idle_timeout", idleTimeout,
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

// RunnerArgs are the turnkey-test-runner flags tk passes after `--`. The
// runner writes the number of reused results to report when it's done.
func (p Plan) RunnerArgs(report string) []string {
	return []string{
		"--turnkey-test-cache=" + string(p.Mode),
		"--turnkey-test-cache-address=" + p.Address,
		"--turnkey-test-cache-origin=" + string(p.Origin),
		"--turnkey-test-cache-report=" + report,
	}
}

// ReadReport returns the number of reused results the runner reported, and
// false when it reported nothing (for instance when the build failed before
// any test ran).
func ReadReport(report string) (int, bool) {
	data, err := os.ReadFile(report)
	if err != nil {
		return 0, false
	}
	hits, err := strconv.Atoi(strings.TrimSpace(string(data)))
	if err != nil {
		return 0, false
	}
	return hits, true
}

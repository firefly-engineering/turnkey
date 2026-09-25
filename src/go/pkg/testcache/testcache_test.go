package testcache

import (
	_ "embed"
	"encoding/json"
	"errors"
	"io"
	"net"
	"os"
	"path/filepath"
	"slices"
	"sync"
	"testing"
	"time"
)

func TestFromEnv(t *testing.T) {
	t.Setenv(ServerEnv, "")
	t.Setenv(AddressEnv, "")
	if FromEnv() != nil {
		t.Fatal("expected no config without an address")
	}
	t.Setenv(AddressEnv, "grpc://cache.example.com:443")
	if c := FromEnv(); c == nil || c.Managed() {
		t.Fatalf("expected an unmanaged remote cache, got %+v", c)
	}
	t.Setenv(ServerEnv, "/bin/bazel-remote")
	t.Setenv(AddressEnv, "grpc://127.0.0.1:1")
	if c := FromEnv(); c == nil || !c.Managed() || c.Address != "grpc://127.0.0.1:1" {
		t.Fatalf("expected a managed local cache, got %+v", c)
	}
}

func TestStoreDirHonoursTurnkeyCacheDir(t *testing.T) {
	t.Setenv(CacheDirEnv, "/somewhere")
	dir, err := StoreDir()
	if err != nil {
		t.Fatal(err)
	}
	if want := filepath.Join("/somewhere", "test-results"); dir != want {
		t.Fatalf("got %s, want %s", dir, want)
	}
}

// serve accepts connections and answers each with respond.
func serve(t *testing.T, respond func(net.Conn)) string {
	t.Helper()
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { listener.Close() })
	go func() {
		for {
			conn, err := listener.Accept()
			if err != nil {
				return
			}
			go func() {
				defer conn.Close()
				respond(conn)
			}()
		}
	}()
	return listener.Addr().String()
}

// http2Server reads the client preface and answers with a SETTINGS frame,
// as any HTTP/2 (and so gRPC) server does.
func http2Server(conn net.Conn) {
	buf := make([]byte, len(http2Preface))
	if _, err := io.ReadFull(conn, buf); err != nil {
		return
	}
	conn.Write([]byte{0, 0, 0, 0x4, 0, 0, 0, 0, 0})
	time.Sleep(time.Second)
}

func TestReachableNeedsAnHTTP2Server(t *testing.T) {
	grpcLike := &Config{Address: "grpc://" + serve(t, http2Server)}
	if !grpcLike.Reachable(time.Second) {
		t.Fatal("expected an HTTP/2 server to be reachable")
	}
	silent := &Config{Address: "grpc://" + serve(t, func(net.Conn) { time.Sleep(time.Second) })}
	if silent.Reachable(100 * time.Millisecond) {
		t.Fatal("expected a listener that doesn't speak HTTP/2 to be unreachable")
	}
}

func TestEnsureFailsFastWhenThePortIsTaken(t *testing.T) {
	t.Setenv(CacheDirEnv, t.TempDir())
	// Something that isn't the cache holds the port, and the "server" exits
	// at once, as bazel-remote does when it can't bind.
	c := &Config{Server: "/usr/bin/false", Address: "grpc://" + serve(t, func(net.Conn) { time.Sleep(time.Second) })}
	start := time.Now()
	if err := c.Ensure(); err == nil {
		t.Fatal("expected an error")
	}
	if elapsed := time.Since(start); elapsed > 2*time.Second {
		t.Fatalf("took %s to give up; expected a fast failure", elapsed)
	}
}

func TestEnsureUsesARunningServer(t *testing.T) {
	c := &Config{Server: "/nonexistent/bazel-remote", Address: "grpc://" + serve(t, http2Server)}
	if err := c.Ensure(); err != nil {
		t.Fatalf("expected the running server to be used without starting one: %v", err)
	}
}

func TestEnsureReportsAServerThatWontStart(t *testing.T) {
	t.Setenv(CacheDirEnv, t.TempDir())
	listener, _ := net.Listen("tcp", "127.0.0.1:0")
	address := listener.Addr().String()
	listener.Close() // now nothing listens there
	c := &Config{Server: "/nonexistent/bazel-remote", Address: "grpc://" + address}
	if err := c.Ensure(); err == nil {
		t.Fatal("expected an error when the server can't start")
	}
}

func TestReachableRejectsNonGrpcAddresses(t *testing.T) {
	c := &Config{Address: "http://127.0.0.1:1"}
	if c.Reachable(10 * time.Millisecond) {
		t.Fatal("expected an unsupported address to be unreachable")
	}
}

func TestPlanAppliesTheReusePolicy(t *testing.T) {
	local := &Config{Server: "bazel-remote", Address: "grpc://127.0.0.1:47301"}
	remote := &Config{Address: "grpc://cache.example.com:443"}
	up := func() error { return nil }
	down := func() error { return errors.New("down") }
	for _, tc := range []struct {
		name   string
		cache  *Config
		forced bool
		usable func() error
		want   Mode
	}{
		{"local", local, false, up, On},
		{"local, forced re-run", local, true, up, RecordOnly},
		{"local, unusable", local, false, down, Off},
		{"remote: never recorded into", remote, false, up, ReadOnly},
		{"remote, forced re-run", remote, true, up, Off},
		{"remote, unusable", remote, false, down, Off},
	} {
		t.Run(tc.name, func(t *testing.T) {
			plan := tc.cache.plan(tc.forced, tc.usable)
			if plan.Mode != tc.want {
				t.Errorf("mode %q, want %q", plan.Mode, tc.want)
			}
			if (plan.Unusable != "") != (tc.usable() != nil) {
				t.Errorf("unusable %q, want it set only for an unusable cache", plan.Unusable)
			}
			if plan.Origin != tc.cache.Origin() || plan.Address != tc.cache.Address {
				t.Errorf("plan %+v doesn't name the cache %+v", plan, tc.cache)
			}
		})
	}
}

func TestOriginFollowsWhoRunsTheCache(t *testing.T) {
	// A shared cache reached through a loopback tunnel is still remote.
	tunnel := &Config{Address: "grpc://127.0.0.1:9092"}
	if tunnel.Origin() != Remote {
		t.Errorf("unmanaged loopback cache: origin %q, want remote", tunnel.Origin())
	}
	if (&Config{Server: "bazel-remote", Address: tunnel.Address}).Origin() != Local {
		t.Error("managed cache: want origin local")
	}
}

func TestUnreachableRemoteCacheIsUnusable(t *testing.T) {
	// Nothing listens on a port just released.
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	address := "grpc://" + listener.Addr().String()
	listener.Close()
	if err := (&Config{Address: address}).usable(); err == nil {
		t.Fatal("expected an unreachable remote cache to be unusable")
	}
}

// runnerContract is what tk passes turnkey-test-runner and reads back from
// it. The runner's tests check their side against the same file.
//
//go:embed testdata/runner-contract.json
var runnerContract []byte

func TestRunnerContract(t *testing.T) {
	var contract struct {
		Address string
		Report  string
		Plans   []struct {
			Mode   Mode
			Origin Origin
			Args   []string
		}
		Hits       int
		HitsReport string `json:"hits_report"`
	}
	if err := json.Unmarshal(runnerContract, &contract); err != nil {
		t.Fatal(err)
	}
	for _, want := range contract.Plans {
		plan := Plan{Mode: want.Mode, Origin: want.Origin, Address: contract.Address}
		if got := plan.RunnerArgs(contract.Report); !slices.Equal(got, want.Args) {
			t.Errorf("%s/%s: got %v, want %v", want.Mode, want.Origin, got, want.Args)
		}
	}
	report := filepath.Join(t.TempDir(), "report")
	if err := os.WriteFile(report, []byte(contract.HitsReport), 0o644); err != nil {
		t.Fatal(err)
	}
	if hits, ok := ReadReport(report); !ok || hits != contract.Hits {
		t.Errorf("ReadReport(%q) = %d, %v; want %d", contract.HitsReport, hits, ok, contract.Hits)
	}
}

func TestReadReport(t *testing.T) {
	report := filepath.Join(t.TempDir(), "report")
	if _, ok := ReadReport(report); ok {
		t.Fatal("expected a missing report to read as nothing")
	}
	os.WriteFile(report, []byte("3\n"), 0o644)
	if hits, ok := ReadReport(report); !ok || hits != 3 {
		t.Fatalf("got %d, %v", hits, ok)
	}
}

// TestMain lets the test binary stand in for bazel-remote: run with
// TESTCACHE_FAKE_SERVER set, it records its start and serves HTTP/2 prefaces
// on --grpc_address until killed.
func TestMain(m *testing.M) {
	if starts := os.Getenv("TESTCACHE_FAKE_SERVER"); starts != "" {
		fakeServer(starts)
		return
	}
	os.Exit(m.Run())
}

func fakeServer(startsFile string) {
	f, _ := os.OpenFile(startsFile, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o644)
	f.WriteString("start\n")
	f.Close()
	var address string
	for i, arg := range os.Args {
		if arg == "--grpc_address" && i+1 < len(os.Args) {
			address = os.Args[i+1]
		}
	}
	listener, err := net.Listen("tcp", address)
	if err != nil {
		os.Exit(1)
	}
	// Serve for long enough to outlive the test, then exit on our own.
	go func() { time.Sleep(10 * time.Second); os.Exit(0) }()
	for {
		conn, err := listener.Accept()
		if err != nil {
			os.Exit(0)
		}
		go http2Server(conn)
	}
}

func TestConcurrentEnsureStartsOneServer(t *testing.T) {
	t.Setenv(CacheDirEnv, t.TempDir())
	starts := filepath.Join(t.TempDir(), "starts")
	t.Setenv("TESTCACHE_FAKE_SERVER", starts)
	probe, _ := net.Listen("tcp", "127.0.0.1:0")
	address := probe.Addr().String()
	probe.Close()
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	c := &Config{Server: executable, Address: "grpc://" + address}

	var wg sync.WaitGroup
	errs := make([]error, 4)
	for i := range errs {
		wg.Add(1)
		go func() {
			defer wg.Done()
			errs[i] = c.Ensure()
		}()
	}
	wg.Wait()
	for _, err := range errs {
		if err != nil {
			t.Fatal(err)
		}
	}
	data, err := os.ReadFile(starts)
	if err != nil {
		t.Fatal(err)
	}
	if n := len(data) / len("start\n"); n != 1 {
		t.Fatalf("started %d servers, want 1", n)
	}
}

func TestMaxSizeGiB(t *testing.T) {
	t.Setenv(SizeEnv, "")
	if size, err := MaxSizeGiB(); err != nil || size != 5 {
		t.Fatalf("default: got %d, %v", size, err)
	}
	t.Setenv(SizeEnv, "2")
	if size, err := MaxSizeGiB(); err != nil || size != 2 {
		t.Fatalf("override: got %d, %v", size, err)
	}
	for _, bad := range []string{"0", "-1", "lots"} {
		t.Setenv(SizeEnv, bad)
		if _, err := MaxSizeGiB(); err == nil {
			t.Fatalf("expected %q to be rejected", bad)
		}
	}
}

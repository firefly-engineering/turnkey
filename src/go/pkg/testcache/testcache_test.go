package testcache

import (
	"net"
	"path/filepath"
	"testing"
	"time"
)

func TestFromEnvNeedsServerAndAddress(t *testing.T) {
	t.Setenv(ServerEnv, "")
	t.Setenv(AddressEnv, "grpc://127.0.0.1:1")
	if FromEnv() != nil {
		t.Fatal("expected no config without a server")
	}
	t.Setenv(ServerEnv, "/bin/bazel-remote")
	if c := FromEnv(); c == nil || c.Address != "grpc://127.0.0.1:1" {
		t.Fatalf("unexpected config %+v", c)
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

func TestEnsureUsesARunningServer(t *testing.T) {
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	c := &Config{Server: "/nonexistent/bazel-remote", Address: "grpc://" + listener.Addr().String()}
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

func TestRunnerArgs(t *testing.T) {
	c := &Config{Address: "grpc://127.0.0.1:47301"}
	got := c.RunnerArgs(On)
	want := []string{"--turnkey-test-cache=on", "--turnkey-test-cache-address=grpc://127.0.0.1:47301"}
	if len(got) != len(want) || got[0] != want[0] || got[1] != want[1] {
		t.Fatalf("got %v, want %v", got, want)
	}
}

package testcache

import (
	"io"
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

func TestRunnerArgs(t *testing.T) {
	c := &Config{Address: "grpc://127.0.0.1:47301"}
	got := c.RunnerArgs(On)
	want := []string{"--turnkey-test-cache=on", "--turnkey-test-cache-address=grpc://127.0.0.1:47301"}
	if len(got) != len(want) || got[0] != want[0] || got[1] != want[1] {
		t.Fatalf("got %v, want %v", got, want)
	}
}

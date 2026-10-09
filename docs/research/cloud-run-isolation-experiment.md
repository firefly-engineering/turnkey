# Cloud Run isolation experiment

- **Ticket:** [Do Cloud Run sandboxes isolate successive actions, and does the Nix sandbox run on Cloud Run?](https://github.com/firefly-engineering/turnkey/issues/76), on the map [Incremental builds everywhere for turnkey repos on GitHub](https://github.com/firefly-engineering/turnkey/issues/60).
- **Builds on:** `docs/research/cloud-run-executor.md` (bookmark `research/cloud-run-executor`), §3 and §4.
- **Run:** 2026-10-09, project `turnkey-cr-iso-20261009` (deleted afterwards), region `europe-west1`. Service: gen2, concurrency 1, max instances 1, 2 vCPU / 4 GiB, `--sandbox-launcher` (needs gcloud ≥ 583; 570 rejects the flag). Job: gen2, same image. Image: `nixos/nix:2.28.3` plus python3, bubblewrap, util-linux, procps, curl, fuse3, and a root-owned read-only `/store` standing in for the per-instance store cache.

Every request in the service ran on **one instance** (same metadata instance id and same host boot id throughout), so "the next request" always meant the same, already-used instance.

## Verdict

1. **Cloud Run sandboxes isolate successive actions on one instance**, for every probe tried. Nothing an action did in one `sandbox do` was visible to the host container or the next sandbox.
2. **The mechanism is gVisor.** Inside `sandbox do` the kernel reports `4.19.0-gvisor`, the mounts are `runsc` 9p/tmpfs, and each sandbox has its own boot id. The host container is the gen2 microVM (kernel `6.9.12`). The open question in the earlier note ("Google doesn't say what enforces it") is answered: a gVisor sandbox per command, inside the per-instance microVM.
3. **The Nix sandbox works in a gen2 service container and in a job**: `nix-build --option sandbox true --option sandbox-fallback false` succeeds as root. User, mount, PID and network namespaces all work, and so does bubblewrap, also as a non-root uid. The seccomp filter (mode 2, `NoNewPrivs: 1`) doesn't block them. This reverses the earlier note's "doubtful".
4. **Nix doesn't build inside `sandbox do`**, sandboxed or not: gVisor refuses `uid_map` writes, and an unsandboxed build dies with an I/O error on the builder's pipe. So the Nix builder runs in the service or job container directly, never in a Cloud Run sandbox.
5. **`sandbox do` costs about 0.25 s per command** (median 0.26 s, range 0.23–0.36 s, against 0.005 s on the host).

## Results

| Probe | Result |
|---|---|
| Background processes (`setsid nohup`, double fork, `disown`) | Gone when `sandbox do` returns; the host's process table holds only the server. |
| Write to the host's root filesystem | `Read-only file system`. |
| Write with `--write` | Lands in a tmpfs overlay; not visible to the host or the next sandbox. |
| Write to `/tmp` (no `--write`) | Allowed, private to the sandbox, gone afterwards. |
| Write to the store, through the root view or a `readonly` bind mount | `Permission denied`. |
| Read-write bind mount (the action's work directory) | Writes visible to the host, as intended. |
| Environment | Empty except what `--env` sets (`HOME=/`, `PWD=/`). |
| Metadata server (`metadata.google.internal`, `169.254.169.254`) | Connection refused. The host container reaches it. |
| Network | DNS fails; no egress. |
| Capabilities in the sandbox | uid 0, `CapEff 0x20000420` (`KILL`, `NET_BIND_SERVICE`, `AUDIT_WRITE`); no `DAC_OVERRIDE`, which is why root-owned read-only directories refuse writes. |
| Memory fill (6 GiB on a 4 GiB instance) | Sandbox killed after 5.5 s; host survives on the same instance with its memory back. |
| Disk fill (8 GiB into the `--write` overlay) | Same: the overlay is memory-backed, the sandbox is killed, the host is unaffected. |
| Fork bomb | Capped at 1024 processes (`ulimit -u`); nothing survives. |
| `/dev/fuse` | Present in the host container, the job and the sandbox (`crw-rw-rw-`); mounting was not tried. |
| Non-root uid 1000 in the host container | Reads the store, can't write it; `unshare -Ur` and `bwrap --unshare-all` work. |

## Consequences for the executor *(inferred)*

- **A killed sandbox looks like an infrastructure error**: `sandbox do` exits 128 with `urpc method "containerManager.WaitPID" failed: EOF`. The executor has to report it as the action exhausting its resources, not as a service fault, or buck2's fallback on infrastructure errors would rerun it locally.
- **An action that itself needs namespaces** (bwrap, a nested Nix build, containers in tests) fails under `sandbox do`. Such actions are `local_only`.
- **The memory limit is the instance's.** Sandboxes share the host container's memory, and one action at a time (concurrency 1) gets all of it minus the executor's.
- **Plan B exists.** If Cloud Run sandboxes changed or disappeared (they are Pre-GA), namespaces work in the host container, so the executor could isolate actions itself with bubblewrap or nsjail. Isolation would then rest on Linux namespaces under seccomp inside the microVM, not on gVisor.

## Not tested

- Escape resistance itself: gVisor's track record is the evidence, not this experiment.
- Concurrency above 1, `sandbox run --detach`, and sandboxes in jobs.
- Mounting a FUSE filesystem.
- Nix as a multi-user daemon with build users; only single-user root builds were run.

## Cost

About 10 minutes of one 2 vCPU / 4 GiB instance, one 1 min 46 s Cloud Build (within the free tier), and a few hundred MB in Artifact Registry for under an hour: well under USD 0.10 *(estimated; billing data lags)*. The project was deleted when the run finished.

## Raw results

Service probes, in order (instance id truncated to its last 12 characters):

```text
### A1 sandbox: leave background processes + write files
instance=8fa62d2a8f06 boot=47c54eba-f7ad-465e-b8df-9d75106f64ba rc=0 secs=0.467
--stdout--
bundle-root
leak-sandbox-tmp
runsc-root
done
--stderr--
bash: line 1: /app/leak: Read-only file system
bash: line 1: /store/leak: Permission denied
bash: line 1: /root/leak: Read-only file system

### A2 host: did anything survive/leak?
instance=8fa62d2a8f06 boot=47c54eba-f7ad-465e-b8df-9d75106f64ba rc=0 secs=0.081
--stdout--
  PID USER     COMMAND
    1 root     /root/.nix-profile/bin/python3 /app/server.py
/app:
total 16
drwxr-xr-x 2 root root   95 Oct  9 10:05 .
drwxr-xr-x 1 root root  160 Oct  9 10:08 ..
-rwxr-xr-x 1 root root  741 Oct  9 07:21 probe-nix.sh
-rw-r--r-- 1 root root 1817 Oct  9 07:21 server.py
-rw-r--r-- 1 root root  171 Oct  9 07:21 trivial.nix

/root:
total 20
drwxr-xr-x 4 root root 118 Oct  9 10:04 .
drwxr-xr-x 1 root root 160 Oct  9 10:08 ..
drwxr-xr-x 3 root root  42 Oct  9 10:04 .cache
lrwxrwxrwx 1 root root  74 Jan  1  1980 .nix-channels -> /nix/store/wwzb50yira33iwzchhk4y5kgbvcx4irm-base-system/root/.nix-channels
drwxr-xr-x 2 root root  47 Jan  1  1980 .nix-defexpr
lrwxrwxrwx 1 root root  29 Jan  1  1980 .nix-profile -> /nix/var/nix/profiles/default

/store:
total 8
dr-xr-xr-x 3 root root  48 Oct  9 10:05 .
drwxr-xr-x 1 root root 160 Oct  9 10:08 ..
dr-xr-xr-x 2 root root  43 Oct  9 10:05 abc-hello

/tmp:
total 0
drwxrwxrwt 1 root root  80 Oct  9 10:08 .
drwxr-xr-x 1 root root 160 Oct  9 10:08 ..
drwxr-xr-x 2 root root  40 Oct  9 10:09 bundle-root
drwx------ 2 root root  60 Oct  9 10:09 runsc-root
--stderr--

### A3 sandbox --write overlay then host check
instance=8fa62d2a8f06 boot=47c54eba-f7ad-465e-b8df-9d75106f64ba rc=0 secs=0.234
--stdout--
bad
--stderr--

### A4 host: overlay write visible?
instance=8fa62d2a8f06 boot=47c54eba-f7ad-465e-b8df-9d75106f64ba rc=0 secs=0.006
--stdout--
total 16
drwxr-xr-x 2 root root   95 Oct  9 10:05 .
drwxr-xr-x 1 root root  160 Oct  9 10:08 ..
-rwxr-xr-x 1 root root  741 Oct  9 07:21 probe-nix.sh
-rw-r--r-- 1 root root 1817 Oct  9 07:21 server.py
-rw-r--r-- 1 root root  171 Oct  9 07:21 trivial.nix
--stderr--

### A5 sandbox: readonly bind of /store, rw bind of /work
instance=8fa62d2a8f06 boot=47c54eba-f7ad-465e-b8df-9d75106f64ba rc=0 secs=0.269
--stdout--
/mnt/store:
total 2
dr-xr-xr-x 3 root root 48 Oct  9 10:05 .
drwxr-xr-x 4 root root 80 Oct  9 10:09 ..
dr-xr-xr-x 2 root root 43 Oct  9 10:05 abc-hello

/mnt/work:
total 2
drwxrwxrwt 1 root root 100 Oct  9 10:09 .
drwxr-xr-x 4 root root  80 Oct  9 10:09 ..
drwxrwxrwt 2 root root  40 Oct  9 10:09 bundle-root
-rw-r--r-- 1 root root   3 Oct  9 10:09 out
drwxrwxrwt 2 root root  40 Oct  9 10:09 runsc-root
--stderr--
bash: line 1: /mnt/store/leak: Permission denied

### A6 host: rw bind write visible, store untouched?
instance=8fa62d2a8f06 boot=47c54eba-f7ad-465e-b8df-9d75106f64ba rc=0 secs=0.011
--stdout--
/store:
total 8
dr-xr-xr-x 3 root root  48 Oct  9 10:05 .
drwxr-xr-x 1 root root 180 Oct  9 10:09 ..
dr-xr-xr-x 2 root root  43 Oct  9 10:05 abc-hello

/tmp:
total 4
drwxrwxrwt 1 root root 100 Oct  9 10:09 .
drwxr-xr-x 1 root root 180 Oct  9 10:09 ..
drwxr-xr-x 2 root root  40 Oct  9 10:09 bundle-root
-rw-r--r-- 1 root root   3 Oct  9 10:09 out
drwx------ 2 root root  60 Oct  9 10:09 runsc-root
ok
--stderr--

### A7 sandbox: next sandbox sees previous sandbox state?
instance=8fa62d2a8f06 boot=47c54eba-f7ad-465e-b8df-9d75106f64ba rc=1 secs=0.306
--stdout--
  PID COMMAND
    1 /usr/local/gcp/bin/sandbox default-entrypoint
    7 bash -c ps -eo pid,args; ls -la /tmp; cat /tmp/leak-sandbox-tmp 2>&1
   11 ps -eo pid,args
total 1
drwxrwxrwt 4 root root  80 Oct  9 10:09 .
drwxr-xr-x 1 root root 180 Oct  9 10:09 ..
drwxrwxrwt 2 root root  40 Oct  9 10:09 bundle-root
drwxrwxrwt 2 root root  40 Oct  9 10:09 runsc-root
cat: /tmp/leak-sandbox-tmp: No such file or directory
--stderr--
Error: failed to exec in container: cmd.Wait(exec) failed: exit status 1


### A8 sandbox: metadata server + network
instance=8fa62d2a8f06 boot=47c54eba-f7ad-465e-b8df-9d75106f64ba rc=1 secs=0.472
--stdout--
curl: (7) Failed to connect to metadata.google.internal port 80 after 1 ms: Could not connect to server

curl: (6) Could not resolve host: www.google.com
000
curl: (7) Failed to connect to 169.254.169.254 port 80 after 0 ms: Could not connect to server
--stderr--
Error: failed to exec in container: cmd.Wait(exec) failed: exit status 7


### A9 host baseline: metadata reachable from host container
instance=8fa62d2a8f06 boot=47c54eba-f7ad-465e-b8df-9d75106f64ba rc=0 secs=0.013
--stdout--
turnkey-cr-iso-20261009--stderr--

### B1 sandbox: fill memory (6 GiB on a 4 GiB instance)
instance=8fa62d2a8f06 boot=47c54eba-f7ad-465e-b8df-9d75106f64ba rc=1 secs=5.524
--stdout--
--stderr--
waiting on pid 10: waiting on PID 10 in sandbox "bf7fe14a-3f81-4604-85df-e8e77c71f272": urpc method "containerManager.WaitPID" failed: EOF
Error: failed to exec in container: cmd.Wait(exec) failed: exit status 128


### B2 host: still the same instance after memory fill?
instance=8fa62d2a8f06 boot=47c54eba-f7ad-465e-b8df-9d75106f64ba rc=0 secs=0.06
--stdout--
 10:10:05  up   0:01,  0 users,  load average: 0.27, 0.07, 0.02
               total        used        free      shared  buff/cache   available
Mem:            3913         290        3731           0          17        3622
Swap:              0           0           0
--stderr--

### B3 sandbox: fill disk via --write overlay (8 GiB)
instance=8fa62d2a8f06 boot=47c54eba-f7ad-465e-b8df-9d75106f64ba rc=1 secs=8.273
--stdout--
--stderr--
waiting on pid 7: waiting on PID 7 in sandbox "50b32836-4b72-47ce-acd6-5c383a2cc7e8": urpc method "containerManager.WaitPID" failed: EOF
Error: failed to exec in container: cmd.Wait(exec) failed: exit status 128


### B4 host: same instance, memory back?
instance=8fa62d2a8f06 boot=47c54eba-f7ad-465e-b8df-9d75106f64ba rc=0 secs=0.033
--stdout--
               total        used        free      shared  buff/cache   available
Mem:            3913         287        3736           0          14        3626
Swap:              0           0           0
Filesystem      Size  Used Avail Use% Mounted on
none            4.0G  4.0K  4.0G   1% /
--stderr--

### B5 sandbox: fork bomb, bounded by timeout
instance=8fa62d2a8f06 boot=47c54eba-f7ad-465e-b8df-9d75106f64ba rc=1 secs=30.89
--stdout--
1024
--stderr--
bash: fork: retry: Resource temporarily unavailable
bash: fork: retry: Resource temporarily unavailable
bash: fork: retry: Resource temporarily unavailable
bash: fork: retry: Resource temporarily unavailable
bash: fork: Interrupted system call
Error: failed to exec in container: cmd.Wait(exec) failed: exit status 254


### B6 host: survivors after fork bomb?
instance=8fa62d2a8f06 boot=47c54eba-f7ad-465e-b8df-9d75106f64ba rc=0 secs=0.018
--stdout--
5
               total        used        free      shared  buff/cache   available
Mem:            3913         312        3709           0          18        3601
Swap:              0           0           0
--stderr--

### C1 host container (root): namespace + nix probes
instance=8fa62d2a8f06 boot=47c54eba-f7ad-465e-b8df-9d75106f64ba rc=0 secs=0.956
--stdout--
== whoami uid=0(root) gid=0(root) groups=0(root) kernel 6.9.12
== unshare-U rc=0 ::  
== unshare-Urm rc=0 ::  
== unshare-Urmpn rc=0 ::  
== bwrap rc=0 ::  
== dev-fuse rc=0 :: crw-rw-rw- 1 root root 10, 229 Oct  9 10:08 /dev/fuse 
== seccomp rc=0 :: NoNewPrivs:	1 Seccomp:	2 Seccomp_filters:	1 
== nix-sandbox rc=0 ::   /nix/store/lrs7icv5ckwajvr0zxfmrbh5yx33flqf-trivial-1791540661.drv building '/nix/store/lrs7icv5ckwajvr0zxfmrbh5yx33flqf-trivial-1791540661.drv'... /nix/store/i662nv0yhnzl23lmhrxlvbvndc7r0xna-trivial-1791540661 
== nix-nosandbox rc=0 :: /nix/store/i662nv0yhnzl23lmhrxlvbvndc7r0xna-trivial-1791540661 
--stderr--

### C2 host container as non-root uid 1000 (setpriv): namespaces, store perms
instance=8fa62d2a8f06 boot=47c54eba-f7ad-465e-b8df-9d75106f64ba rc=0 secs=0.078
--stdout--
uid=1000 gid=1000 groups=1000
unshare-U rc=0
bwrap rc=0
store-write rc=1
hello
--stderr--
touch: cannot touch '/store/x': Permission denied

### C3 sandbox --write (root in gVisor): namespace + nix probes
instance=8fa62d2a8f06 boot=47c54eba-f7ad-465e-b8df-9d75106f64ba rc=0 secs=10.402
--stdout--
== whoami uid=0(root) gid=0(root) groups=0(root) kernel 4.19.0-gvisor
== unshare-U rc=1 :: unshare: write failed /proc/self/uid_map: Operation not permitted 
== unshare-Urm rc=1 :: unshare: write failed /proc/self/uid_map: Operation not permitted 
== unshare-Urmpn rc=1 :: unshare: write failed /proc/self/uid_map: Operation not permitted 
== bwrap rc=1 :: bwrap: loopback: Failed RTM_NEWADDR: No such file or directory 
== dev-fuse rc=0 :: crw-rw-rw- 1 root root 10, 229 Oct  9 10:11 /dev/fuse 
== seccomp rc=0 :: CapEff:	0000000020000420 NoNewPrivs:	0 Seccomp:	0 
== nix-sandbox rc=1 ::        … while setting up the build environment         error: setgroups failed. Set the require-drop-supplementary-groups option to false to skip this step. 
== nix-nosandbox rc=1 ::        … while waiting for the build environment for '/nix/store/sn2hv7yyb2laszhdnpgag6mmg4vzah3z-trivial-1791540669.drv' to initialize (succeeded, previous messages: )         error: reading a line: Input/output error 
--stderr--

### C4 sandbox --write: nix sandbox with require-drop-supplementary-groups=false
instance=8fa62d2a8f06 boot=47c54eba-f7ad-465e-b8df-9d75106f64ba rc=0 secs=9.854
--stdout--
error:
       … writing file '/proc/25/uid_map'

       error: writing to file: Operation not permitted

error:
       … while waiting for the build environment for '/nix/store/c8phwmshm6pb1sdjkf554097m7ac77q1-trivial-1791540689.drv' to initialize (succeeded, previous messages: )

       error: reading a line: Input/output error
--stderr--

### D1 latency: 10x host 'true' vs 10x sandbox 'true' (server-side secs)
sandbox=false: 0.004 0.005 0.004 0.005 0.005 0.004 0.006 0.004 0.006 0.004
sandbox=true: 0.231 0.255 0.246 0.258 0.353 0.279 0.338 0.255 0.235 0.362
```

Job probe (`sh /app/probe-nix.sh` as the job command):

```text
== whoami uid=0(root) gid=0(root) groups=0(root) kernel 6.9.12
== unshare-U rc=0 ::  
== unshare-Urm rc=0 ::  
== unshare-Urmpn rc=0 ::  
== bwrap rc=0 ::  
== dev-fuse rc=0 :: crw-rw-rw- 1 root root 10, 229 Oct  9 10:03 /dev/fuse 
== seccomp rc=0 :: NoNewPrivs:	1 Seccomp:	2 Seccomp_filters:	1 
== nix-sandbox rc=0 ::   /nix/store/psslhiinwfn730lbrbn5ifby01zvwp4k-trivial-1791540768.drv building '/nix/store/psslhiinwfn730lbrbn5ifby01zvwp4k-trivial-1791540768.drv'... /nix/store/2amab9mva3h6vqwp7wqxbbwm49xm4p92-trivial-1791540768 
== nix-nosandbox rc=0 :: /nix/store/2amab9mva3h6vqwp7wqxbbwm49xm4p92-trivial-1791540768 
Container called exit(0).
```

## Harness

`server.py`:

```python
# Throwaway Cloud Run isolation experiment server (turnkey #76).
# POST /run {"cmd": "...", "sandbox": bool, "args": [...]} -> runs bash -c cmd,
# directly or inside `sandbox do`, and reports which instance ran it.
import json, os, subprocess, time, urllib.request
from http.server import BaseHTTPRequestHandler, HTTPServer

def instance_id():
    try:
        req = urllib.request.Request(
            "http://metadata.google.internal/computeMetadata/v1/instance/id",
            headers={"Metadata-Flavor": "Google"})
        return urllib.request.urlopen(req, timeout=2).read().decode()
    except Exception as e:
        return f"err:{e}"

INSTANCE = instance_id()
BOOT = open("/proc/sys/kernel/random/boot_id").read().strip()

class H(BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        argv = ["bash", "-c", body["cmd"]]
        if body.get("sandbox"):
            argv = ["/usr/local/gcp/bin/sandbox", "do", *body.get("args", []), "--", *argv]
        t = time.monotonic()
        try:
            r = subprocess.run(argv, capture_output=True, text=True, timeout=body.get("timeout", 120))
            rc, out, err = r.returncode, r.stdout, r.stderr
        except subprocess.TimeoutExpired as e:
            rc, out, err = "timeout", str(e.stdout), str(e.stderr)
        res = {"instance": INSTANCE, "boot": BOOT, "rc": rc,
               "secs": round(time.monotonic() - t, 3), "stdout": out[-4000:], "stderr": err[-4000:]}
        data = json.dumps(res).encode()
        self.send_response(200); self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data))); self.end_headers(); self.wfile.write(data)

HTTPServer(("", int(os.environ.get("PORT", 8080))), H).serve_forever()
```

`probe-nix.sh`:

```sh
#!/bin/sh
# Namespace and Nix-sandbox probes; prints one labelled result per probe.
p() { label=$1; shift; out=$("$@" 2>&1); echo "== $label rc=$? :: $(echo "$out" | tail -3 | tr '\n' ' ')"; }
echo "== whoami $(id) kernel $(uname -r)"
p unshare-U          unshare -Ur true
p unshare-Urm        unshare -Urm --fork true
p unshare-Urmpn      unshare -Urmpn --fork true
p bwrap              bwrap --unshare-all --dev-bind / / true
p dev-fuse           ls -l /dev/fuse
p seccomp            grep -E 'Seccomp|NoNewPrivs|CapEff' /proc/self/status
p nix-sandbox        nix-build --option sandbox true --option sandbox-fallback false /app/trivial.nix --no-out-link
p nix-nosandbox      nix-build --option sandbox false /app/trivial.nix --no-out-link
```

`Dockerfile`:

```dockerfile
FROM nixos/nix:2.28.3
RUN nix-env -iA nixpkgs.python3 nixpkgs.bubblewrap nixpkgs.util-linux nixpkgs.procps nixpkgs.curl nixpkgs.fuse3 \
 && mkdir -p /store/abc-hello && echo hello > /store/abc-hello/file && chmod -R a-w /store \
 && mkdir -p /etc/nix && printf 'experimental-features = nix-command flakes\nbuild-users-group =\n' >> /etc/nix/nix.conf
COPY server.py probe-nix.sh trivial.nix /app/
ENV PORT=8080
CMD ["python3", "/app/server.py"]
```

"""Parity harness: run a Go tool and its Rust port on the same inputs, diff.

Temporary: each port's switch deletes its tool's case file, and the tk
switch deletes the harness (#204, #207, #209). Run it by hand:

    nix run .#parity -- <tool>               # Go package against Rust package
    nix run .#parity -- --self-check <tool>  # Go package against itself

The Nix wrapper (default.nix) passes --config, a JSON file naming the cases
directory, the source tree the case inputs are read from, the real tools
put on the child's PATH after the stubs, and each tool's Go and Rust
package derivations. See README.md for the case-file format.

A stub is this very file, run by a generated script named after the tool
it stands in for (`parity.py stub <spec> <name> args...`).
"""

import argparse
import ast
import difflib
import fcntl
import hashlib
import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import tomllib

# Every copied input gets this mtime, so mtime-based staleness (tk sync,
# syncer) sees the same thing on both sides: 2000-01-01T00:00:00Z
FIXED_MTIME = 946684800

# Never copied out of an input tree: VCS state and the dev shell's
# generated, machine-specific state
DEFAULT_EXCLUDES = [".git", ".jj", ".direnv", ".devenv", "buck-out", "result"]

# Variables the OS sets on a process it starts, whatever its parent says
PLATFORM_ENV = ["__CF_USER_TEXT_ENCODING"]

COMPARE_MODES = ["bytes", "toml", "json", "starlark"]

NIX_STORE = b"/nix/store/"
NIX32 = frozenset(b"0123456789abcdfghijklmnpqrsvwxyz")
NIX_HASH_LEN = 32


class CaseError(Exception):
    """A case file that doesn't say what the harness needs."""


# --- case files ------------------------------------------------------------


def check_keys(where, table, required, optional):
    if not isinstance(table, dict):
        raise CaseError(f"{where}: expected a table")
    missing = [k for k in required if k not in table]
    if missing:
        raise CaseError(f"{where}: missing {', '.join(missing)}")
    unknown = sorted(set(table) - set(required) - set(optional))
    if unknown:
        raise CaseError(f"{where}: unknown key(s) {', '.join(unknown)}")


def check_relpath(where, path):
    """A path relative to a side's root, staying inside it."""
    if not isinstance(path, str) or not path:
        raise CaseError(f"{where}: expected a non-empty relative path")
    parts = path.split("/")
    if path.startswith("/") or ".." in parts:
        raise CaseError(f"{where}: {path!r} must stay inside the side's root")
    return path


def check_strings(where, value):
    if not isinstance(value, list) or not all(isinstance(v, str) for v in value):
        raise CaseError(f"{where}: expected a list of strings")
    return value


def load_case_file(path, tool):
    with open(path, "rb") as f:
        doc = tomllib.load(f)
    where = os.path.basename(path)
    check_keys(where, doc, ["tool", "cases"], [])
    check_keys(f"{where} [tool]", doc["tool"], ["go"], ["rust", "bin"])
    bin_name = doc["tool"].get("bin", tool)
    cases = doc["cases"]
    if not isinstance(cases, list) or not cases:
        raise CaseError(f"{where}: [[cases]] must list at least one case")
    names = set()
    for i, case in enumerate(cases):
        cw = f"{where} cases[{i}]"
        check_keys(
            cw,
            case,
            ["name", "args"],
            [
                "description",
                "inputs",
                "exclude",
                "files",
                "cwd",
                "env",
                "inherit_env",
                "exit",
                "timeout",
                "outputs",
                "stubs",
            ],
        )
        if case["name"] in names:
            raise CaseError(f"{cw}: duplicate case name {case['name']!r}")
        names.add(case["name"])
        check_strings(f"{cw}.args", case["args"])
        for dest, src in case.get("inputs", {}).items():
            check_relpath(f"{cw}.inputs", dest)
            if not isinstance(src, str):
                raise CaseError(f"{cw}.inputs.{dest}: expected a source path")
        check_strings(f"{cw}.exclude", case.get("exclude", []))
        for dest, content in case.get("files", {}).items():
            check_relpath(f"{cw}.files", dest)
            if not isinstance(content, str):
                raise CaseError(f"{cw}.files.{dest}: expected the file's content")
        check_relpath(f"{cw}.cwd", case.get("cwd", "work"))
        for k, v in case.get("env", {}).items():
            if not isinstance(v, str):
                raise CaseError(f"{cw}.env.{k}: expected a string")
        check_strings(f"{cw}.inherit_env", case.get("inherit_env", []))
        if "exit" in case and not isinstance(case["exit"], int):
            raise CaseError(f"{cw}.exit: expected an integer")
        for j, out in enumerate(case.get("outputs", [])):
            ow = f"{cw}.outputs[{j}]"
            check_keys(ow, out, [], ["path", "stream", "compare"])
            if ("path" in out) == ("stream" in out):
                raise CaseError(f"{ow}: give exactly one of path or stream")
            if "path" in out:
                check_relpath(f"{ow}.path", out["path"])
            elif out["stream"] != "stdout":
                raise CaseError(f"{ow}.stream: only stdout is compared")
            if out.get("compare", "bytes") not in COMPARE_MODES:
                raise CaseError(f"{ow}.compare: one of {', '.join(COMPARE_MODES)}")
        for name, responses in case.get("stubs", {}).items():
            sw = f"{cw}.stubs.{name}"
            if not name or "/" in name:
                raise CaseError(f"{sw}: a stub is named after a command")
            if not isinstance(responses, list):
                raise CaseError(f"{sw}: expected an array of responses")
            for k, resp in enumerate(responses):
                rw = f"{sw}[{k}]"
                check_keys(
                    rw,
                    resp,
                    [],
                    ["args", "args_prefix", "exit", "stdout", "stderr", "write"],
                )
                for key in ("args", "args_prefix"):
                    if key in resp:
                        check_strings(f"{rw}.{key}", resp[key])
                for dest in resp.get("write", {}):
                    check_relpath(f"{rw}.write", dest)
    return {"bin": bin_name, "decl": doc["tool"], "cases": cases}


# --- normalisation ---------------------------------------------------------


def normalise_store_hashes(data):
    """Replace the hash part of every /nix/store/<hash>-<name> with <hash>.

    A tool built differently (the Rust buckgen) builds different
    derivations, so the store paths it leads to differ in their hash only.
    """
    out = bytearray()
    i = 0
    while True:
        j = data.find(NIX_STORE, i)
        if j < 0:
            out += data[i:]
            return bytes(out)
        h = j + len(NIX_STORE)
        candidate = data[h : h + NIX_HASH_LEN]
        if (
            len(candidate) == NIX_HASH_LEN
            and all(c in NIX32 for c in candidate)
            and data[h + NIX_HASH_LEN : h + NIX_HASH_LEN + 1] == b"-"
        ):
            out += data[i:h] + b"<hash>"
            i = h + NIX_HASH_LEN
        else:
            out += data[i:h]
            i = h


class Normaliser:
    """Rewrites one side's own paths to placeholders both sides share."""

    def __init__(self, replacements):
        # Longest first, so a path inside another is replaced as itself
        self.pairs = sorted(
            ((k.encode(), v.encode()) for k, v in replacements.items()),
            key=lambda kv: -len(kv[0]),
        )

    def bytes(self, data):
        for old, new in self.pairs:
            data = data.replace(old, new)
        return normalise_store_hashes(data)

    def str(self, text):
        return self.bytes(text.encode()).decode("utf-8", "surrogateescape")


# --- trees -----------------------------------------------------------------


def copy_input(src, dest, excludes):
    ignore = shutil.ignore_patterns(*excludes) if excludes else None
    if os.path.isdir(src):
        shutil.copytree(src, dest, symlinks=True, ignore=ignore, dirs_exist_ok=True)
    else:
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        shutil.copy2(src, dest, follow_symlinks=False)


def settle(root):
    """Make the copied tree writable (store sources are read-only) and give
    every entry the same mtime."""
    for dirpath, dirnames, filenames in os.walk(root, topdown=False):
        for name in filenames + dirnames:
            p = os.path.join(dirpath, name)
            st = os.lstat(p)
            if stat.S_ISLNK(st.st_mode):
                os.utime(p, (FIXED_MTIME, FIXED_MTIME), follow_symlinks=False)
                continue
            os.chmod(p, stat.S_IMODE(st.st_mode) | stat.S_IWUSR | stat.S_IRUSR)
            os.utime(p, (FIXED_MTIME, FIXED_MTIME))
    os.utime(root, (FIXED_MTIME, FIXED_MTIME))


def snapshot(root):
    """Every entry under root: relpath -> (kind, detail)."""
    entries = {}
    for dirpath, dirnames, filenames in os.walk(root):
        for name in dirnames + filenames:
            p = os.path.join(dirpath, name)
            rel = os.path.relpath(p, root)
            st = os.lstat(p)
            if stat.S_ISLNK(st.st_mode):
                entries[rel] = ("symlink", os.readlink(p))
            elif stat.S_ISDIR(st.st_mode):
                entries[rel] = ("dir", None)
            else:
                with open(p, "rb") as f:
                    digest = hashlib.sha256(f.read()).hexdigest()
                exe = bool(st.st_mode & stat.S_IXUSR)
                entries[rel] = ("file", (digest, exe))
    return entries


def written(before, after):
    """The entries a run added, changed or removed: relpath -> what."""
    changes = {}
    for rel in sorted(set(before) | set(after)):
        b, a = before.get(rel), after.get(rel)
        if b == a:
            continue
        if b is None:
            changes[rel] = "added"
        elif a is None:
            changes[rel] = "removed"
        elif b[0] == "dir" and a[0] == "dir":
            continue
        else:
            changes[rel] = "changed"
    return changes


# --- stubs -----------------------------------------------------------------


def write_stubs(private, root, base_env, stubs):
    stub_dir = os.path.join(private, "stubs")
    os.makedirs(stub_dir)
    spec = {
        "root": root,
        "log": os.path.join(private, "calls.jsonl"),
        "base_env": base_env,
        "responses": stubs,
    }
    spec_path = os.path.join(private, "stubs.json")
    with open(spec_path, "w") as f:
        json.dump(spec, f)
    me = os.path.abspath(__file__)
    for name in stubs:
        path = os.path.join(stub_dir, name)
        # A Python script, not a shell one: a shell sets PWD, SHLVL and _,
        # which would read as env the tool set
        with open(path, "w") as f:
            f.write(
                f"#!{sys.executable}\n"
                "import runpy, sys\n"
                f"sys.argv = [{me!r}, 'stub', {spec_path!r}, {name!r}] + sys.argv[1:]\n"
                f"runpy.run_path({me!r}, run_name='__main__')\n"
            )
        os.chmod(path, 0o755)
    return stub_dir


def stub_main(spec_path, name, args):
    """Record this call, then replay the first response it matches."""
    with open(spec_path) as f:
        spec = json.load(f)
    base = spec["base_env"]
    env = dict(os.environ)
    record = {
        "command": name,
        "argv": args,
        "cwd": os.getcwd(),
        "env_set": {k: v for k, v in sorted(env.items()) if base.get(k) != v},
        "env_unset": sorted(k for k in base if k not in env),
    }
    with open(spec["log"], "a") as log:
        fcntl.flock(log, fcntl.LOCK_EX)
        log.write(json.dumps(record) + "\n")
    response = {}
    for candidate in spec["responses"][name]:
        if "args" in candidate and candidate["args"] != args:
            continue
        prefix = candidate.get("args_prefix")
        if prefix is not None and args[: len(prefix)] != prefix:
            continue
        response = candidate
        break
    for rel, content in response.get("write", {}).items():
        dest = os.path.join(spec["root"], rel)
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        with open(dest, "w") as f:
            f.write(content)
    sys.stdout.write(response.get("stdout", ""))
    sys.stderr.write(response.get("stderr", ""))
    sys.stdout.flush()
    sys.stderr.flush()
    return response.get("exit", 0)


# --- running one side ------------------------------------------------------


def expand(value, places):
    for key, path in places.items():
        value = value.replace("{" + key + "}", path)
    return value


def run_side(label, program, pkg, case, config, scratch):
    side = os.path.join(scratch, label)
    root = os.path.join(side, "root")
    private = os.path.join(side, "private")
    tmp = os.path.join(private, "tmp")
    for d in (os.path.join(root, "work"), os.path.join(root, "out"), tmp):
        os.makedirs(d)
    places = {
        "root": root,
        "work": os.path.join(root, "work"),
        "out": os.path.join(root, "out"),
    }

    excludes = DEFAULT_EXCLUDES + case.get("exclude", [])
    for dest, src in case.get("inputs", {}).items():
        src_path = os.path.normpath(os.path.join(config["source"], src))
        copy_input(src_path, os.path.join(root, dest), excludes)
    # Writable before the files are laid over the inputs
    settle(root)
    for dest, content in case.get("files", {}).items():
        path = os.path.join(root, dest)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w") as f:
            f.write(content)
    settle(root)

    stubs = case.get("stubs", {})
    env = {
        "HOME": os.environ.get("HOME", private),
        "TMPDIR": tmp,
        "SSL_CERT_FILE": config["sslCertFile"],
        "LANG": "C.UTF-8",
    }
    # macOS adds this one to every process it starts that lacks it, so a
    # stub would record it as set by the tool
    for key in PLATFORM_ENV:
        if key in os.environ:
            env[key] = os.environ[key]
    for key in case.get("inherit_env", []):
        if key in os.environ:
            env[key] = os.environ[key]
    stub_dir = os.path.join(private, "stubs")
    env["PATH"] = os.pathsep.join([stub_dir, config["toolsPath"]])
    for key, value in case.get("env", {}).items():
        env[key] = expand(value, places)
    if stubs:
        write_stubs(private, root, env, stubs)
    else:
        os.makedirs(stub_dir)

    before = snapshot(root)
    argv = [program] + [expand(a, places) for a in case["args"]]
    try:
        proc = subprocess.run(
            argv,
            cwd=os.path.join(root, case.get("cwd", "work")),
            env=env,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            timeout=case.get("timeout", 600),
        )
        exit_code, stdout, stderr = proc.returncode, proc.stdout, proc.stderr
    except subprocess.TimeoutExpired as e:
        exit_code, stdout = "timeout", e.stdout or b""
        stderr = (e.stderr or b"") + b"\nparity: timed out\n"
    after = snapshot(root)

    calls = []
    log = os.path.join(private, "calls.jsonl")
    if os.path.exists(log):
        with open(log) as f:
            calls = [json.loads(line) for line in f if line.strip()]

    replacements = {root: "<root>", private: "<private>"}
    if pkg:
        replacements[pkg] = "<pkg>"
    return {
        "label": label,
        "root": root,
        "exit": exit_code,
        "stdout": stdout,
        "stderr": stderr,
        "after": after,
        "written": written(before, after),
        "calls": calls,
        "norm": Normaliser(replacements),
    }


# --- comparing -------------------------------------------------------------


def to_plain(value):
    """TOML values as JSON-comparable ones (datetimes as their text)."""
    if isinstance(value, dict):
        return {k: to_plain(v) for k, v in value.items()}
    if isinstance(value, list):
        return [to_plain(v) for v in value]
    if isinstance(value, (str, int, float, bool)) or value is None:
        return value
    return str(value)


def canonical(mode, data):
    """The text two outputs are compared by, under mode.

    bytes compares the (normalised) bytes; the parsed modes compare what a
    parser makes of them, so formatting, ordering of table keys and (for
    Starlark) comments don't count.
    """
    text = data.decode("utf-8", "surrogateescape")
    if mode == "bytes":
        return text
    try:
        if mode == "toml":
            parsed = to_plain(tomllib.loads(text))
            return json.dumps(parsed, indent=2, sort_keys=True) + "\n"
        if mode == "json":
            return json.dumps(json.loads(text), indent=2, sort_keys=True) + "\n"
        if mode == "starlark":
            # Starlark's syntax is a subset of Python's: Python's own parser
            # reads any Starlark file, and its AST drops layout and comments
            return ast.dump(ast.parse(text), indent=1) + "\n"
    except (ValueError, SyntaxError) as e:
        return f"<not valid {mode}: {e}>\n"
    raise AssertionError(mode)


def text_diff(what, a_label, a, b_label, b, limit=60):
    lines = list(
        difflib.unified_diff(
            a.splitlines(keepends=True),
            b.splitlines(keepends=True),
            fromfile=f"{a_label}/{what}",
            tofile=f"{b_label}/{what}",
        )
    )
    if len(lines) > limit:
        lines = lines[:limit] + [f"... {len(lines) - limit} more diff lines\n"]
    return "".join(line if line.endswith("\n") else line + "\n" for line in lines)


def mode_for(path, outputs):
    """The compare mode of the most specific declared output holding path."""
    best, mode = -1, "bytes"
    for out in outputs:
        p = out.get("path")
        if p is None:
            continue
        if (path == p or path.startswith(p + "/")) and len(p) > best:
            best, mode = len(p), out.get("compare", "bytes")
    return mode


def read_entry(side, rel):
    entry = side["after"].get(rel)
    if entry is None:
        return None, None
    kind = entry[0]
    if kind == "symlink":
        return kind, side["norm"].str(entry[1])
    if kind == "dir":
        return kind, None
    with open(os.path.join(side["root"], rel), "rb") as f:
        return kind, (side["norm"].bytes(f.read()), entry[1][1])


def compare(case, a, b):
    diffs = []
    al, bl = a["label"], b["label"]

    expected = case.get("exit")
    for side in (a, b):
        if expected is not None and side["exit"] != expected:
            diffs.append(
                f"exit code: {side['label']} exited {side['exit']}, "
                f"the case expects {expected}"
            )
    if a["exit"] != b["exit"]:
        diffs.append(f"exit code: {al} {a['exit']}, {bl} {b['exit']}")

    outputs = case.get("outputs", [])
    for out in outputs:
        if out.get("stream") == "stdout":
            mode = out.get("compare", "bytes")
            sa = canonical(mode, a["norm"].bytes(a["stdout"]))
            sb = canonical(mode, b["norm"].bytes(b["stdout"]))
            if sa != sb:
                diffs.append(
                    f"stdout ({mode}):\n" + text_diff("stdout", al, sa, bl, sb)
                )

    wa, wb = a["written"], b["written"]
    if wa != wb:
        lines = [
            f"  {p}: {al} {wa.get(p, 'untouched')}, {bl} {wb.get(p, 'untouched')}"
            for p in sorted(set(wa) | set(wb))
            if wa.get(p) != wb.get(p)
        ]
        diffs.append("files written:\n" + "\n".join(lines))

    paths = {p for p, what in wa.items() if what != "removed"}
    paths |= {p for p, what in wb.items() if what != "removed"}
    for out in outputs:
        p = out.get("path")
        if p is None:
            continue
        under = {q for side in (a, b) for q in side["after"] if q.startswith(p + "/")}
        paths |= under | {p}
    for rel in sorted(paths):
        mode = mode_for(rel, outputs)
        ka, va = read_entry(a, rel)
        kb, vb = read_entry(b, rel)
        if ka is None or kb is None:
            if ka != kb:
                missing = al if ka is None else bl
                diffs.append(f"{rel}: missing on {missing}")
            elif any(o.get("path") == rel for o in outputs):
                diffs.append(f"{rel}: declared output missing on both sides")
            continue
        if ka != kb:
            diffs.append(f"{rel}: {al} has a {ka}, {bl} a {kb}")
        elif ka == "symlink" and va != vb:
            diffs.append(f"{rel}: symlink to {va!r} on {al}, {vb!r} on {bl}")
        elif ka == "file":
            if va[1] != vb[1]:
                diffs.append(f"{rel}: executable on one side only")
            ca, cb = canonical(mode, va[0]), canonical(mode, vb[0])
            if ca != cb:
                diffs.append(f"{rel} ({mode}):\n" + text_diff(rel, al, ca, bl, cb))

    ca = calls_text(a)
    cb = calls_text(b)
    if ca != cb:
        diffs.append("child processes:\n" + text_diff("calls", al, ca, bl, cb))
    return diffs


def calls_text(side):
    norm = side["norm"]
    lines = []
    for call in side["calls"]:
        call = {
            "command": call["command"],
            "argv": [norm.str(x) for x in call["argv"]],
            "cwd": norm.str(call["cwd"]),
            "env_set": {k: norm.str(v) for k, v in call["env_set"].items()},
            "env_unset": call["env_unset"],
        }
        lines.append(json.dumps(call, sort_keys=True) + "\n")
    return "".join(lines)


def remove_tree(path):
    # A tool may leave read-only directories behind (Go's module cache does)
    for dirpath, dirnames, _ in os.walk(path):
        for name in dirnames:
            p = os.path.join(dirpath, name)
            if not os.path.islink(p):
                os.chmod(p, stat.S_IRWXU)
    shutil.rmtree(path, ignore_errors=True)


# --- the command -----------------------------------------------------------


def realise(drv):
    """The out path of drv, built (or substituted) if need be."""
    proc = subprocess.run(
        ["nix", "build", "--quiet", "--no-link", "--print-out-paths", drv + "^out"],
        stdout=subprocess.PIPE,
        text=True,
    )
    if proc.returncode != 0:
        raise SystemExit(f"parity: building {drv} failed")
    return proc.stdout.strip()


def resolve_program(path, bin_name):
    """--go/--rust PATH: an executable, or a package with bin/<bin>."""
    path = os.path.abspath(path)
    if os.path.isdir(path):
        return os.path.join(path, "bin", bin_name), path
    return path, None


def tail(data, n=15):
    lines = data.decode("utf-8", "replace").splitlines()
    return "\n".join("    " + line for line in lines[-n:])


def main(argv):
    if argv[:1] == ["stub"]:
        return stub_main(argv[1], argv[2], argv[3:])

    parser = argparse.ArgumentParser(
        prog="parity",
        description="Run a Go tool and its Rust port on the same inputs and diff their outputs.",
    )
    parser.add_argument("--config", required=True, help=argparse.SUPPRESS)
    parser.add_argument(
        "tool", nargs="?", help="the tool to compare (a case file's name)"
    )
    parser.add_argument(
        "--list", action="store_true", help="list the tools and their cases"
    )
    parser.add_argument(
        "--self-check",
        action="store_true",
        help="run the Go package against itself: must come out clean",
    )
    parser.add_argument(
        "--case", action="append", help="run only this case (repeatable)"
    )
    parser.add_argument(
        "--go", metavar="PATH", help="use this Go build (package or executable)"
    )
    parser.add_argument(
        "--rust",
        metavar="PATH",
        help="use this Rust build (package or executable), e.g. a cargo target dir binary",
    )
    parser.add_argument(
        "--keep", action="store_true", help="keep the scratch directory"
    )
    opts = parser.parse_args(argv)

    with open(opts.config) as f:
        config = json.load(f)
    cases_dir = config["casesDir"]
    tools = sorted(config["packages"])

    if opts.list or not opts.tool:
        for tool in tools:
            cf = load_case_file(os.path.join(cases_dir, tool + ".toml"), tool)
            rust = cf["decl"].get("rust", "(no Rust package yet)")
            print(f"{tool}: go={cf['decl']['go']} rust={rust}")
            for case in cf["cases"]:
                print(f"  {case['name']}")
        return 0 if opts.list else 2
    if opts.tool not in config["packages"]:
        print(
            f"parity: no case file for {opts.tool!r} (have: {', '.join(tools)})",
            file=sys.stderr,
        )
        return 2

    try:
        cf = load_case_file(os.path.join(cases_dir, opts.tool + ".toml"), opts.tool)
    except CaseError as e:
        print(f"parity: {e}", file=sys.stderr)
        return 2
    bin_name = cf["bin"]
    drvs = config["packages"][opts.tool]

    if opts.go:
        go_prog, go_pkg = resolve_program(opts.go, bin_name)
    else:
        go_pkg = realise(drvs["go"])
        go_prog = os.path.join(go_pkg, "bin", bin_name)
    if opts.self_check:
        if opts.rust:
            print(
                "parity: --self-check runs Go against itself; drop --rust",
                file=sys.stderr,
            )
            return 2
        sides = [("go", go_prog, go_pkg), ("go-again", go_prog, go_pkg)]
    else:
        if opts.rust:
            rust_prog, rust_pkg = resolve_program(opts.rust, bin_name)
        elif drvs["rust"]:
            rust_pkg = realise(drvs["rust"])
            rust_prog = os.path.join(rust_pkg, "bin", bin_name)
        else:
            print(
                f"parity: cases/{opts.tool}.toml names no Rust package (tool.rust);"
                " pass --rust PATH, or --self-check",
                file=sys.stderr,
            )
            return 2
        sides = [("go", go_prog, go_pkg), ("rust", rust_prog, rust_pkg)]

    selected = cf["cases"]
    if opts.case:
        unknown = set(opts.case) - {c["name"] for c in selected}
        if unknown:
            print(f"parity: no case(s) {', '.join(sorted(unknown))}", file=sys.stderr)
            return 2
        selected = [c for c in selected if c["name"] in opts.case]

    print(f"parity: {opts.tool}: {sides[0][1]} against {sides[1][1]}")
    scratch = os.path.realpath(tempfile.mkdtemp(prefix=f"parity-{opts.tool}-"))
    failed = []
    try:
        for case in selected:
            case_dir = os.path.join(scratch, case["name"])
            results = [
                run_side(label, prog, pkg, case, config, case_dir)
                for label, prog, pkg in sides
            ]
            diffs = compare(case, *results)
            if not diffs:
                print(f"ok    {case['name']}")
                continue
            failed.append(case["name"])
            print(f"DIFF  {case['name']}")
            for d in diffs:
                print("  " + d.replace("\n", "\n  ").rstrip())
            for r in results:
                print(f"  stderr of {r['label']} (exit {r['exit']}, not compared):")
                print(tail(r["stderr"]) or "    (empty)")
    finally:
        if opts.keep:
            print(f"parity: scratch kept in {scratch}")
        else:
            remove_tree(scratch)

    if failed:
        print(
            f"parity: {opts.tool}: {len(failed)} of {len(selected)} case(s) differ: {', '.join(failed)}"
        )
        return 1
    print(f"parity: {opts.tool}: {len(selected)} case(s), no differences")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

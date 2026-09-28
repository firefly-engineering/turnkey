# Write-once layout: vendor/_store/<store basename> -> store path (created or
# deleted, never retargeted); vendor/<key>/rules.star and vendor/<alias>/rules.star
# are real in-project files of alias() rules, so every change the file watcher
# must see is a change to a real file
import ast, json, os, shutil, sys
idx = json.load(open(sys.argv[1]))
root = ".turnkey/rustdeps"; vendor = f"{root}/vendor"; store = f"{vendor}/_store"
os.makedirs(store, exist_ok=True)
bc = f"{root}/.buckconfig"
want_bc = "[cells]\n    rustdeps = .\n    prelude = prelude\n\n[buildfile]\n    name = rules.star\n"
if not os.path.exists(bc) or open(bc).read() != want_bc:
    open(bc, "w").write(want_bc)

def names(rules_path):
    # Target names: the name= keyword of each top-level call
    tree = ast.parse(open(rules_path).read())
    out = []
    for node in tree.body:
        if isinstance(node, ast.Expr) and isinstance(node.value, ast.Call):
            for kw in node.value.keywords:
                if kw.arg == "name" and isinstance(kw.value, ast.Constant):
                    out.append(kw.value.value)
    return out

def alias_file(pkg, names_):
    return "".join(
        f'alias(name = "{n}", actual = "//{pkg}:{n}", visibility = ["PUBLIC"])\n' for n in names_
    )

def write_if_changed(path, content):
    if os.path.exists(path) and open(path).read() == content:
        return False
    open(path, "w").write(content); return True

stats = dict(store_added=0, store_removed=0, rules_written=0, rules_unchanged=0, pkgs_removed=0)
# 1. store links: create missing, never retarget
want_store = {os.path.basename(p): p for p in idx["crates"].values()}
for b, p in want_store.items():
    lp = f"{store}/{b}"
    if not os.path.lexists(lp):
        os.symlink(p, lp); stats["store_added"] += 1
# 2. versioned packages -> alias into _store
key_names = {}
want_pkgs = {}
for key, p in idx["crates"].items():
    ns = names(f"{p}/rules.star"); key_names[key] = ns
    want_pkgs[key] = alias_file(f"vendor/_store/{os.path.basename(p)}", ns)
# 3. unversioned aliases -> alias to the versioned package
for a, target in idx["aliases"].items():
    want_pkgs[a] = alias_file(f"vendor/{target}", key_names[target])
for pkg, content in want_pkgs.items():
    d = f"{vendor}/{pkg}"
    if os.path.islink(d):
        os.unlink(d)
    os.makedirs(d, exist_ok=True)
    if write_if_changed(f"{d}/rules.star", content): stats["rules_written"] += 1
    else: stats["rules_unchanged"] += 1
# 4. remove what's no longer wanted
for e in os.listdir(vendor):
    if e != "_store" and e not in want_pkgs:
        p = f"{vendor}/{e}"
        (os.unlink if os.path.islink(p) else shutil.rmtree)(p); stats["pkgs_removed"] += 1
for e in os.listdir(store):
    if e not in want_store:
        os.unlink(f"{store}/{e}"); stats["store_removed"] += 1
print("sync2:", stats)

# Oracle cell -> per-crate rules.star texts and alias map (stand-in for the
# sync-time slice computation)
import json, os, sys
cell, out = sys.argv[1], sys.argv[2]
v = os.path.join(cell, "vendor")
rules, aliases = {}, {}
for e in sorted(os.listdir(v)):
    p = os.path.join(v, e)
    if os.path.islink(p):
        aliases[e] = os.readlink(p)
    else:
        rules[e] = open(os.path.join(p, "rules.star")).read()
json.dump({"rules": rules, "aliases": aliases}, open(out, "w"), indent=0, sort_keys=True)
print(f"{len(rules)} crates, {len(aliases)} aliases")

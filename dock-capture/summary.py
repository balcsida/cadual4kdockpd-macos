#!/usr/bin/env python3
"""10-second bucket summary of sampler.tsv from HH:MM:SS. Usage: summary.py 10:58:50"""
import re, sys
start = sys.argv[1]
rows = [l.rstrip("\n").split("\t") for l in open("sampler.tsv") if l.startswith("2026") and l[11:19] >= start]
def g(r, pat):
    m = re.search(pat, "\t".join(r)); return m[1] if m else "?"
b = {}
for r in rows:
    k = r[0][11:18] + "0"
    cc = [int(g(r, rf'Port-USB-C@{n}\[.*?ConnectionCount=(\d+)')) for n in (1, 2, 3)]
    ori = b.get(k, (0, set()))[1] | {g(r, rf'Port-USB-C@{n}\[ConnectionActive=Yes PlugOrientation=(\d)') + f"@{n}" for n in (1, 2, 3)}
    b[k] = (cc, ori, r[1], r[2], r[3], r[4], g(r, r'ext=(\w+)'))
p = None
for k, v in b.items():
    d = [a - c for a, c in zip(v[0], p)] if p else [0, 0, 0]
    print(k, "conn+ p1/p2/p3:", d, "orient:", sorted(o for o in v[1] if not o.startswith("?")), *v[2:]); p = v[0]
l = rows[-1]
for i in range(4):
    print(f"pc{i}", {k: g(l, rf'pc{i}\[.*?' + k + r'=(-?\d+)') for k in ["IrqCntRxSrcCap", "IrqCntPdStsUpd", "HardResetCount"]})
print("hpm", g(l, r'hpm\[(.*?)\]'))

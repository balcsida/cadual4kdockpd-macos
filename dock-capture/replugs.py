#!/usr/bin/env python3
"""Mac-end replugs on one port: unplug = no plug seen for >=3 s. Usage: replugs.py HH:MM:SS [A|C] [sampler file]"""
import re, sys
start = sys.argv[1]; hal = sys.argv[2] if len(sys.argv) > 2 else "A"
g = lambda s, p: (re.search(p, s) or [0, "0"])[1]
sec = lambda t: int(t[:2]) * 3600 + int(t[3:5]) * 60 + float(t[6:])
rows = [(l[11:23], int(g(l, rf'StatusReg@{hal}=(\w+)'), 16), l.split("\t")[4] != "usbdev=0")
        for l in open(sys.argv[3] if len(sys.argv) > 3 else "sampler.tsv") if l.startswith("2026") and l[11:19] >= start]
runs = []; s = None
for ts, st, usb in rows + [("23:59:59.000", 1, False)]:
    if not st & 1: s = s or ts; e = ts
    else:
        if s and sec(e) - sec(s) >= 3: runs.append((s, e))
        s = None
ok = 0
for k, (a, b) in enumerate(runs):
    nxt = runs[k + 1][0] if k + 1 < len(runs) else rows[-1][0]
    win = [r for r in rows if sec(b) < sec(r[0]) <= min(sec(nxt), sec(b) + 10)]
    if not win: continue
    up = sum(r[2] for r in win); roles = sorted({(r[1] >> 5) & 1 for r in win if r[1] & 1 and (r[1] >> 1) & 7 in (6, 7)})
    good = up > len(win) * 0.6; ok += good
    print(f"out {a[:8]}-{b[:8]} ({sec(b) - sec(a):.0f}s) -> {'CONNECTED' if good else 'FLAPPING '} Mac role bit {roles}")
print("replugs", len(runs), "connected", ok)

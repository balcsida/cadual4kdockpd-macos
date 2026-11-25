#!/usr/bin/env python3
"""Non-flap events since HH:MM:SS for a port: cable-only periods, stable periods, flap runs. Usage: trans.py 11:09:00 2"""
import re, sys
start, port = sys.argv[1], sys.argv[2]
hal = {"1": "C", "2": "A", "3": "8"}[port]
g = lambda s, p: (re.search(p, s) or [0, "?"])[1]
runs = []
for l in open("sampler.tsv"):
    if not l.startswith("2026") or l[11:19] < start: continue
    f = l.split("\t"); ts = l[11:23]
    st = g(l, rf'StatusReg@{hal}=(\w+)'); act = g(l, rf'Port-USB-C@{port}\[ConnectionActive=(\w+)')
    cc = int(g(l, rf'Port-USB-C@{port}\[.*?ConnectionCount=(\d+)'))
    if f[4] != "usbdev=0": s = f"STABLE {f[1]} {f[2]} {f[3]} {f[4]} {g(l, r'ext=(\w+ W=\S+)')}"
    elif st == "0x10000009": s = "CABLE-ONLY (no dock seen)"
    elif st == "0x10000000" and act == "No": s = "NOTHING in port"
    else: s = "flapping/transient"
    if runs and runs[-1][0] == s: runs[-1][2] = ts; runs[-1][4] = cc
    else: runs.append([s, ts, ts, cc, cc])
for s, a, b, c0, c1 in runs:
    print(a, "->", b, s, f"connections +{c1 - c0}")

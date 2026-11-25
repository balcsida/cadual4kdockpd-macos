#!/usr/bin/env python3
"""Print only what changed between consecutive sampler lines. Usage: changes.py [from-marker-substring]"""
import re, sys
start = sys.argv[1] if len(sys.argv) > 1 else None
on = start is None
prev = {}
for line in open(sys.argv[2] if len(sys.argv) > 2 else "sampler.tsv"):
    line = line.rstrip("\n")
    if line.startswith("### MARK"):
        if start and start in line: on, prev = True, prev
        if on: print(line)
        continue
    ts, _, rest = line.partition("\t")
    cur = {}
    for grp in re.finditer(r"(\S+?)\[(.*?)\](?= |\t|$)", rest):
        for kv in re.finditer(r"(\w+)=(\(.*?\)|\".*?\"|\S+)", grp[2]):
            cur[f"{grp[1]}.{kv[1]}"] = kv[2]
    flat = re.sub(r"\S+?\[.*?\](?= |\t|$)", "", rest)
    for f in flat.split("\t"):
        if "=" in f:
            k, _, v = f.strip().partition("=")
            cur[k] = v
    if on:
        d = [f"{k}: {prev.get(k,'?')}->{v}" for k, v in cur.items() if prev and prev.get(k) != v]
        if d: print(ts[11:], " | ".join(d))
    prev = cur

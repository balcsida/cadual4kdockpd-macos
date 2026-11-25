#!/usr/bin/env python3
"""List fresh physical plug-ins (after >=3 s with no dock seen) and their outcome. Usage: attaches.py HH:MM:SS"""
import re, sys
start = sys.argv[1]
g = lambda s, p: (re.search(p, s) or [0, "?"])[1]
sec = lambda t: int(t[:2]) * 3600 + int(t[3:5]) * 60 + float(t[6:])
rows = []
for l in open("sampler.tsv"):
    if not l.startswith("2026") or l[11:19] < start: continue
    f = l.split("\t"); best = None
    for hal, port in (("C", "1"), ("A", "2"), ("8", "3")):
        v = int(g(l, rf'StatusReg@{hal}=(\w+)').replace("?", "0"), 16)
        if v & 1 and (v >> 1) & 7 in (6, 7): best = (port, (v >> 5) & 1)   # dock seen on this port
    rows.append((l[11:23], best, f[4] != "usbdev=0", g(l, r'Port-USB-C@\d\[ConnectionActive=Yes PlugOrientation=(\d)')))
last_seen = None; i = 0; out = []
while i < len(rows):
    ts, best, usb, ori = rows[i]
    if best or usb:
        if last_seen is None or sec(ts) - sec(last_seen) >= 3:
            # outcome: look ahead 8 s
            win = [r for r in rows[i:i + 40] if sec(r[0]) - sec(ts) <= 8]
            ok = sum(r[2] for r in win) > len(win) * 0.6
            roles = {r[1][1] for r in win if r[1]}
            out.append((ts, f"gap {sec(ts) - sec(last_seen):.0f}s" if last_seen else "first", f"port {best[0] if best else '?'}",
                        "CONNECTED" if ok else "FLAPPING", "Mac role bit(s) " + ",".join(map(str, sorted(roles)))))
        last_seen = ts
    i += 1
for o in out: print(*o)
print("total", len(out), "connected", sum(o[3] == "CONNECTED" for o in out))

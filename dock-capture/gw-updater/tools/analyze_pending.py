#!/usr/bin/env python3
"""Evaluate the queued read-only checks. Usage (from gw-updater/): python3 -I tools/analyze_pending.py"""
import glob
S = {}
for f in glob.glob('itec-ti-sums-*.csv'):
    for l in open(f).read().split():
        n, s, _ = l.split(','); S[int(n)] = int(s, 16)
img = open('itec-ti-flash-original.bin', 'rb').read()
ref = lambda a: img[a] if a < len(img) else 0xFF
for a, b, what in ((0x0FF8, 0x1008, 'low AOFF / high RPTR'), (0x1FF8, 0x2050, 'high AOFF / low region header'), (0x16000, 0x16014, 'high region header'),
                   (0x16800, 0x16810, 'high config header + CRC'), (0x168F0, 0x16912, 'high config registers 0x28/0x29')):
    if not all(n in S for n in range(a, b + 1)): print(f"{what}: not measured yet"); continue
    got = bytes((S[n + 1] - S[n]) & 0xFF for n in range(a, b)); bad = [(S[n + 1] - S[n]) & 0xFFFF for n in range(a, b) if (S[n + 1] - S[n]) & 0xFFFF > 255]
    exp = bytes(ref(n) for n in range(a, b)); diff = [hex(a + i) for i in range(b - a) if got[i] != exp[i]]
    print(f"{what} {a:#x}-{b - 1:#x}: {'MATCHES the reconstructed original' if not diff and not bad else 'DIFFERS at ' + str(diff) + str(bad)}")
    if a in (0x1FF8, 0x16000): print('   read:', got.hex(' ')[:200])
blank = [n for n in sorted(S) if n >= 0x30000]
for p, n in zip(blank, blank[1:]):
    w = (S[n] - S[p]) & 0xFFFF; e = (0xFF * (n - p)) & 0xFFFF
    if n <= 0x100000: print(f"blank check {p:#08x}-{n:#08x}: {'blank' if w == e else f'NOT BLANK (sum {w:04X}, blank would be {e:04X})'}")
if 0x200000 in S and 0x100000 in S:
    s1, s2 = S[0x100000], S[0x200000]
    print(f"S(1 MB)={s1:04X} S(2 MB)={s2:04X}:", 'flash wraps at 1 MB (1 MB chip)' if s2 == (2 * s1) & 0xFFFF and s2 != s1 else
          ('second MB blank or same content (2 MB+ chip, blank)' if s2 == s1 else 'second MB holds other data: investigate'))

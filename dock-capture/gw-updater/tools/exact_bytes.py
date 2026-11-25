#!/usr/bin/env python3
"""Compare exact flash bytes derived from a tisession sums CSV with an image: byte[n] = S(n+1) - S(n).
Usage: exact_bytes.py <sums.csv> <image.bin>   - exits 0 if every derivable byte matches."""
import sys
S = {}
for l in open(sys.argv[1]).read().split():
    n, s, _ = l.split(','); S[int(n)] = int(s, 16)
img = open(sys.argv[2], 'rb').read(); bad = []; n_ok = 0
for n in sorted(S):
    if n + 1 not in S: continue
    d = (S[n + 1] - S[n]) & 0xFFFF; want = img[n] if n < len(img) else 0xFF
    if d == want: n_ok += 1
    else: bad.append(f"{n:#07x}: flash {d:#06x} image {want:#04x}")
print(f"{n_ok} bytes match the image, {len(bad)} differ" + ("" if not bad else ":\n  " + "\n  ".join(bad[:40])))
sys.exit(1 if bad or not n_ok else 0)

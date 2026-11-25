#!/usr/bin/env python3
"""Annotated 8051 listing: ann.py file base start end. Adds printf strings (R2:R1 before LCALL printf)."""
import sys, io, re, contextlib
import os; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); import d51
f, base, start, end = sys.argv[1], int(sys.argv[2], 16), int(sys.argv[3], 16), int(sys.argv[4], 16)
PRINTF = int(sys.argv[5], 16) if len(sys.argv) > 5 else 0x5D60
b = open(f, 'rb').read()
buf = io.StringIO()
with contextlib.redirect_stdout(buf): d51.dis(b, base, start, end)
r2 = r1 = None; out = []
for line in buf.getvalue().splitlines():
    a, _, ins = line.partition(': '); ins = ins[9:]
    m = re.match(r'MOV R2,#0x(..)', ins);  r2 = int(m[1], 16) if m else r2
    m = re.match(r'MOV R1,#0x(..)', ins);  r1 = int(m[1], 16) if m else r1
    note = ''
    if ins == f'LCALL 0x{PRINTF:04X}' and r2 is not None and r1 is not None:
        o = (r2 << 8 | r1) - base
        if 0 <= o < len(b): note = '   ; printf ' + repr(b[o:b.index(0, o)].decode('latin1'))[:70]
    out.append(f"{a}: {ins}{note}")
print(' | '.join(out) if '--flat' in sys.argv else '\n'.join(out))

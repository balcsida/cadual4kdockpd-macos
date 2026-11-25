#!/usr/bin/env python3
"""Inspect, check and patch a TPS65982 flash image (Good Way DUD15X0D1 layout).

  ti_image.py check <image>            validate pointers, both regions, both CRCs; print key settings
  ti_image.py diff <a> <b>             list differing bytes
  ti_image.py patch-trysrc <in> <out>  clear TrySRCSupport in the LOW region only and fix its config CRC

Layout (TI SLVUAH7B fig. 3-4, confirmed on the images):
  0x0000 RPTR low, 0x0FFC AOFF low, 0x1000 RPTR high, 0x1FFC AOFF high; region = RPTR + AOFF.
  region+0x000 header: ACE00001, .., header size, app size, app CRC; region+0x800 config block;
  region+0x1000 application.
CRC (both): bit-reversed(NOT zlib.crc32(data)), stored little-endian.
  application CRC covers the application bytes.
  config CRC covers block[0:4] + block[0xC:len], len = u16 at block offset 2 (confirmed on two images).
"""
import struct, sys, zlib

def rev32(x): return int(f"{x:032b}"[::-1], 2)
def ti_crc(data): return rev32((zlib.crc32(data) ^ 0xFFFFFFFF) & 0xFFFFFFFF)
def cfg_crc(block):
    n = struct.unpack_from('<H', block, 2)[0]
    return ti_crc(block[0:4] + block[0xC:n])

def regions(img):
    out = []
    for name, hdr in (('low', 0x0000), ('high', 0x1000)):
        rptr = struct.unpack_from('<I', img, hdr)[0]; aoff = struct.unpack_from('<I', img, hdr + 0xFFC)[0]
        out.append((name, rptr + aoff))
    return out

def records(block):
    """Yield (register, offset_of_data_in_block, data) for each FF FF FF FF | reg | 00 | n | n+1 bytes record."""
    n = struct.unpack_from('<H', block, 2)[0]; i = 0x10
    while True:
        i = block.find(b'\xff\xff\xff\xff', i, n)
        if i < 0: return
        reg, zero, ln = block[i + 4], block[i + 5], block[i + 6]
        if zero != 0: i += 1; continue
        yield reg, i + 7, block[i + 7:i + 7 + ln + 1]
        i += 7 + ln + 1

def check(img, verbose=True):
    ok = True
    for name, base in regions(img):
        magic, _, hsize, asize, acrc = struct.unpack_from('<5I', img, base)
        app = img[base + hsize:base + hsize + asize]
        a_ok = magic == 0xACE00001 and len(app) == asize and ti_crc(app) == acrc
        blk = img[base + 0x800:base + 0x1000]
        c_ok = blk[0x10:0x14] == b'CST1' and cfg_crc(blk) == struct.unpack_from('<I', blk, 4)[0]
        ok &= a_ok and c_ok
        if verbose:
            recs = {r: (o, d) for r, o, d in records(blk)}
            o28, d28 = recs[0x28]; o29, d29 = recs[0x29]
            w1, w2 = struct.unpack_from('<II', d28, 0); c = struct.unpack_from('<I', d29, 0)[0]
            print(f"{name:4} region at {base:#07x}: app {asize:#x} bytes CRC {'ok' if a_ok else 'BAD'}; "
                  f"config CRC {struct.unpack_from('<I', blk, 4)[0]:08X} {'ok' if c_ok else 'BAD'}")
            print(f"     PortInfo={w1 & 7:03b} TrySRCSupport={(w2 >> 10) & 1} (flash {base + 0x800 + o28 + 5:#07x}={d28[5]:#04x}) "
                  f"ExternallyPowered={(c >> 2) & 1} InitiateSwapToSource={(c >> 7) & 1} AutomaticIDRequest={(c >> 24) & 1}")
    if verbose: print(f"length {len(img):#x}, 16-bit byte sum {sum(img) & 0xFFFF:04X}, all checks {'ok' if ok else 'FAILED'}")
    return ok

def patch_trysrc(img):
    img = bytearray(img)
    name, base = regions(img)[0]
    blk = base + 0x800
    for reg, off, data in records(bytes(img[blk:blk + 0x800])):
        if reg == 0x28:
            pos = blk + off + 5
            assert img[pos] & 0x04, 'TrySRCSupport already clear'
            img[pos] &= ~0x04 & 0xFF
            break
    else: raise SystemExit('System Configuration record not found')
    struct.pack_into('<I', img, blk + 4, cfg_crc(bytes(img[blk:blk + 0x800])))
    return bytes(img)

if __name__ == '__main__':
    cmd = sys.argv[1]
    if cmd == 'check':
        sys.exit(0 if check(open(sys.argv[2], 'rb').read()) else 1)
    elif cmd == 'diff':
        a, b = open(sys.argv[2], 'rb').read(), open(sys.argv[3], 'rb').read()
        n = min(len(a), len(b)); d = [i for i in range(n) if a[i] != b[i]]
        print(f"lengths {len(a):#x} / {len(b):#x}; {len(d)} differing bytes")
        for i in d[:64]: print(f"  {i:#07x}: {a[i]:02x} -> {b[i]:02x}")
    elif cmd == 'patch-trysrc':
        src = open(sys.argv[2], 'rb').read()
        assert check(src, verbose=False), 'input image does not validate'
        out = patch_trysrc(src)
        assert check(out, verbose=False), 'patched image does not validate'
        open(sys.argv[3], 'wb').write(out); print('written', sys.argv[3])
    else: raise SystemExit(__doc__)

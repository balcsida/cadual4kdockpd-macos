#!/usr/bin/env python3
"""Model of the Good Way chip's TI program-data handler (E109 firmware, 0x3E21) fed with tiwrite's frame stream.
Usage: sim_chip_write.py <frames.bin> <image.bin> [stale_count]   - exits 0 if the modelled flash equals the image."""
import sys
frames = open(sys.argv[1], 'rb').read(); image = open(sys.argv[2], 'rb').read()
stale = int(sys.argv[3]) if len(sys.argv) > 3 else 0
image += b'\xff' * ((256 - len(image) % 256) % 256)

class Chip:
    def __init__(self, stale):
        self.flash = bytearray(b'\xff' * 0x100000); self.buf = bytearray(0x400)   # buffer 0x00FA.. ; overflow lands in buf[256:]
        self.ptr = stale; self.count = stale; self.mode = 0; self.addr = 0; self.seq = 1; self.acks = 0; self.overflow = False
    def erase(self):                       # handler 0A: chip erase, address 0, expected sequence 1, mode 2 (pointer and count untouched)
        self.flash = bytearray(b'\xff' * len(self.flash)); self.addr = 0; self.seq = 1; self.mode = 2
    def frame(self, f):
        if self.mode != 2: return False
        n = f[1]
        if n > 0x3B or f[63] != 0x5A or (f[2] | f[3] << 8) != self.seq: return False
        for i in range(n):
            if self.ptr >= 256: self.overflow = True
            self.buf[self.ptr] = f[4 + i]; self.ptr += 1
        self.count += n
        if n < 0x3B:                       # page program: flash can only clear bits, address wraps inside the 256-byte page
            self.ptr = 0
            page = self.addr & ~0xFF
            for i in range(self.count):
                a = page | ((self.addr + i) & 0xFF); self.flash[a] &= self.buf[i]
            self.addr += self.count; self.count = 0
        self.seq += 1; self.acks += 1
        return True

c = Chip(stale)
flush = bytes([0x02, 1, 1, 0, 0xFF]) + bytes(58) + bytes([0x5A])
c.erase(); assert c.frame(flush), 'flush frame rejected'; c.erase()        # tiwrite's prelude
fs = [frames[i:i + 64] for i in range(0, len(frames), 64)]
ok = all(c.frame(f) for f in fs)
same = bytes(c.flash[:len(image)]) == image and set(c.flash[len(image):]) == {0xFF}
print(f"stale bytes in buffer before start: {stale}; frames accepted {c.acks - 0}/{len(fs)} (+1 flush); "
      f"buffer overflow: {c.overflow}; final address {c.addr:#x}; flash equals image: {same}")
sys.exit(0 if ok and same and not c.overflow else 1)

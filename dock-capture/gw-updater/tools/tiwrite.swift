// tiwrite: write a TPS65982 image to the dock's TI flash through the Good Way chip (USB 065f:f60f).
//
// DANGER. The chip can only write after a FULL CHIP ERASE (its command 0A sends flash command C7).
// Between the erase and a verified write the flash holds no valid firmware. The TI controller keeps
// running from RAM, so this is harmless as long as the dock's own power stays on. If the dock loses
// power in that window it will not start again without an external flash programmer.
//
// Sequence (the vendor tool's, read from DUD15X0D1_Update_E108.exe and the chip firmware):
//   09 init, 0A erase, poll 0C until idle, data frames, 0D checksum, 0E end.
// Data frame (64 bytes): 02 | n | seq lo | seq hi | 59 data bytes | 5A, n <= 59, seq counts from 1.
//   The chip buffers data and programs the page when a frame with n < 59 arrives, so every 256-byte
//   page goes out as 59+59+59+59+20. It acknowledges each frame with 02 3B seq-lo seq-hi.
// Additions over the vendor sequence, for safety:
//   - refuses an image whose pointers, magic or CRCs do not validate;
//   - checks the present flash content against --expect-current before erasing;
//   - after the first erase sends a 1-byte flush frame (FF) and erases again, so a page left half
//     filled by an earlier aborted attempt cannot corrupt the chip's write address;
//   - verifies the result with a prefix sum at every 4 KB, not only the single total;
//   - waits 5 ms after every page, the flash's worst-case page-program time;
//   - repeats the whole erase/write/verify up to 3 times (with 09 again); never resets or power-cycles anything.
//
// Usage:
//   tiwrite frames    <image> <out>       no device access: write the frame stream, for offline checks
//   tiwrite simulate  <image> --expect-current <hex16|any> --flash <present-content.bin> [faults]
//                                         no device access: the whole write sequence against a model of the chip
//                                         (tools/rehearse.sh runs every scenario)
//   tiwrite preflight <image>             read only: open the flash, report its present checksum, release
//   tiwrite write     <image> --expect-current <hex16|any> --yes-erase-ti-flash
// After a NOT VERIFIED result, `tiwrite preflight <image>` shows what the flash holds now.
import Foundation
import IOKit.hid
setvbuf(stdout, nil, _IOLBF, 0)

// ---------- image handling (no device access) ----------
func rev32(_ x: UInt32) -> UInt32 { var r: UInt32 = 0; for i in 0..<32 where (x >> UInt32(i)) & 1 == 1 { r |= 1 << UInt32(31 - i) }; return r }
func tiCRC(_ d: ArraySlice<UInt8>) -> UInt32 {          // bit-reversed register of a reflected CRC-32, init FFFFFFFF, no final xor
    var c: UInt32 = 0xFFFF_FFFF
    for b in d { c ^= UInt32(b); for _ in 0..<8 { c = (c & 1) == 1 ? (c >> 1) ^ 0xEDB8_8320 : c >> 1 } }
    return rev32(c)
}
func u32(_ d: [UInt8], _ o: Int) -> UInt32 { UInt32(d[o]) | UInt32(d[o + 1]) << 8 | UInt32(d[o + 2]) << 16 | UInt32(d[o + 3]) << 24 }
func validate(_ img: [UInt8]) -> String? {
    guard img.count >= 0x4000, img.count <= 0x40000 else { return "length \(img.count) outside 16 KB..256 KB" }
    for (name, hdr) in [("low", 0x0000), ("high", 0x1000)] {
        let base = Int(u32(img, hdr)) + Int(u32(img, hdr + 0xFFC))
        guard base + 0x1000 <= img.count else { return "\(name) region pointer outside image" }
        guard u32(img, base) == 0xACE0_0001 else { return "\(name) region: bad magic" }
        let hsize = Int(u32(img, base + 8)), asize = Int(u32(img, base + 12))
        guard base + hsize + asize <= img.count else { return "\(name) region: application outside image" }
        guard tiCRC(img[(base + hsize)..<(base + hsize + asize)]) == u32(img, base + 16) else { return "\(name) region: application CRC mismatch" }
        let blk = base + 0x800, n = Int(img[blk + 2]) | Int(img[blk + 3]) << 8
        guard Array(img[(blk + 0x10)..<(blk + 0x14)]) == Array("CST1".utf8), n > 0x10, n <= 0x800 else { return "\(name) region: no CST1 configuration" }
        guard tiCRC(ArraySlice(Array(img[blk..<(blk + 4)]) + Array(img[(blk + 0xC)..<(blk + n)]))) == u32(img, blk + 4) else { return "\(name) region: configuration CRC mismatch" }
    }
    return nil
}
func padded(_ img: [UInt8]) -> [UInt8] { img + [UInt8](repeating: 0xFF, count: (256 - img.count % 256) % 256) }
func dataFrame(_ seq: Int, _ chunk: ArraySlice<UInt8>) -> [UInt8] {
    precondition(chunk.count <= 59 && seq >= 1 && seq < 0xFF00)
    var f = [UInt8](repeating: 0, count: 64)
    f[0] = 0x02; f[1] = UInt8(chunk.count); f[2] = UInt8(seq & 0xFF); f[3] = UInt8(seq >> 8); f[63] = 0x5A
    for (i, b) in chunk.enumerated() { f[4 + i] = b }
    return f
}
func framesFor(_ p: [UInt8]) -> [[UInt8]] {                  // p is a whole number of 256-byte pages
    precondition(p.count % 256 == 0)
    var out = [[UInt8]](); var seq = 1
    for page in stride(from: 0, to: p.count, by: 256) {
        var o = page
        for n in [59, 59, 59, 59, 20] { out.append(dataFrame(seq, p[o..<(o + n)])); seq += 1; o += n }
    }
    return out
}
func sum16(_ d: [UInt8], _ n: Int) -> UInt16 {              // what the chip's 0D returns for length n if the flash holds d then FF
    var s = 0; for i in 0..<min(n, d.count) { s += Int(d[i]) }; if n > d.count { s += 0xFF * (n - d.count) }; return UInt16(s & 0xFFFF)
}

// ---------- offline model of the chip, for `tiwrite simulate` only (same rules as tools/sim_chip_write.py) ----------
final class SimChip {
    var flash: [UInt8], buf = [UInt8](repeating: 0, count: 1024)
    var ptr = 0, count = 0, mode = 0, addr = 0, seq = 1, busy = 0, erases = 0, spi = false, overflow = false
    var dropAck = -1, failErase = 0, skipPage = -1, badByte = -1          // injected faults, each used once
    init(_ content: [UInt8]) { flash = content + [UInt8](repeating: 0xFF, count: 0x200000 - content.count) }
    func reply(_ c: UInt8, _ data: [UInt8]) -> [UInt8] {
        var r = [UInt8](repeating: 0, count: 64); r[0] = 0x02; r[1] = c; r[2] = 0xFE; r[3] = 0xFF; r[4] = c
        for (i, b) in data.enumerated() { r[5 + i] = b }
        return r
    }
    func handle(_ f: [UInt8]) -> [UInt8]? {
        guard f.count == 64, f[0] == 0x02, f[63] == 0x5A else { return nil }
        if f[1] == 0x04, f[2] == 0xFE, f[3] == 0xFF {
            switch f[4] {
            case 0x09: spi = true; return reply(0x09, [0])
            case 0x0A:
                addr = 0; seq = 1; mode = 2                                 // the handler sets these even when the erase fails
                if failErase > 0 || !spi { if failErase > 0 { failErase -= 1 }; spi = false; return reply(0x0A, [1]) }
                erases += 1; flash = [UInt8](repeating: 0xFF, count: flash.count); busy = 3; return reply(0x0A, [0])
            case 0x0C: if busy > 0 { busy -= 1; return reply(0x0C, [0x03, 0x02]) }; return reply(0x0C, [0x00, 0x02])
            case 0x0D:
                let n = Int(f[5]) << 24 | Int(f[6]) << 16 | Int(f[7]) << 8 | Int(f[8]); var s = 0
                for i in 0..<n { s += Int(flash[i]) }
                return reply(0x0D, [UInt8((s >> 8) & 0xFF), UInt8(s & 0xFF)])
            case 0x0E: mode = 0; spi = false; return reply(0x0E, [0])
            default: return nil
            }
        }
        let n = Int(f[1])
        guard mode == 2, n <= 0x3B, Int(f[2]) | Int(f[3]) << 8 == seq else { return nil }
        for i in 0..<n { if ptr >= 256 { overflow = true }; buf[ptr] = f[4 + i]; ptr += 1 }
        count += n
        if n < 0x3B {                                                       // page program: bits can only be cleared, address wraps inside the page
            ptr = 0
            let page = addr & ~0xFF
            if page >> 8 == skipPage, count == 256 { skipPage = -1 } else {    // a full page only: the 1-byte flush frame does not use the fault up
                for i in 0..<count { let x = page | ((addr + i) & 0xFF); flash[x] &= buf[i] }
                if badByte >= page, badByte < page + 256 { flash[badByte] = flash[badByte] == 0 ? 1 : flash[badByte] & (flash[badByte] - 1); badByte = -1 }
            }
            addr += count; count = 0
        }
        seq += 1
        if dropAck == seq - 1 { dropAck = -1; return nil }                  // frame processed, acknowledgement lost
        var r = [UInt8](repeating: 0, count: 64); r[0] = 0x02; r[1] = 0x3B; r[2] = f[2]; r[3] = f[3]
        return r
    }
}

let a = CommandLine.arguments
let usage = "usage: tiwrite frames <image> <out> | tiwrite preflight <image> | tiwrite write <image> --expect-current <hex16|any> --yes-erase-ti-flash\n       tiwrite simulate <image> --expect-current <hex16|any> --flash <present-content.bin> [stale=N] [dropack=SEQ] [failerase=N] [skippage=N] [badbyte=OFFSET]   (no device access)"
guard a.count >= 3, ["frames", "preflight", "write", "simulate"].contains(a[1]), let raw = FileManager.default.contents(atPath: a[2]) else { print(usage); exit(2) }
let image = [UInt8](raw)
if let why = validate(image) { print("image rejected: \(why)"); exit(2) }
let img = padded(image), frames = framesFor(img)
print(String(format: "image %d bytes (padded to %d), %d pages, %d frames, byte sum %04X", image.count, img.count, img.count / 256, frames.count, sum16(img, img.count)))
if a[1] == "frames" {
    guard a.count == 4 else { print("usage: tiwrite frames <image> <out>"); exit(2) }
    FileManager.default.createFile(atPath: a[3], contents: Data(frames.flatMap { $0 })); print("frame stream written to \(a[3])"); exit(0)
}
let preflight = a[1] == "preflight", simulate = a[1] == "simulate"
let argsOK = preflight ? a.count == 3
    : simulate ? (a.count >= 7 && a[3] == "--expect-current" && a[5] == "--flash")
    : (a.count == 6 && a[3] == "--expect-current" && a[5] == "--yes-erase-ti-flash")
guard argsOK, preflight || a[4] == "any" || UInt16(a[4], radix: 16) != nil else { print(usage); exit(2) }
let expectCurrent: UInt16? = preflight || a[4] == "any" ? nil : UInt16(a[4], radix: 16)

// ---------- device access ----------
var last = [UInt8](); var got = false
// an interrupted write is worse than a finished one: also survive a closed terminal (HUP) and a closed output pipe
for s in [SIGINT, SIGTERM, SIGHUP, SIGQUIT, SIGPIPE] { signal(s, SIG_IGN) }
var sim: SimChip? = nil
var dev: IOHIDDevice? = nil
let m = IOHIDManagerCreate(kCFAllocatorDefault, 0)
let buf = UnsafeMutablePointer<UInt8>.allocate(capacity: 64)
if simulate {
    guard let cur = FileManager.default.contents(atPath: a[6]) else { print("cannot read \(a[6])"); exit(2) }
    let s = SimChip([UInt8](cur))
    for o in a.dropFirst(7) {
        let kv = o.split(separator: "=")
        guard kv.count == 2, let v = Int(kv[1]) else { print("bad option \(o)"); exit(2) }
        switch kv[0] {
        case "stale": s.ptr = v; s.count = v
        case "dropack": s.dropAck = v
        case "failerase": s.failErase = v
        case "skippage": s.skipPage = v
        case "badbyte": s.badByte = v
        default: print("bad option \(o)"); exit(2)
        }
    }
    sim = s
} else {
    IOHIDManagerSetDeviceMatching(m, [kIOHIDVendorIDKey: 0x065f, kIOHIDProductIDKey: 0xf60f] as CFDictionary)
    IOHIDManagerOpen(m, 0)
    guard let d = (IOHIDManagerCopyDevices(m) as? Set<IOHIDDevice>)?.first else { print("device not found"); exit(1) }
    IOHIDDeviceRegisterInputReportCallback(d, buf, 64, { _, _, _, _, _, rep, len in
        last = Array(UnsafeBufferPointer(start: rep, count: len)); got = true
    }, nil)
    IOHIDDeviceScheduleWithRunLoop(d, CFRunLoopGetCurrent(), CFRunLoopMode.defaultMode.rawValue)
    dev = d
}
func hex(_ b: ArraySlice<UInt8>) -> String { b.map { String(format: "%02x", $0) }.joined(separator: " ") }
func simReport(_ verdict: String) -> Never {                 // exit 3 = the tool's verdict disagrees with the modelled flash
    let s = sim!
    let same = Array(s.flash[0..<img.count]) == img && !s.flash[img.count...].contains { $0 != 0xFF }
    print("SIMULATION: \(s.erases) erases; flash equals the new image: \(same); buffer overflow: \(s.overflow); tool verdict: \(verdict)")
    exit(verdict == "verified" ? (same && !s.overflow ? 0 : 3) : (same ? 3 : 1))
}
var oddShown = 0                                             // log replies nobody asked for: needed to debug with a blank flash
func exchange(_ f: [UInt8], timeout: Double, accept: ([UInt8]) -> Bool) -> [UInt8]? {
    if let s = sim { if let r = s.handle(f), accept(r) { return r }; return nil }   // offline model: a missing reply returns at once
    got = false
    let rc = IOHIDDeviceSetReport(dev!, kIOHIDReportTypeOutput, 2, f, 64)
    guard rc == 0 else { print(String(format: "send failed (0x%08x) for frame %@", rc, hex(f[0..<6]))); return nil }
    let t0 = Date()
    while Date().timeIntervalSince(t0) < timeout {
        CFRunLoopRunInMode(.defaultMode, 0.005, true)
        if got {
            got = false
            if last.count == 64, accept(last) { return last }
            if oddShown < 40 { oddShown += 1; print("unexpected report (\(last.count) bytes): \(hex(last.prefix(12)))") }
        }
    }
    return nil
}
func cmd(_ c: UInt8, _ args: [UInt8] = [], timeout: Double) -> [UInt8]? {
    precondition([0x09, 0x0A, 0x0C, 0x0D, 0x0E].contains(c))
    var f = [UInt8](repeating: 0, count: 64)
    f[0] = 0x02; f[1] = 0x04; f[2] = 0xFE; f[3] = 0xFF; f[4] = c; f[63] = 0x5A
    for (i, x) in args.enumerated() { f[5 + i] = x }
    let r = exchange(f, timeout: timeout) { $0[1] == c && $0[2] == 0xFE && $0[3] == 0xFF && $0[4] == c }
    if r == nil { print(String(format: "no valid reply to command %02X within %.0f s", c, timeout)) }
    return r
}
func sum(_ n: Int) -> UInt16? {
    guard let r = cmd(0x0D, [UInt8(n >> 24), UInt8((n >> 16) & 0xFF), UInt8((n >> 8) & 0xFF), UInt8(n & 0xFF)], timeout: 240) else { return nil }
    return UInt16(r[5]) << 8 | UInt16(r[6])
}
func sendData(_ f: [UInt8]) -> Bool {                          // duplicates are ignored by the chip, so resending is safe
    for attempt in 1...4 {
        if exchange(f, timeout: attempt == 1 ? 3 : 6, accept: { $0[1] == 0x3B && $0[2] == f[2] && $0[3] == f[3] }) != nil { return true }
        print("no acknowledgement for frame seq \(Int(f[2]) | Int(f[3]) << 8), attempt \(attempt)")
    }
    return false
}
func eraseAndWait() -> Bool {
    guard let r = cmd(0x0A, timeout: 30), r[5] == 0 else { print("erase command not accepted"); return false }
    let t0 = Date(); var told = 0.0                           // the flash is 2 MB or larger: a chip erase can take a minute
    while Date().timeIntervalSince(t0) < 240 {
        usleep(100_000)
        guard let s = cmd(0x0C, timeout: 5) else { continue }
        let t = Date().timeIntervalSince(t0)
        if s[5] & 0x01 == 0 { print(String(format: "erase finished after %.1f s (status %02x %02x)", t, s[5], s[6])); return true }
        if t - told >= 10 { told = t; print(String(format: "  erasing, %.0f s (status %02x %02x)", t, s[5], s[6])) }
    }
    print("flash still busy after 240 s"); return false
}
func finish(_ ok: Bool) -> Never {
    if let r = cmd(0x0E, timeout: 5) { print("0E end: \(hex(r[5..<7]))") } else { print("WARNING: no reply to 0E") }
    if sim != nil { simReport(ok ? "verified" : "not verified") }
    if ok { print("\nWRITE VERIFIED. The new image loads the next time the dock is powered off and on (unplug its power supply for 10 s).") }
    else { print("\n*** NOT VERIFIED. DO NOT POWER-CYCLE THE DOCK. Keep its power supply connected and rerun this tool with --expect-current any. ***") }
    exit(ok ? 0 : 1)
}

guard let r9 = cmd(0x09, timeout: 10), r9[5] == 0 else { print("flash access (09) failed; nothing was changed"); _ = cmd(0x0E, timeout: 5); exit(1) }
guard let now = sum(image.count) else { print("could not read the present checksum; nothing was changed"); _ = cmd(0x0E, timeout: 5); exit(1) }
print(String(format: "present flash content: byte sum of first %d bytes = %04X", image.count, now))
if preflight {
    print(String(format: "image's own sum over the same length: %04X (%@)", sum16(image, image.count), now == sum16(image, image.count) ? "flash already holds this image" : "flash holds something else"))
    if let s = cmd(0x0C, timeout: 5) { print("flash status: \(hex(s[5..<7]))") }
    if let r = cmd(0x0E, timeout: 5) { print("0E end: \(hex(r[5..<7])); nothing was changed") }
    exit(0)
}
if let e = expectCurrent, e != now {
    print(String(format: "expected %04X: content is not what was backed up; nothing was changed", e)); _ = cmd(0x0E, timeout: 5)
    if sim != nil { simReport("aborted before erase") }
    exit(1)
}

for attempt in 1...3 {
    // a failed erase makes the chip release the flash pins (0x331A): enable them again before every retry
    if attempt > 1, cmd(0x09, timeout: 10)?[5] != 0 { print("flash access (09) failed on retry"); continue }
    print("--- attempt \(attempt): erasing")
    guard eraseAndWait(), sum(4096) == 0xF000 else { print("erase did not leave a blank flash"); continue }
    // flush whatever an earlier aborted attempt may have left in the chip's page buffer, then erase again
    guard sendData(dataFrame(1, [0xFF])) else { print("flush frame not acknowledged"); continue }
    usleep(5_000)                                            // let the flush frame's page program end before the erase
    guard eraseAndWait(), sum(4096) == 0xF000, sum(0x30000) == 0x0000 else { print("second erase did not leave a blank flash"); continue }
    print("programming \(frames.count) frames")
    var sent = 0; let t0 = Date()
    for f in frames {
        if !sendData(f) { break }
        sent += 1
        // the chip stops waiting for a page program after 20 status reads (0x58EC) and acknowledges anyway:
        // give the flash its worst-case page time (5 ms) before the next page can start
        if f[1] < 59, sim == nil { usleep(5_000) }
        if sent % 500 == 0 { print(String(format: "  %d/%d frames, %.0f s", sent, frames.count, Date().timeIntervalSince(t0))) }
    }
    guard sent == frames.count else { print("programming stopped at frame \(sent + 1) of \(frames.count)"); continue }
    print(String(format: "programmed in %.0f s; verifying", Date().timeIntervalSince(t0)))
    var firstBad = -1, n = 4096                              // prefix sums first, so a failure says where it is
    while firstBad < 0 && n <= img.count + 8192 { if sum(n) != sum16(img, n) { firstBad = n }; n += 4096 }
    if firstBad < 0 && sum(img.count) == sum16(img, img.count) { finish(true) }
    print(firstBad < 0 ? "verification failed: the total sum differs" : "verification failed in the 4 KB block ending at \(firstBad)")
}
finish(false)

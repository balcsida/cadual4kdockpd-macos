// Read-only session on the TI controller's flash through the Good Way chip (USB 065f:f60f).
// Sends only: 09 (enable flash access, read flash ID), 0C (flash status), 0D (16-bit byte sum of the
// first N bytes), 0E (release flash). Never erase (0A), program (0B), bootloader (03) or reset (1C).
// 0E is always sent before exit, including on Ctrl-C and on errors.
//   tisession probe
//   tisession sums <lengths.txt> <out.csv>      one decimal or 0x length per line
import Foundation
import IOKit.hid
setvbuf(stdout, nil, _IOLBF, 0)
var last = [UInt8](); var got = false; var stop = false
signal(SIGINT) { _ in stop = true }
signal(SIGTERM) { _ in stop = true }
let a = CommandLine.arguments
guard a.count >= 2, ["probe", "sums"].contains(a[1]) else { print("usage: tisession probe | sums <lengths.txt> <out.csv>"); exit(2) }
let m = IOHIDManagerCreate(kCFAllocatorDefault, 0)
IOHIDManagerSetDeviceMatching(m, [kIOHIDVendorIDKey: 0x065f, kIOHIDProductIDKey: 0xf60f] as CFDictionary)
IOHIDManagerOpen(m, 0)
guard let d = (IOHIDManagerCopyDevices(m) as? Set<IOHIDDevice>)?.first else { print("device not found"); exit(1) }
let buf = UnsafeMutablePointer<UInt8>.allocate(capacity: 64)
IOHIDDeviceRegisterInputReportCallback(d, buf, 64, { _, _, _, _, _, rep, len in
    last = Array(UnsafeBufferPointer(start: rep, count: len)); got = true
}, nil)
IOHIDDeviceScheduleWithRunLoop(d, CFRunLoopGetCurrent(), CFRunLoopMode.defaultMode.rawValue)
func hex(_ b: ArraySlice<UInt8>) -> String { b.map { String(format: "%02x", $0) }.joined(separator: " ") }
func xfer(_ cmd: UInt8, _ args: [UInt8] = [], timeout: Double) -> [UInt8]? {
    precondition([0x09, 0x0C, 0x0D, 0x0E].contains(cmd), "command not on the read-only list")
    var f = [UInt8](repeating: 0, count: 64)
    f[0] = 0x02; f[1] = 0x04; f[2] = 0xFE; f[3] = 0xFF; f[4] = cmd; f[63] = 0x5A
    for (i, x) in args.enumerated() { f[5 + i] = x }
    got = false
    guard IOHIDDeviceSetReport(d, kIOHIDReportTypeOutput, 2, f, 64) == 0 else { print("send failed for \(String(cmd, radix: 16))"); return nil }
    let t0 = Date()
    while !got && Date().timeIntervalSince(t0) < timeout { CFRunLoopRunInMode(.defaultMode, 0.01, true) }
    guard got, last.count == 64, last[1] == cmd, last[2] == 0xFE, last[3] == 0xFF, last[4] == cmd else {
        print("no valid reply to \(String(cmd, radix: 16)) within \(timeout)s" + (got ? ": \(hex(last[0..<10]))" : "")); return nil }
    return last
}
func sum(_ n: UInt32) -> (UInt16, Double)? {
    let t0 = Date()
    guard let r = xfer(0x0D, [UInt8(n >> 24), UInt8((n >> 16) & 0xFF), UInt8((n >> 8) & 0xFF), UInt8(n & 0xFF)], timeout: 180) else { return nil }
    return (UInt16(r[5]) << 8 | UInt16(r[6]), Date().timeIntervalSince(t0))
}
func end() { if let r = xfer(0x0E, timeout: 5) { print("0E end: \(hex(r[5..<9]))") } else { print("WARNING: no reply to 0E (release flash)") } }
guard let r9 = xfer(0x09, timeout: 10) else { end(); exit(1) }
print("09 init: \(hex(r9[5..<13]))")
guard r9[5] == 0 else { print("flash not recognised by the chip (result \(r9[5])); the chip releases the flash itself"); end(); exit(1) }
if let rc = xfer(0x0C, timeout: 5) { print("0C status: \(hex(rc[5..<9]))") }
var lengths = [UInt32]()
var out: FileHandle? = nil
if a[1] == "probe" { lengths = [1, 2, 3, 4, 8, 16, 256, 4096, 65536] }
else {
    lengths = try! String(contentsOfFile: a[2]).split(separator: "\n").compactMap { s in s.hasPrefix("0x") ? UInt32(s.dropFirst(2), radix: 16) : UInt32(s) }
    FileManager.default.createFile(atPath: a[3], contents: nil); out = FileHandle(forWritingAtPath: a[3])
}
var done = 0
for n in lengths {
    if stop { print("interrupted"); break }
    guard let (s, t) = sum(n) else { print("aborting at length \(n)"); break }
    let line = String(format: "%u,%04X,%.3f", n, s, t)
    if let o = out { o.write((line + "\n").data(using: .utf8)!) } else { print(line) }
    done += 1
    if out != nil && done % 50 == 0 { print("\(done)/\(lengths.count) last \(line)") }
}
end()
print("done: \(done) of \(lengths.count) sums")

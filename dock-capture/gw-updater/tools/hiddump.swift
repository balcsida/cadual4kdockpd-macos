// Reads code memory through Good Way command 1B (read only): one 57-byte record per index.
// Usage: hiddump <firstIndex> <lastIndex> <outfile>. Sends nothing but command 1B.
import Foundation
import IOKit.hid
var last = [UInt8](); var got = false
let a = CommandLine.arguments
guard a.count == 4, let lo = Int(a[1]), let hi = Int(a[2]), lo >= 0, hi <= 255, lo <= hi else { print("usage: hiddump lo hi outfile"); exit(2) }
let m = IOHIDManagerCreate(kCFAllocatorDefault, 0)
IOHIDManagerSetDeviceMatching(m, [kIOHIDVendorIDKey: 0x065f, kIOHIDProductIDKey: 0xf60f] as CFDictionary)
IOHIDManagerOpen(m, 0)
guard let d = (IOHIDManagerCopyDevices(m) as? Set<IOHIDDevice>)?.first else { print("device not found"); exit(1) }
let buf = UnsafeMutablePointer<UInt8>.allocate(capacity: 64)
IOHIDDeviceRegisterInputReportCallback(d, buf, 64, { _, _, _, _, _, rep, len in
    last = Array(UnsafeBufferPointer(start: rep, count: len)); got = true
}, nil)
IOHIDDeviceScheduleWithRunLoop(d, CFRunLoopGetCurrent(), CFRunLoopMode.defaultMode.rawValue)
var out = Data(); var bad = 0
for idx in lo...hi {
    var ok = false
    for _ in 0..<3 {
        var f = [UInt8](repeating: 0, count: 64)
        f[0] = 0x02; f[1] = 0x04; f[2] = 0xFE; f[3] = 0xFF; f[4] = 0x1B; f[5] = UInt8(idx); f[63] = 0x5A
        got = false
        let rc = IOHIDDeviceSetReport(d, kIOHIDReportTypeOutput, 2, f, 64)
        let t0 = Date()
        while !got && Date().timeIntervalSince(t0) < 1.0 { CFRunLoopRunInMode(.defaultMode, 0.02, true) }
        if rc == 0, got, last.count == 64, last[1] == 0x1B, last[4] == 0x1B, last[5] == UInt8(idx) { ok = true; break }
        usleep(50_000)
    }
    if ok { out.append(contentsOf: last[6..<63]) } else { bad += 1; out.append(contentsOf: [UInt8](repeating: 0xEE, count: 57)); print("index \(idx): no valid reply") }
    usleep(5_000)
}
try! out.write(to: URL(fileURLWithPath: a[3]))
print("indices \(lo)-\(hi): \(out.count) bytes written, \(bad) failed")

// Sends ONE Good Way frame (02 04 FE FF cmd args.. 5A) and prints replies for 3 s.
// Only commands on the read-only allow-list are accepted.
import Foundation
import IOKit.hid
let args = CommandLine.arguments.dropFirst().map { UInt8($0, radix: 16)! }
guard let cmd = args.first, [0x07, 0x08, 0x1B].contains(cmd) else { print("refused: only 07, 08, 1B allowed"); exit(2) }
let m = IOHIDManagerCreate(kCFAllocatorDefault, 0)
IOHIDManagerSetDeviceMatching(m, [kIOHIDVendorIDKey: 0x065f, kIOHIDProductIDKey: 0xf60f] as CFDictionary)
IOHIDManagerOpen(m, 0)
guard let d = (IOHIDManagerCopyDevices(m) as? Set<IOHIDDevice>)?.first else { print("device not found"); exit(1) }
let buf = UnsafeMutablePointer<UInt8>.allocate(capacity: 64)
IOHIDDeviceRegisterInputReportCallback(d, buf, 64, { _, _, _, _, id, rep, len in
    print("reply id=\(id) len=\(len) " + (0..<len).map { String(format: "%02x", rep[$0]) }.joined(separator: " "))
}, nil)
IOHIDDeviceScheduleWithRunLoop(d, CFRunLoopGetCurrent(), CFRunLoopMode.defaultMode.rawValue)
var f = [UInt8](repeating: 0, count: 64)
f[0] = 0x02; f[1] = 0x04; f[2] = 0xFE; f[3] = 0xFF; f[4] = cmd; f[63] = 0x5A
for (i, a) in args.dropFirst().enumerated() { f[5 + i] = a }
print("send " + f.map { String(format: "%02x", $0) }.joined(separator: " "))
let rc = IOHIDDeviceSetReport(d, kIOHIDReportTypeOutput, 2, f, 64)
print("send rc=\(rc)")
CFRunLoopRunInMode(.defaultMode, 3, false)

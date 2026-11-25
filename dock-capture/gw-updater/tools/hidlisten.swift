// Passive listener: opens the HID device non-exclusively and prints input reports. Sends nothing.
import Foundation
import IOKit.hid
let secs = Double(CommandLine.arguments.dropFirst().first ?? "60") ?? 60
let m = IOHIDManagerCreate(kCFAllocatorDefault, IOOptionBits(kIOHIDOptionsTypeNone))
IOHIDManagerSetDeviceMatching(m, [kIOHIDVendorIDKey: 0x065f, kIOHIDProductIDKey: 0xf60f] as CFDictionary)
let rc = IOHIDManagerOpen(m, IOOptionBits(kIOHIDOptionsTypeNone))
let devs = (IOHIDManagerCopyDevices(m) as? Set<IOHIDDevice>) ?? []
print("open rc=\(rc) devices=\(devs.count)")
let buf = UnsafeMutablePointer<UInt8>.allocate(capacity: 64)
var n = 0
for d in devs {
    for k in [kIOHIDManufacturerKey, kIOHIDProductKey, kIOHIDVersionNumberKey, kIOHIDTransportKey] {
        print("  \(k) = \(IOHIDDeviceGetProperty(d, k as CFString) ?? "?" as CFString)")
    }
    IOHIDDeviceRegisterInputReportCallback(d, buf, 64, { _, _, _, _, id, rep, len in
        let t = String(format: "%.3f", Date().timeIntervalSince1970.truncatingRemainder(dividingBy: 100000))
        print("\(t) id=\(id) len=\(len) " + (0..<len).map { String(format: "%02x", rep[$0]) }.joined(separator: " "))
        fflush(stdout)
    }, nil)
    IOHIDDeviceScheduleWithRunLoop(d, CFRunLoopGetCurrent(), CFRunLoopMode.defaultMode.rawValue)
}
CFRunLoopRunInMode(.defaultMode, secs, false)
print("done after \(secs)s")

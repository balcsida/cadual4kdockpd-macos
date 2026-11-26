// Draws the app icon, a USB-C socket on a rounded square, as a 1024x1024 PNG.
// Usage: swift packaging/icon.swift <out.png>
// The icon is drawn at build time so that the repository holds no binary files.
import AppKit

let rep = NSBitmapImageRep(
    bitmapDataPlanes: nil, pixelsWide: 1024, pixelsHigh: 1024, bitsPerSample: 8, samplesPerPixel: 4,
    hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)

// macOS icon grid: the plate is 824 of the 1024 points, with rounded corners
let plate = NSBezierPath(roundedRect: NSRect(x: 100, y: 100, width: 824, height: 824), xRadius: 185, yRadius: 185)
let top = NSColor(calibratedRed: 0.14, green: 0.40, blue: 0.60, alpha: 1)
let bottom = NSColor(calibratedRed: 0.06, green: 0.18, blue: 0.32, alpha: 1)
NSGradient(starting: top, ending: bottom)!.draw(in: plate, angle: -90)

let socket = NSBezierPath(roundedRect: NSRect(x: 252, y: 412, width: 520, height: 200), xRadius: 100, yRadius: 100)
socket.lineWidth = 46
NSColor.white.setStroke()
socket.stroke()

let tongue = NSBezierPath(roundedRect: NSRect(x: 352, y: 490, width: 320, height: 44), xRadius: 22, yRadius: 22)
NSColor.white.setFill()
tongue.fill()

NSGraphicsContext.current = nil
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: CommandLine.arguments[1]))

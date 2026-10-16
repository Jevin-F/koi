import AppKit
import Foundation

let output = URL(fileURLWithPath: CommandLine.arguments.dropFirst().first ?? "packaging/macos/Koi.icns")

func color(_ hex: UInt32) -> CGColor {
    CGColor(
        colorSpace: CGColorSpaceCreateDeviceRGB(),
        components: [
            CGFloat((hex >> 16) & 255) / 255,
            CGFloat((hex >> 8) & 255) / 255,
            CGFloat(hex & 255) / 255,
            1,
        ]
    )!
}

func drawIcon(size: Int) -> Data {
    let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil,
        pixelsWide: size,
        pixelsHigh: size,
        bitsPerSample: 8,
        samplesPerPixel: 4,
        hasAlpha: true,
        isPlanar: false,
        colorSpaceName: .deviceRGB,
        bytesPerRow: 0,
        bitsPerPixel: 0
    )!
    let context = NSGraphicsContext(bitmapImageRep: bitmap)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = context
    let canvas = context.cgContext
    canvas.scaleBy(x: CGFloat(size) / 1024, y: CGFloat(size) / 1024)
    canvas.setAllowsAntialiasing(true)
    canvas.setShouldAntialias(true)

    let tile = CGPath(roundedRect: CGRect(x: 30, y: 30, width: 964, height: 964), cornerWidth: 215, cornerHeight: 215, transform: nil)
    canvas.addPath(tile)
    canvas.setFillColor(color(0x20282c))
    canvas.fillPath()

    func stroke(_ points: [CGPoint], width: CGFloat, ink: UInt32) {
        canvas.setStrokeColor(color(ink))
        canvas.setLineWidth(width)
        canvas.setLineCap(.round)
        canvas.setLineJoin(.round)
        canvas.beginPath()
        canvas.move(to: points[0])
        for point in points.dropFirst() {
            canvas.addLine(to: point)
        }
        canvas.strokePath()
    }

    stroke([CGPoint(x: 345, y: 292), CGPoint(x: 345, y: 732)], width: 86, ink: 0xe6eee9)
    stroke([CGPoint(x: 420, y: 510), CGPoint(x: 680, y: 730)], width: 86, ink: 0x7fceb4)
    stroke([CGPoint(x: 420, y: 510), CGPoint(x: 680, y: 292)], width: 86, ink: 0x7fceb4)

    context.flushGraphics()
    NSGraphicsContext.restoreGraphicsState()
    return bitmap.representation(using: .png, properties: [:])!
}

func appendSize(_ size: Int, to data: inout Data) {
    var value = UInt32(size).bigEndian
    withUnsafeBytes(of: &value) { data.append(contentsOf: $0) }
}

var entries = Data()
for (size, kind) in [
    (16, "icp4"),
    (32, "icp5"),
    (64, "icp6"),
    (128, "ic07"),
    (256, "ic08"),
    (512, "ic09"),
    (1024, "ic10"),
] {
    let png = drawIcon(size: size)
    entries.append(contentsOf: kind.utf8)
    appendSize(png.count + 8, to: &entries)
    entries.append(png)
}
var icon = Data("icns".utf8)
appendSize(entries.count + 8, to: &icon)
icon.append(entries)
try icon.write(to: output)

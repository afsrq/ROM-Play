// Renders an SVG to a square PNG with macOS's own SVG support: swift render.swift in.svg out.png size [inset radius]
// With inset and radius (in 1024ths), the art is shrunk into a rounded square on a transparent
// canvas, the way macOS app icons are drawn.
import AppKit

let args = CommandLine.arguments
guard args.count >= 4, let size = Int(args[3]),
      let image = NSImage(contentsOf: URL(fileURLWithPath: args[1])) else {
    FileHandle.standardError.write("usage: render.swift in.svg out.png size [inset radius]\n".data(using: .utf8)!)
    exit(1)
}
let inset = args.count >= 6 ? (Double(args[4]) ?? 0) : 0
let radius = args.count >= 6 ? (Double(args[5]) ?? 0) : 0

let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size,
                           bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                           colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
rep.size = NSSize(width: size, height: size)
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
NSGraphicsContext.current?.imageInterpolation = .high
let scale = Double(size) / 1024
let box = NSRect(x: inset * scale, y: inset * scale,
                 width: Double(size) - 2 * inset * scale, height: Double(size) - 2 * inset * scale)
if radius > 0 {
    NSBezierPath(roundedRect: box, xRadius: radius * scale, yRadius: radius * scale).addClip()
}
image.draw(in: box, from: .zero, operation: .sourceOver, fraction: 1)
NSGraphicsContext.restoreGraphicsState()
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: args[2]))

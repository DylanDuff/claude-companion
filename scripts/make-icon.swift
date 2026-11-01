// Draws the app icon at 1024×1024: a dark rounded tile holding a frosted
// chat bar with the orange send button, and a small sparkle above it.
//
//   swift scripts/make-icon.swift assets/icon/AppIcon.png
//
// The tile follows Apple's macOS icon grid: an 824pt rounded square centred
// in the 1024pt canvas, leaving room for the drop shadow.

import AppKit

let size: CGFloat = 1024
let output = CommandLine.arguments.dropFirst().first ?? "AppIcon.png"

func color(_ hex: UInt32, _ alpha: CGFloat = 1) -> NSColor {
    NSColor(
        srgbRed: CGFloat((hex >> 16) & 0xff) / 255,
        green: CGFloat((hex >> 8) & 0xff) / 255,
        blue: CGFloat(hex & 0xff) / 255,
        alpha: alpha
    )
}

// Draw into an exact 1024px bitmap (an NSImage would render at the
// display's Retina scale).
let bitmap = NSBitmapImageRep(
    bitmapDataPlanes: nil, pixelsWide: Int(size), pixelsHigh: Int(size),
    bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
    colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
let context = NSGraphicsContext.current!.cgContext

// Tile, with a soft shadow.
let tile = NSRect(x: 100, y: 100, width: 824, height: 824)
let tilePath = NSBezierPath(roundedRect: tile, xRadius: 185, yRadius: 185)
context.saveGState()
context.setShadow(offset: CGSize(width: 0, height: -12), blur: 28, color: color(0x000000, 0.35).cgColor)
color(0x1c1c22).setFill()
tilePath.fill()
context.restoreGState()

// Tile gradient: lifted ink at the top to near-black at the bottom.
context.saveGState()
tilePath.addClip()
NSGradient(colors: [color(0x3a3a48), color(0x15151b)])!.draw(in: tile, angle: -90)
// A faint warm glow behind the bar, echoing the send button.
let glow = NSGradient(colors: [color(0xd97757, 0.28), color(0xd97757, 0)])!
glow.draw(fromCenter: NSPoint(x: 512, y: 430), radius: 0, toCenter: NSPoint(x: 512, y: 430), radius: 420, options: [])
context.restoreGState()

// Hairline highlight along the tile's edge.
color(0xffffff, 0.12).setStroke()
let edge = NSBezierPath(roundedRect: tile.insetBy(dx: 2, dy: 2), xRadius: 183, yRadius: 183)
edge.lineWidth = 4
edge.stroke()

// Chat bar: the compact composer, frosted.
let bar = NSRect(x: 196, y: 330, width: 632, height: 200)
let barPath = NSBezierPath(roundedRect: bar, xRadius: 100, yRadius: 100)
color(0xffffff, 0.14).setFill()
barPath.fill()
color(0xffffff, 0.22).setStroke()
barPath.lineWidth = 4
barPath.stroke()

// Text cursor and a placeholder line inside the bar.
color(0xd97757).setFill()
NSBezierPath(roundedRect: NSRect(x: 268, y: 395, width: 14, height: 70), xRadius: 7, yRadius: 7).fill()
color(0xffffff, 0.35).setFill()
NSBezierPath(roundedRect: NSRect(x: 306, y: 418, width: 230, height: 24), xRadius: 12, yRadius: 12).fill()

// Send button with an up arrow.
let button = NSRect(x: 640, y: 354, width: 152, height: 152)
color(0xd97757).setFill()
NSBezierPath(ovalIn: button).fill()
let arrow = NSBezierPath()
arrow.lineWidth = 18
arrow.lineCapStyle = .round
arrow.lineJoinStyle = .round
arrow.move(to: NSPoint(x: 716, y: 392))
arrow.line(to: NSPoint(x: 716, y: 468))
arrow.move(to: NSPoint(x: 684, y: 438))
arrow.line(to: NSPoint(x: 716, y: 470))
arrow.line(to: NSPoint(x: 748, y: 438))
color(0xffffff).setStroke()
arrow.stroke()

// Sparkle above the bar.
func sparkle(at centre: NSPoint, radius: CGFloat, waist: CGFloat) -> NSBezierPath {
    let path = NSBezierPath()
    for index in 0..<8 {
        let angle = CGFloat(index) * .pi / 4 + .pi / 2
        let r = index % 2 == 0 ? radius : waist
        let point = NSPoint(x: centre.x + cos(angle) * r, y: centre.y + sin(angle) * r)
        index == 0 ? path.move(to: point) : path.line(to: point)
    }
    path.close()
    return path
}
color(0xffffff, 0.92).setFill()
sparkle(at: NSPoint(x: 512, y: 700), radius: 92, waist: 22).fill()
color(0xffffff, 0.55).setFill()
sparkle(at: NSPoint(x: 640, y: 770), radius: 38, waist: 10).fill()

NSGraphicsContext.restoreGraphicsState()

let png = bitmap.representation(using: .png, properties: [:])!
try! png.write(to: URL(fileURLWithPath: output))
print("wrote \(output)")

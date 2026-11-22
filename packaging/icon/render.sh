#!/bin/sh
# Draws every icon ROM Play ships, and the README banner, from icon.svg and banner.svg.
# Needs macOS (it uses its SVG support through Swift). Run it from anywhere: sh packaging/icon/render.sh
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
render() { swift "$here/render.swift" "$@"; }
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# macOS: the art in a rounded square with the standard margin. Others: full bleed or rounded.
render "$here/icon.svg" "$root/crates/romp-app/assets/icon-macos.png" 1024 100 185
render "$here/icon.svg" "$root/crates/romp-app/assets/icon.png" 1024
render "$here/icon.svg" "$root/crates/romp-app/ui/icons/app-icon.png" 512 0 225
render "$here/icon.svg" "$root/packaging/linux/romp.png" 256 12 52

# Windows: an .ico holding a PNG image at each size, which Windows Vista and later read.
for size in 16 24 32 48 64 128 256; do
    render "$here/icon.svg" "$tmp/$size.png" "$size"
done
python3 - "$tmp" "$root/crates/romp-app/assets/icon.ico" <<'PY'
import struct, sys
src, out = sys.argv[1], sys.argv[2]
sizes = [16, 24, 32, 48, 64, 128, 256]
images = [open(f"{src}/{s}.png", "rb").read() for s in sizes]
offset = 6 + 16 * len(sizes)
header = struct.pack("<HHH", 0, 1, len(sizes))
entries = b""
for size, data in zip(sizes, images):
    side = 0 if size == 256 else size
    entries += struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, len(data), offset)
    offset += len(data)
open(out, "wb").write(header + entries + b"".join(images))
PY

# README banner: the background and words, with the macOS icon placed on the left.
swift - "$here/banner.svg" "$root/crates/romp-app/assets/icon-macos.png" "$root/docs/banner.png" <<'SWIFT'
import AppKit
let a = CommandLine.arguments
let bg = NSImage(contentsOf: URL(fileURLWithPath: a[1]))!
let icon = NSImage(contentsOf: URL(fileURLWithPath: a[2]))!
let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 2400, pixelsHigh: 800, bitsPerSample: 8,
                           samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                           bytesPerRow: 0, bitsPerPixel: 0)!
rep.size = NSSize(width: 2400, height: 800)
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
NSGraphicsContext.current?.imageInterpolation = .high
bg.draw(in: NSRect(x: 0, y: 0, width: 2400, height: 800))
icon.draw(in: NSRect(x: 280, y: 90, width: 620, height: 620))
NSGraphicsContext.restoreGraphicsState()
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: a[3]))
SWIFT
echo "Icons and banner drawn."

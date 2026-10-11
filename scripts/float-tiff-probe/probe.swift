// What Apple's readers see of HDR in a file: the content headroom ImageIO and Core
// Image report, and whether ImageIO's own encoder keeps one. Procedure and results:
// README.md beside this file.
//
// Build: swiftc -O probe.swift -o probe   (macOS 26+)
// Run:   ./probe read <file>...
//        ./probe write <out.{tif,exr,heic,...}> <headroom> [thumb]

import Foundation
import ImageIO
import CoreGraphics
import CoreImage
import UniformTypeIdentifiers

func describe(_ img: CGImage, _ label: String) {
    let float = img.bitmapInfo.contains(.floatComponents)
    var line = "  \(label): \(img.width)x\(img.height) bpc=\(img.bitsPerComponent) float=\(float)"
    // `headroom` is what a reader acts on; 0 means unknown, and such an image is
    // excluded from tone mapping (CGImage.h). `calculated` is measured from the pixels.
    line += " headroom=\(img.contentHeadroom) calculated=\(img.calculatedContentHeadroom)"
    if let cs = img.colorSpace {
        line += " extended=\(CGColorSpaceUsesExtendedRange(cs)) pq-or-hlg=\(CGColorSpaceUsesITUR_2100TF(cs))"
    }
    print(line)
}

func read(_ path: String) {
    let url = URL(fileURLWithPath: path)
    print("\n=== \(url.lastPathComponent) ===")
    guard let src = CGImageSourceCreateWithURL(url as CFURL, nil) else {
        print("  FAILED to open"); return
    }
    let props = CGImageSourceCopyPropertiesAtIndex(src, 0, nil) as? [String: Any] ?? [:]
    print("  type=\(CGImageSourceGetType(src) as String? ?? "nil") profile=\(props["ProfileName"] ?? "nil")")
    if let img = CGImageSourceCreateImageAtIndex(src, 0, nil) { describe(img, "imageio") }
    let hdr = [kCGImageSourceDecodeRequest: kCGImageSourceDecodeToHDR] as CFDictionary
    if let img = CGImageSourceCreateImageAtIndex(src, 0, hdr) { describe(img, "imageio-to-hdr") }
    if let ci = CIImage(contentsOf: url, options: [.expandToHDR: true]) {
        print("  coreimage: headroom=\(ci.contentHeadroom)")
    }
}

/// A 512×128 ramp from 0 to nc's peak (1000/203) in extended linear Display P3,
/// tagged with `headroom` in memory and written by ImageIO's encoder for the
/// path's extension. `thumb` asks ImageIO to embed a thumbnail.
func write(_ path: String, _ headroom: Float, thumb: Bool) {
    let w = 512, h = 128
    var px = [Float](repeating: 0, count: w * h * 3)
    for y in 0..<h { for x in 0..<w {
        let v = Float(x) / Float(w - 1) * 4.926108
        let i = (y * w + x) * 3
        px[i] = v; px[i + 1] = v * (y < h / 2 ? 1 : 0.5); px[i + 2] = v * 0.25
    } }
    let data = px.withUnsafeBufferPointer { Data(buffer: $0) } as CFData
    let info = CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue
        | CGBitmapInfo.floatComponents.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)
    guard let img = CGImage(width: w, height: h, bitsPerComponent: 32, bitsPerPixel: 96,
                            bytesPerRow: w * 12,
                            space: CGColorSpace(name: CGColorSpace.extendedLinearDisplayP3)!,
                            bitmapInfo: info, provider: CGDataProvider(data: data)!,
                            decode: nil, shouldInterpolate: false, intent: .defaultIntent),
          let tagged = headroom > 0 ? CGImageCreateCopyWithContentHeadroom(headroom, img) : img
    else { print("FAILED to build the image"); exit(1) }
    describe(tagged, "in-memory")
    guard let type = UTType(filenameExtension: (path as NSString).pathExtension) else {
        print("FAILED: no image type for \(path)'s extension"); exit(2)
    }
    guard let dst = CGImageDestinationCreateWithURL(URL(fileURLWithPath: path) as CFURL,
                                                    type.identifier as CFString, 1, nil)
    else { print("FAILED: cannot create \(path) as \(type.identifier) (does its directory exist?)"); exit(1) }
    let opts = thumb ? [kCGImageDestinationEmbedThumbnail: true] as CFDictionary : nil
    CGImageDestinationAddImage(dst, tagged, opts)
    print(CGImageDestinationFinalize(dst) ? "wrote \(path)" : "FAILED to finalize")
}

let args = CommandLine.arguments
switch args.count > 1 ? args[1] : "" {
case "read" where args.count > 2: args.dropFirst(2).forEach(read)
case "write" where args.count >= 4:
    guard let headroom = Float(args[3]) else { print("headroom must be a number"); exit(2) }
    write(args[2], headroom, thumb: args.count > 4 && args[4] == "thumb")
default:
    print("usage: probe read <file>... | probe write <out> <headroom> [thumb]"); exit(2)
}

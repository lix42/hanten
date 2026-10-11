# Float TIFFs in Apple's readers

Can the HDR linear TIFF (`--range hdr --transfer linear`) carry a signal that makes Apple's
readers show it as HDR, and can either 32-bit float TIFF (that one and the film master)
get a thumbnail in Preview's sidebar? Produced by `output/float-tiff-in-readers`,
2026-10-11, from the failures `analysis/viewer-interoperability`'s rubric found
(`progress/analysis.md`, 2026-10-07).

Measured on macOS 26.6.2 (25G83), SDK 26.5, on a synthetic ramp and on renders of the
committed fixture `tests/fixtures/hdri-64bit.tif`. Headroom: `scripts/float-tiff-probe/`
(procedure there). Thumbnails: a one-off check, described below. Files in
`../temp/float-tiff-probe/`.

## Verdict

- **No.** No TIFF signal makes Apple's readers show float samples as HDR. The HDR linear
  TIFF is an **editor and interchange format**: its luminance mapping is in the report's
  `hdr_linear_tiff` block, and the files that display as HDR on Apple devices are the
  gain-map JPEG and the PQ/HLG TIFFs.
- **No thumbnail fix.** Preview's sidebar shows no thumbnail for any 32-bit float TIFF,
  Apple's own included, and an embedded reduced-resolution image does not change that.
  No encoder change.

## What was measured

**Apple documents headroom as an image property, not a file field.** The public API is
`CGImageCreateCopyWithContentHeadroom` / `CGImage.contentHeadroom` (macOS 15), where `0`
means unknown and "the image with unknown content headroom will be excluded from tone
mapping" (`CGImage.h`). No ImageIO header names a property, TIFF key or XMP field for
a headroom, and Apple's documentation describes none for TIFF.

**Apple's own encoder cannot write one to a TIFF.** A float ramp to 4.926108 in extended
linear Display P3, tagged with headroom 4.926108 in memory, was written by ImageIO:

| container | headroom read back (ImageIO, Core Image) |
|---|---|
| TIFF, 32-bit float | 0, 0. The file is **byte-identical** to the untagged image's |
| OpenEXR | 0, 0 |
| HEIC, PNG, AVIF | 1: ImageIO converts to SDR Display P3 on a plain write |

**What the readers see of nc's files** (the fixture, ImageIO with and without
`kCGImageSourceDecodeToHDR`, and Core Image with `expandToHDR`, all agree):

| file | headroom | calculated from pixels |
|---|---|---|
| HDR linear TIFF (Display P3) | 0 | 3.05 |
| film master | 0 | 2.37 |
| PQ TIFF (BT.2020) | 4.9261084 | 3.05 |

The PQ TIFF's 4.9261084 is `kCGDefaultHDRImageContentHeadroom` (= 1000/203), the default
Apple applies to any PQ or HLG colour space, not a value nc writes. The float files'
samples above 1.0 are decoded intact into an extended-range space; what is missing is
only the headroom, and `calculated` (macOS 26) is not used in its place: the rubric saw
the linear TIFF as SDR.

So in a TIFF, Apple shows HDR only through a PQ or HLG transfer, which the PQ/HLG TIFFs
already are. Design-spec §5's rule stands: the linear TIFF carries no `cicpTag`, since
H.273 describes a bounded code range and these samples exceed 1.0.

**Thumbnails.** ImageIO's TIFF encoder ignores `kCGImageDestinationEmbedThumbnail` (the
file is byte-identical, one IFD). Its thumbnail API
(`CGImageSourceCreateThumbnailAtIndex`, `FromImageIfAbsent` false) ignores an 8-bit
reduced-resolution IFD (`NewSubfileType` 1) appended with `tiffcp` and `tiffset`, and
scales the float main image instead.
Opened together in Preview (user, 2026-10-11), only the 8-bit TIFF and the PQ TIFF got
a sidebar thumbnail. These did not: Apple's own float TIFF, nc's linear TIFF and film
master, and both of nc's with the reduced-resolution IFD appended.

## Not settled

- Non-Apple readers. The rubric gives TIFFs to Apple's readers only; an editor that
  reads the report's reference white is the intended consumer, and none was checked.
- A later macOS. If Apple starts reading a headroom from a TIFF, nothing in CI notices:
  re-run the probe.

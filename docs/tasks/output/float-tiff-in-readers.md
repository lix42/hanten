# Float TIFFs in Apple's readers

## Goal

Decide what the two 32-bit float TIFFs owe a viewer, from
[`analysis/viewer-interoperability`](../analysis/viewer-interoperability.md)'s rubric
(2026-10-07): the HDR linear TIFF (`--range hdr --transfer linear`) and the film master.
Every other destination passed in every reader.

## What was found

- **The HDR linear TIFF does not display as HDR** in macOS Preview, macOS Photos or
  iPhone Photos on an HDR display. The samples above reference white are in the file and
  ImageIO decodes them as float in an extended-range colour space, but it reports the
  image's content headroom as `0` (unknown), against 4.93 for the PQ/HLG TIFFs and the
  gain-map JPEG. This follows from design-spec §5: no ICC profile can state the
  luminance mapping, so the float TIFF carries no HDR signal and only the report says
  `1.0` is 203 cd/m².
- **Preview's sidebar shows no thumbnail** for either float TIFF when several files are
  open. QuickLook (`qlmanage -t`) does thumbnail them, so the gap is Preview's own path.
- The film master not displaying as HDR is expected: it is scene-linear ACEScg, not a
  display rendition.

## Open questions

1. Does TIFF have any HDR signal Apple's readers act on (a metadata tag, an XMP field,
   a profile form) that would not break the "no `cicpTag`, samples exceed 1.0" rule?
   Unknown; needs research and an ImageIO probe, not a guess.
2. If there is none, is the linear TIFF an editor/interchange output only? Then
   `scripts/analysis/viewer.json`'s rendition expectation for it is wrong and changes to
   "record what the reader shows", and `docs/using-nc.md` says where it is meant to go.
3. Would an embedded reduced-resolution image (a TIFF thumbnail IFD) fix Preview's
   sidebar, and is it worth the bytes and the encode-path change? It must not touch the
   pixel IFD the goldens and reports describe.

## Dependencies

- [Viewer interoperability](../analysis/viewer-interoperability.md)

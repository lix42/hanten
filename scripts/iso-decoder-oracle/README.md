# iso-decoder-oracle

The external decoder oracle for
[`iso-gain-map-metadata`](../../docs/tasks/output/iso-gain-map-metadata.md) and
[`gain-map-destination`](../../docs/tasks/nf-destinations/gain-map-destination.md):
a small Swift program that reads nc's gain-map JPEG with **Apple ImageIO**, an
independent implementation of ISO 21496-1.
What it reports about nc's bytes is evidence nc's own reader can never supply —
it found the placement defect fixed on 2026-08-06, which the entire Rust suite
had passed over.

**macOS-only, and deliberately not part of CI.** It needs the system ImageIO
framework and a Swift toolchain, and it is a manual gate run by hand when the
container or the ISO serializer changes. Nothing in `cargo test` invokes it.

Results write-up:
[`docs/progress/output.md`](../../docs/progress/output.md), under
`## iso-gain-map-metadata (decoder oracle — a real defect)`.

## Contents

- `oracle.swift` — the reader. Prints, per file: how many images the container
  holds, whether each gain-map dialect is present, the ISO metadata fields
  ImageIO parsed, and the SDR and HDR decode headrooms.

## Prerequisites

- macOS 15.0 or newer (`kCGImageAuxiliaryDataTypeISOGainMap` was added there;
  the recorded results are from macOS 26.5) and Xcode command-line tools.
- A `hanten` build and a real scan, for the file step 2 writes.

## Usage

Every command below runs from the **repo root**.

```bash
# 1. build the reader
(cd scripts/iso-decoder-oracle && swiftc -O oracle.swift -o oracle)

# 2. write the one gain-map file the build produces
mkdir -p /tmp/iso-oracle
hanten convert <scan> -o /tmp/iso-oracle/gain-map --film-base <r,g,b> --range hdr

# 3. read it back
./scripts/iso-decoder-oracle/oracle /tmp/iso-oracle/gain-map.jpg
```

`--range hdr` writes the gain-map JPEG (`nf-destinations/gain-map-destination`): ISO
21496-1 metadata only, a **three-channel** map, and MPF written by `io::iso_gain_map`.
Measure `<r,g,b>` once per roll the usual way — `hanten measure-base`, or the frozen
`scripts/real-scan-verify/recipes/<roll>.json`. The default render is not flat on a
real frame, so no exposure push is needed; a flat frame reports `GainMapMax = 0` on
every channel and nc's report says `chain.gain_map.flat: true`.

The legacy Ultra HDR v1 and dual-dialect files the earlier results cover can no longer
be written by this build; they are reproducible only from the reference build
(`scripts/reference-snapshot/`).

## Reading the output

The gate wants:

```
  ISO 21496-1 gain map (kCGImageAuxiliaryDataTypeISOGainMap): PRESENT
      description: {…, PixelFormat: 875836518, …}
      meta: HDRToneMap:AlternateHeadroom = 2.300448
      meta: HDRToneMap:ChannelMetadata = [ … GainMapMax = 0.936979 … ]
  Apple/legacy HDR gain map (kCGImageAuxiliaryDataTypeHDRGainMap): ABSENT
  ...
  HDR decode: WxH, headroom 4.926107
```

- **The pass condition is `PRESENT` *plus* a `GainMapMax` materially above 0.**
  `ABSENT` is a failure, and has meant a *placement* problem before rather than a
  serialization one. `PRESENT` with every `GainMapMax ≈ 0` means the metadata parsed
  but the gain map is inert: structurally fine and photographically a no-op, which
  cannot discriminate an HDR rendition from an SDR one — check nc's report
  (`chain.gain_map.flat`) before reading it as a defect.
- **The three `ChannelMetadata` entries differ from each other** — the map is
  per-channel. This is the evidence that the per-channel fields are read, not just
  parsed. Three entries is `is_multichannel = true` read back; that is deliberate and
  must not be "fixed" (C.2.3 lets the metadata channel count differ from the map's).
- The description's `PixelFormat` is `875836518` (`420f`, biplanar YCbCr), and no
  `data:` line prints: ImageIO hands a colour map back as a pixel buffer, not bytes.
- The legacy dialect (`kCGImageAuxiliaryDataTypeHDRGainMap`) is expected **ABSENT**:
  the file carries no Ultra HDR v1 XMP.
- **Do not read the headroom figure as a measurement — it is the trap here.**
  `HDR decode: headroom 4.926107` is just `2^AlternateHeadroom`, i.e. nc's own
  declared `1000/203` policy constant parsed out of the metadata and echoed
  back. It reads the same **even on a completely flat gain map**, so it can
  confirm that ImageIO parsed the headroom field, and nothing more. "Headroom
  1.0 with `PRESENT`" is a state nc's files cannot produce; treating it as the
  failure mode makes the gate unfalsifiable.
- The `meta:` lines are ImageIO's own parse of each ISO field, and *this* is the
  substantive evidence: compare each against what nc wrote (the report's
  `chain.gain_map`, and `exiftool -a -G1` shows the segments).

  | ImageIO prints | nc's `IsoGainMapFields` |
  |---|---|
  | `HDRToneMap:ChannelMetadata[i]` `GainMapMin` / `GainMapMax` | `gain_map_min_log2[i]` / `gain_map_max_log2[i]` |
  | `…[i]` `Gamma` / `BaseOffset` / `AlternateOffset` | `gain_map_gamma[i]` / `base_offset[i]` / `alternate_offset[i]` |
  | `BaseHeadroom` / `AlternateHeadroom` / `BaseColorIsWorkingColor` | `base_hdr_headroom_log2` / `alternate_hdr_headroom_log2` / `use_base_colour_space` |

## Notes

- **Validate the oracle before trusting a negative.** An `ABSENT` result is only
  evidence about nc if a known-good file reads as `PRESENT` on the same machine.
  The control was `ultrahdr_app` v1.4.0 (homebrew) encoding a synthetic
  rgba1010102 gradient — pinned here because homebrew will move off 1.4.0 and
  the exact invocation is what makes the negative trustworthy:

  ```bash
  # <raw> = 256x256 rgba1010102, e.g. a horizontal luminance ramp
  ultrahdr_app -m 0 -p <raw> -w 256 -h 256 -a 5 -t 1 -C 2   # writes out.jpeg
  ./oracle out.jpeg    # must report the ISO gain map PRESENT
  ```

  Its ISO payload is 61 bytes (1 metadata channel) against nc's 141 (3
  channels); both fit `4 + 1 + 16 + 40·channels`, which is independent evidence
  nc's C.2.2 field order is right.
- The compiled `oracle` binary and any generated JPEG are build products; the
  directory's `.gitignore` keeps them out of the repo. It ignores them
  **silently** — a `git add` of a sample or control image here looks like it
  worked and stages nothing, which is the intended outcome. Don't `-f` past it:
  no sample, control, or PDF belongs in the repo.
- exiftool accepts files no decoder parses, so it is not a substitute: it shows the
  segments, not whether a decoder finds the gain map.

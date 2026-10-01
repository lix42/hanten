# Display-Output Acceptance

> **Re-scoped 2026-10-01** for the new chain and **split in three**. The matrix named
> the removed output presets (`gain-map-hdr`, `display-p3`, `compatibility`,
> `hdr-pq`), the Ultra HDR v1 / dual-dialect oracle, the sigmoid / exponential /
> `simple` rows and the `print.*` keys; that version is in git. This task keeps the
> **specification of the gate** and the **run on real scans**:
>
> - [`analysis/display-acceptance-harness`](display-acceptance-harness.md) builds the
>   harness and the decode-back oracles below, on fixtures;
> - [`analysis/viewer-interoperability`](viewer-interoperability.md) owns the manual
>   viewer rubric, Android included.

## Goal

Verify every shipped destination against the user's full-size real scans: the final
product-quality and interoperability gate. `core/release-readiness` sequences
packaging after it.

## Design

Reuse the asset classes and frozen recipes of `real-scan-verification`
(`scripts/real-scan-verify/recipes/`). A small checked-in manifest pins, for every
case: the source asset ID and hash (never the asset), the resolved recipe and its
hash, `pipeline_version`, `working_mapping`, the destination (four axes or
`--film-master`), the rendering, the expected container and signalling, a canonical
pre-encode buffer hash, a golden metadata dump, the independent decoder and its
version, and the numeric tolerances. (As built, the buffer and metadata hashes live in
a per-machine golden, not the checked-in manifest; see "Automated oracles".) A changed golden needs an explicit reviewed
update recording old and new metrics. The case list should be
`nf-verification/benchmark-set`'s, extended rather than duplicated.

For representative colour and HDR frames:

1. **The default as shipped** — `rendering: default`, SDR, `native`, Display P3, 16-bit
   TIFF.
2. **Every ready row of `destination::ROWS`** under `default`: SDR TIFF (Display P3,
   Adobe RGB, sRGB); HDR linear float TIFF (Display P3, Adobe RGB, sRGB, BT.2020);
   PQ and HLG BT.2020 TIFF; the gain-map JPEG (Display P3, sRGB). The
   SDR JPEG rows join when `output/sdr-jpeg-preset` lands.
3. **`direct`** — its unset destination (HDR linear Adobe RGB float TIFF) and its SDR
   form.
4. **Film master** — unclamped linear ACEScg, cross-frame exposure preserved under a
   roll recipe.
5. **Container and profile metadata** — independent inspection confirms the container,
   ICC/CICP signalling, gain-map metadata (three channel entries), reference white and
   headroom; suffixes agree with containers.
6. **Determinism** — repeated runs meet each encoder's documented contract:
   byte-identical where promised, else decoded pixels within the pinned codec bounds
   and identical semantic metadata.
7. **Film-rendering fidelity** — representative stocks, lenses, development processes
   and scanners keep their intended differences through NC film RGB v1 and across the
   encodings. Acceptance compares encodings of the same render, not against a
   physically neutral scene.
8. **Cross-encoding consistency** — matched SDR, HDR, gain-map and film-master outputs
   preserve hue and relative exposure within each destination's declared tone and
   gamut policy; clipping and gamut compression are measured and reported.
9. **Master/display agreement on mid-grey** — fit range holds `f(0.18) = 0.18`; the
   display operator carries the tonal character by design, so no wider tonal match is
   required.
10. **Interoperability** — `analysis/viewer-interoperability`'s rubric passes.

### Automated oracles

`analysis/display-acceptance-harness` built these as `nctool acceptance`
(`scripts/analysis/README.md`); its manifest, `scripts/analysis/acceptance.json`, holds
the bounds. Running on real scans adds an `inputs` entry and a `--write-golden` run on
the acceptance machine: canonical buffers differ by target, so goldens are
per-machine, never committed for CI. Where the harness departed from the list below, the
bullet says so.

All comparisons start from the manifest's canonical buffers and use a decoder
independent of nc:

- `film-master`: decoded float ACEScg matches the canonical buffer with per-sample
  maximum absolute error ≤ 2×10⁻⁶ and RMS ≤ 5×10⁻⁷; metadata matches the golden dump
  exactly.
- 16-bit SDR TIFF: after independent ICC/transfer decode to linear destination RGB,
  each channel differs by at most 1 code value when re-quantized to 16 bits.
- HDR float TIFF: bit-identical to the canonical buffer (the encoder writes it
  verbatim). PQ/HLG TIFF: BT.2100 of the canonical buffer in binary64, quantized to 16
  bits, within 1 code.
- Lossy 8-bit JPEG (the gain-map base): compare the independent decode with the
  canonical encoded base using pinned max/RMS error, structural, neutral-ramp and
  saturated-patch bounds. A universal one-code bound is not valid for JPEG. Record the
  codec version, quality, chroma mode and every threshold in the manifest.
- Gain-map reconstruction: an independent ISO 21496-1 implementation reconstructs the
  canonical HDR rendition and declared headroom within max(0.02 nit, 0.5 % relative)
  per channel, with the three channels read separately. **The harness gates this at the
  map's own grid** instead: the half-resolution map cannot match the rendition pixel
  for pixel (on the fixture, 60 % of pixels miss the bound before any JPEG), so the
  gains decoded from the file's metadata and the pre-JPEG codes must lie within ½ code
  step of the canonical gains resampled to the map grid, and the full-resolution
  error is reported. The oracle converts both
  linear renditions to reference-white-relative units (SDR white `1.0`; HDR absolute
  luminance divided by the pinned 203 cd/m²) and derives each canonical gain as
  `(HDR_c + offset_hdr,c) / (SDR_c + offset_sdr,c)` with the manifest-pinned positive
  offsets. Equal reference-white samples with equal offsets yield gain 1. Black,
  near-black and zero-channel rows stay finite without an arbitrary epsilon;
  negative or non-finite samples and a non-positive adjusted denominator fail loudly.
- Deterministic encoders produce byte-identical files; where an encoder documents
  container variability, decoded pixels meet the bound and a normalized metadata dump
  matches, with only manifest-listed volatile fields allowed to differ.
- Cross-encoding colour comparisons use only manifest-listed patches that are neither
  tone- nor gamut-mapped: independent decodes to XYZ D65 must have ΔE2000 ≤ 0.5 and
  neutral Δu'v' ≤ 0.0001. 8-bit quantization alone reaches ΔE2000 0.52, so the gain
  map's renditions are held to a measured allowance in the manifest instead. Mapped patches compare against their own canonical buffer
  and report hue-angle, clipping and compression deltas.

For the cross-encoding oracle, the manifest pins each rendition's declared
reference-white luminance in nits and the shared source exposure. Decode every patch
to absolute XYZ D65, then divide X, Y and Z by that rendition's reference-white
luminance, so reference white is `Y = 1` with no per-image or per-patch exposure fit.
Convert to CIELAB with the D65 2° tabulated white `(Xn, Yn, Zn) = (0.95047, 1.00000,
1.08883)`, the CIE 1976 piecewise `f(t)` with `δ = 6/29`, and CIEDE2000 per
Sharma–Wu–Dalal (2005) with `kL = kC = kH = 1`. Neutral chromaticity is CIE 1976
`u' = 4X/(X+15Y+3Z)`, `v' = 9Y/(X+15Y+3Z)` on the same normalized XYZ; `Δu'v'` is
Euclidean distance. A zero-denominator sample is an invalid fixture, not a pass.

Any metric outside its bound fails the row. The harness writes a machine-readable
result with measured maxima, RMS and percentile summaries, metadata diffs, decoder
identity, and pass/fail.

## How to Verify

On real scans, the harness passes every applicable numeric and metadata bound,
repeat runs meet each determinism class, and the viewer rubric passes. Results are
recorded with tool and viewer versions in `docs/progress/analysis.md`, and every
failure has a follow-up task (or the log records none).

## Dependencies

- [Output presets and guidance](../output/presets.md) — historical; the presets it
  shipped retired with the flip.
- [Real-scan core verification](real-scan-verification.md)
- [Flip the default to the new flow](../nf-core/default-flip.md)
- [Display-acceptance harness](display-acceptance-harness.md)
- [Viewer interoperability](viewer-interoperability.md)

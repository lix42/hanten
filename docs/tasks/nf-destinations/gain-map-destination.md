# The gain-map destination

## Goal

Ship the new chain's HDR JPEG: an SDR base with a per-channel ISO 21496-1 gain map,
built from `chain::render_pair` and `pipeline::gain_ratio`. In the destination set
([preset-set](preset-set.md)) it is the `--range hdr --container jpeg` row, which is
refused as "not yet" until this lands.

## Design

What is known:

- **Split out of `preset-set` (user, 2026-09-25)** because it needs a container the
  tree does not have. `io::ultra_hdr` packages a *single-channel luminance* map, since
  the legacy Ultra HDR v1 XMP cannot signal a multichannel one, and its ISO fields are
  projected from that map. `gain_ratio` gives **per-channel** gains, and the colour they
  carry where the SDR cube binds is the reason the branch contract keeps them
  (`nf-display-stages/branch-contract`). Collapsing them to luminance to reuse the old
  writer was rejected.
- **No dialect knob**: the legacy XMP dialect cannot carry a per-channel map, and Apple
  platforms read only the ISO one (`output` epic summary). An ISO-only file is the
  destination.
- **The rules `gain_ratio` leaves to its caller**: the HDR rendition is clamped to the
  destination's peak (`1000/203`) and what that clamps is counted into the report, since
  `gain_ratio::between` clamps the alternate only to `>= 0`; a flat map
  (`GainRange::flat`) is reported rather than shipped silently. The gain is ratioed
  against the base as stored, never the unclamped rendition.
- **Verification needs `scripts/iso-decoder-oracle/`** (macOS, manual): exiftool and
  libultrahdr both accept files no decoder parses.
- Needs its own `RunProfile` (the pair holds two working buffers; the copy includes the
  IR plane).

Decided (user, 2026-09-27):

- **nc writes the container itself** — JFIF, ICC, the ISO segments and MPF, with no
  XMP and no libultrahdr. `package()` always writes the legacy XMP, which cannot
  describe an RGB map, so reusing it would ship a misleading dual-dialect file. The
  writer types the gain map `050000` from the start, so `output/mp-container-conformance`'s
  type-code and JFIF-order gaps never arise on this path (that task keeps the legacy
  one).
- **The map is half resolution**, three channels, centre-aligned like
  `gain_map::resample_axis`.
- **A flat map is a report field only** — a fact about the frame, not a warning, so
  `--strict` still passes a frame with no highlights.
- `--range hdr` alone resolves here once the row is ready, by the table's existing
  derivation.
- The ISO field set and serializers in `pipeline/gain_map/iso.rs` are the format's, not
  the old chain's, and are reused; its dead RGB encoder (`encode_iso_gain_map`), which
  takes the legacy `GainMapRender`, is replaced rather than adapted.

Settled by the oracle (2026-09-27): Apple ImageIO reads the ISO gain map from nc's own
container, with the `050000` type code, and parses all three channels' metadata
distinctly — see the progress log.

Not checked: Chrome, and the HDR rendition on an HDR display. Chrome is
`analysis/viewer-interoperability`'s, Android `analysis/android-gain-map-check`'s (from
`output/gain-map-dialect-activation`, closed 2026-10-01).

## How to Verify

- `--new-flow --range hdr --container jpeg` writes a JPEG the ISO oracle reads as
  `PRESENT` with a `GainMapMax` above 0 on a frame with highlights, and reports a flat
  map on one without.
- The HDR rendition's above-peak samples are counted in the report.
- `docs/using-nc.md` updated by running the binary.

## Dependencies

- [The destination set](preset-set.md)

# Gain-Map Dialect Activation

## Goal

Verify the dual-dialect gain-map file on **Android 15+**, the one item left over from
[`iso-gain-map-metadata`](iso-gain-map-metadata.md). Android is the only platform
that reads both ISO 21496-1 and Google's legacy Ultra HDR v1 XMP, so it is the only
place where dual-dialect coexistence is observable end to end, and the only
independent check that adding ISO segments did not disturb the legacy path on its
home platform.

**Rescoped 2026-09-13.** This task originally also owned a CLI path for
`Dialects::LegacyPlusIso`. That shipped as the `gain-map-hdr` preset, the product
default, in `output/presets` (2026-08-09), and the dialect's `#[allow(dead_code)]` is
gone. Only the Android half remains.

## Design

Generate the sample set exactly as `scripts/iso-decoder-oracle/README.md` describes
(a real scan at `NC_ISO_SAMPLE_EV=3.0`; at defaults the gain map is inert and cannot
discriminate anything), then on a physical Android 15+ device or emulator confirm:

- `oracle-dual-dialect.jpg` displays as HDR, and `oracle-legacy-only.jpg` also does.
  The latter is the control proving the ISO segments did not break the legacy dialect.
- Which dialect wins on `oracle-conflicting.jpg`, whose two dialects disagree by
  exactly one stop. Apple selects ISO. **Record whatever Android does as observed
  behaviour, never as a conformance property**: ISO 21496-1 is silent on coexistence,
  and that guard is load-bearing in three documents already.

If the two platforms disagree on precedence, that is a finding worth a design
decision, not a bug to paper over: a dual-dialect file would render differently by
platform whenever the dialects diverge, which argues for keeping both derived from one
model (as today) rather than picking a winner.

Extend `scripts/iso-decoder-oracle/README.md` with the Android procedure. The files are
the same, so no second harness is needed. `analysis/display-output-acceptance`'s
interoperability rubric wants "at least one non-Apple gain-map-aware reader"; this is
that reader, so record the result where both can cite it.

## How to Verify

- On Android 15+: the dual-dialect file displays as HDR; the legacy-only control still
  displays as HDR; the conflicting file's selected dialect is recorded as observed
  behaviour with the platform and OS version noted.
- The Apple oracle still passes (`PRESENT` plus a `GainMapMax` above 0, never the
  headroom figure, which is nc's own declared constant echoed back).

## Dependencies

- [Final ISO gain-map metadata](iso-gain-map-metadata.md)

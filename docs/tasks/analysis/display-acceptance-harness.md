# Display-acceptance harness

## Goal

Build the manifest-driven harness and the independent decode-back oracles that
[`analysis/display-output-acceptance`](display-output-acceptance.md) specifies, and
prove them on committed fixtures, so the real-scan run is only a matter of pointing
the harness at the assets. Split out of that task on 2026-10-01 because this half
needs no real scans and no human eyes.

## Design

- **One manifest format** for the acceptance cases: source hash, recipe hash,
  `pipeline_version`, destination, rendering, expected container and signalling,
  canonical pre-encode buffer hash, golden metadata dump, decoder identity and
  tolerances. It reuses `nf-verification/benchmark-set`'s case list (one case per
  destination) rather than keeping a second one.
- **The oracles** for every encoding acceptance names: film master, 16-bit SDR TIFF,
  HDR float and PQ/HLG TIFF, and the ISO 21496-1 gain-map JPEG (AVIF was removed,
  `output/drop-avif`).
  Each decodes with a decoder independent of nc and compares against the canonical
  buffer.
- **Where it lives**: `nctool` (`scripts/analysis/`), beside `metrics` and `compare`,
  with its own tests.

## Open questions

- Which independent decoders, and how CI gets them: an ICC/transfer decoder for TIFF,
  an ISO 21496-1 gain-map decoder (libultrahdr
  reads ISO; Apple ImageIO is macOS-only and stays the manual oracle).
- How the canonical pre-encode buffer is exported from nc: a debug export or an
  existing report field.
- The bounds acceptance leaves to this task (HDR float and PQ/HLG TIFF), from each
  encoder's documented contract.

## How to Verify

- On the fixtures, every oracle passes for a correct file and fails for a
  deliberately corrupted one (wrong ICC, a moved APP segment, a swapped channel, a
  shifted code value).
- The harness's machine-readable result carries measured maxima, RMS summaries,
  metadata diffs, decoder identity and pass/fail.
- The `nctool` suite stays green.

## Dependencies

- [Flip the default to the new flow](../nf-core/default-flip.md)
- [A benchmark set for the new flow](../nf-verification/benchmark-set.md) — the case
  list.

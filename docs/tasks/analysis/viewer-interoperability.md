# Viewer interoperability

## Goal

Check by hand that real viewers open and display each shipped destination as
intended: the manual half of
[`analysis/display-output-acceptance`](display-output-acceptance.md), split out on
2026-10-01. It also absorbs the Android check of `output/gain-map-dialect-activation`
(closed into this task the same day).

## Design

For each pinned viewer, OS and display-HDR setting, record these binary observations
per file:

1. it opens without repair or error;
2. the viewer reports or visibly selects the intended SDR or HDR rendition;
3. turning HDR off, or an SDR-only reader, falls back as the destination specifies;
4. orientation, dimensions, crop and extra-channel handling are correct;
5. no gross channel swap, inversion, all-black or all-white render, or edge artifact.

Readers to cover:

- macOS and iPhone (Apple ImageIO, Preview / Photos);
- **Android 15+**: does it display nc's ISO-only, **three-channel** gain-map JPEG as
  HDR? Android's platform decoder is the non-Apple ISO reader, and the three-channel
  map is the risk. Record the device and OS version;
- one more non-Apple gain-map reader (e.g. Chrome);
- one SDR-only reader, for every gain-map and HDR destination.

"Plausible", "looks good" and agreement with a remembered scene are not pass
criteria. A failure becomes its own follow-up task, not an inline fix.

## Open questions

- Which file set: the real-scan acceptance outputs, or a smaller fixed set that can be
  re-checked after each container change.
- Whether a libultrahdr decode on Linux (`ultrahdr_app`) is worth adding as a cheap
  proxy for Android's parser before the device check.

## How to Verify

Every rubric item is recorded in `docs/progress/analysis.md` with the viewer, OS and
display setting and the evidence, for every reader above, and every failure has a
follow-up task.

## Dependencies

- [Flip the default to the new flow](../nf-core/default-flip.md)

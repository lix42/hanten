# Content-light levels per CTA-861.3

## Goal

Measure an HDR rendition's MaxCLL and MaxFALL as CTA-861.3 defines them — from each
pixel's largest linear R, G or B component — not from its luminance. Found by
`analysis/display-acceptance-harness`: on the scan fixture nc reported MaxCLL 391 /
MaxFALL 51 cd/m² where the standard's definition gives 578 / 68, so colourful
highlights read too dim.

## Design

- `hdr::measure_content_light` took each pixel's luminance in the rendition's own
  gamut. Its consumers are the HDR TIFF reports' `max_cll_nits` / `max_fall_nits`
  (`hdr_linear_tiff`, `hdr_coded_tiff`) and `hdr::sdr_range_warning` (the "HDR wrapper
  around an SDR-range signal" warning). The AVIF `clli` box that first exposed it is
  gone (`output/drop-avif`).
- No rendered pixel depends on it, so no `pipeline_version` change; the report numbers
  and the warning's trigger move.

## Decisions (2026-10-01)

- **The warning keeps MaxCLL as its trigger.** An SDR container holds each channel in
  `[0, 1]`, so a saturated channel above reference white is content it cannot carry,
  whatever the pixel's luminance. This also resolves `output/sdr-preset-followups`'
  "misfires on saturated colour" finding.
- **Field names kept.** They are the standard's names and now carry its values; the
  report has no schema version a rename could signal through.
- **Pinned** by `hdr.rs` unit tests (a saturated pixel) and by `nctool acceptance`'s
  `content_light` check on every HDR TIFF case; stated in design-spec §5 and
  `using-nc.md` §8.

## How to Verify

- A saturated test pixel (one channel well above the others) reports MaxCLL from that
  channel, not its luminance.
- The scan fixture's reported MaxCLL / MaxFALL match an independent max(R, G, B)
  computation on the HDR rendition (`--export-pre-encode`'s `hdr-linear` page × 203).
- `docs/using-nc.md` and the design spec state the definition; the warning's trigger
  (MaxCLL) is documented.

## Dependencies

- [Display-HDR rendering](hdr-display-rendering.md) — the measurement this corrects.

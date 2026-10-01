# Content-light levels per CTA-861.3

## Goal

Measure an HDR rendition's MaxCLL and MaxFALL as CTA-861.3 defines them — from each
pixel's largest linear R, G or B component — not from its luminance. Found by
`analysis/display-acceptance-harness`: on the scan fixture nc reports MaxCLL 391 /
MaxFALL 51 cd/m² where the standard's definition gives 578 / 68, so colourful
highlights read too dim.

## Design

- `hdr::measure_content_light` takes each pixel's luminance in the rendition's own
  gamut (`gamut.luma()`), so the P3, Adobe RGB and sRGB linear TIFFs each use their own
  luma vector. Its consumers are
  the HDR TIFF reports' `max_cll_nits` / `max_fall_nits` (`hdr_linear_tiff`,
  `hdr_coded_tiff`) and `hdr::sdr_range_warning` (the "HDR wrapper around an SDR-range
  signal" warning). The AVIF `clli` box that first exposed it is gone
  (`output/drop-avif`).
- No rendered pixel depends on it, so no `pipeline_version` change is expected; the
  report numbers and the warning's trigger do move.

## Open questions

- Does the SDR-range warning keep MaxCLL as its trigger, or does it want luminance
  (a saturated highlight above 203 cd/m² in one channel is not "brighter than
  reference white" in the usual sense)? If the latter, it needs its own measure and
  the report field changes meaning alone.
- Field names: keep `max_cll_nits` / `max_fall_nits` with corrected values, or rename
  so a report from an older build cannot be read as the new definition.
- Where the definition is pinned: design-spec and `using-nc.md` should state it, and
  a test should hold it with a saturated pixel.

## How to Verify

- A saturated test pixel (one channel well above the others) reports MaxCLL from that
  channel, not its luminance.
- The scan fixture's reported MaxCLL / MaxFALL match an independent max(R, G, B)
  computation on the HDR rendition (`--export-pre-encode`'s `hdr-linear` page × 203).
- `docs/using-nc.md` and the design spec state the definition; the warning's behaviour
  is whatever the open question decides, and documented.

## Dependencies

- [Display-HDR rendering](hdr-display-rendering.md) — the measurement this corrects.

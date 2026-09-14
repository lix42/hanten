# Adobe RGB (1998) as an output gamut

## Goal

Give Adobe RGB (1998) a first-class name on the SDR display path. nc supports sRGB,
Display P3, ProPhoto and ACEScg; Adobe RGB is the one notable omission for a
photography tool. It is usable today only via `--output-profile <path-to-icc>` on the
legacy path.

## What is known

- Split out of `output/sdr-preset-followups` on 2026-09-13.
- **The colorimetry definition already exists**: `definitions::ADOBE_RGB` was added
  for the analysis tooling (`nctool metrics --space adobe-rgb`) and is deliberately
  unused by the runtime. So the provenance half is done; what remains is the render
  half.
- Not a one-line addition: the modern SDR renderer does not merely *tag* a profile, it
  **gamut-maps** into the destination (`neutral-axis-radial-boundary-v1`). A new gamut
  needs pinned artifacts in `pinned.rs`, an `SdrGamut` arm, an ICC profile built from
  the definition, gamut-mapping coverage, and a preset name (or a selector on the
  existing SDR presets). `pipeline/colorimetry/` is the area CLAUDE.md guards most
  carefully; follow `docs/colorimetry-maintenance.md`.
- ProPhoto and ACEScg deliberately stay off the *display* path: they are editing
  spaces, which is what `film-master` and `hdr-linear-tiff` are for.

## Open questions

- A new output preset, or a gamut selector on `display-p3`/`compatibility`? A preset
  keeps the atomic-preset rule simple; a selector avoids a thirteenth name.
- `RunProfile`: the SDR TIFF profile is shared by the two SDR presets today and should
  cover a third identical-shape one, but confirm rather than inherit.

## How to Verify

- The pinned artifacts are audited against the definition; a render into Adobe RGB is
  gamut-mapped, not merely tagged, and covered by a test.
- `nctool metrics` reads the output with `--space adobe-rgb` and agrees with the
  declared space.

## Dependencies

- [Output presets and guidance](presets.md)

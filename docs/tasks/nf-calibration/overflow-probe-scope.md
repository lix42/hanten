# A steep frame refused by a sample no picture holds

## Goal

A slope a real frame can render must not be refused because of a zero sample no picture
holds — and one frame's entry must not refuse its neighbours.

## Design

Known:

- `recipe::validate_render` runs the densest sample a scan can hold — a zero sample, read
  at the scan floor (1e-6, ~6 density above the base, ~30 stops above mid-grey) — through
  the decode and the look, and refuses (exit 2) if it overflows f32. It checks every
  `roll.frames` entry, so one frame's entry refuses the whole roll.
- Under `--placement frame --contrast 1.3` (2026-10-12, `nf-display-stages/midtone-contrast-toe`
  step 4, `../temp/midtone-contrast-toe-placement/`): 11 frames are refused on their own
  entry — 09-18: 1789, 1799, 1813, 1820; 09-29: 2038, 2040, 2045, 2047, 2048, 2051, 2053 —
  and with the measured whole-roll recipe both rolls refuse every frame. On 1789 alone the
  limit lies between slope 4.33 (×1.25, renders) and 4.50 (×1.3, refused). The binary
  before the toe refuses the same.
- `nf-calibration/short-span-protection` holds the maximum slope; it records that
  uncapped, 1612 and 1719 overflow the same probe. Related, not a dependency.

Open:

- Clamp the probe pixel to the densest value a real scan reaches.
- Check only the entry being rendered — in `roll`, each frame as it is reached — or warn up
  front instead of refusing.
- Cap the final slope (placement × contrast): that changes rendering and needs a review.

## How to Verify

- `--placement frame --contrast 1.3` renders the reviewed rolls (09-18, 09-29).
- A genuinely overflowing value is still refused before anything is decoded.

## Dependencies

None.

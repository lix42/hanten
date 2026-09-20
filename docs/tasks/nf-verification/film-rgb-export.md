# Export the pre-matrix film RGB

## Goal

Let nc write the reconstruction's output *before* the NC film RGB v1 3×3. That is
the cleanest point at which to measure a decode, and nc cannot export it today.

## Design

- **Why before the matrix.** `film-master` is reconstruction → 3×3 → unclamped
  f32 ACEScg, so it is already the artifact a reconstruction is judged on — but
  the 3×3 treats the dye-layer channels as Rec.709, which leaves neutrality
  checks intact and mixes per-layer slope measurements. A per-channel `scale`
  calibration is exactly a per-layer slope question, so it wants the unmixed
  values.
- **This is a new destination for an existing typed boundary**, not a new
  reconstruction path: `algo::FilmRgbImage` is mintable only by `algo`, and the
  export consumes one where `working_space::map_nc_film_rgb_v1` would.
- **Part 1 asks whether the 3×3 belongs in scene correction instead.** If it
  moves, `film-master` *becomes* film RGB and this export collapses into it. So
  settle the export's shape without assuming the matrix stays where it is — and
  if the matrix moves first, check whether this task still has content.
- **Declaring a colour space would be a lie.** Film RGB is not colorimetric;
  tagging the file as ACEScg or Rec.709 would make every downstream tool wrong in
  a way that looks right. An untagged f32 TIFF that `nctool metrics` reads by
  convention is the honest option; decide explicitly rather than by default.
- `--export-ir` is the precedent for an operational side output with a recipe key
  under its stage section.

## How to Verify

- One frame exported both ways agrees with `film-master` through the pinned
  matrix to f32 round-trip.
- `nctool metrics` reads the exported file and its per-channel numbers differ
  from the `film-master` ones in the direction the matrix predicts — i.e. the
  export is measurably *not* just a renamed master.

## Dependencies

- [The fixed decode](../nf-reconstruction/fixed-decode.md)

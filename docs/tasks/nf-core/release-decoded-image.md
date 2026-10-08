# Release the decoded image after the decode

## Goal

Free the decoded input image once `fixed::decode` has read it, so a frame's render
and encode no longer hold it alongside the chain's buffer. That lowers the modelled
and measured peak per frame by the decoded image's size (12 B/px for RGB, 16 B/px
with an IR plane) on every destination.

## Design

- **Nothing reads the decoded image after the decode now.** `--export-ir`, which
  wrote from it at encode time, retired with `nf-verification/roll-side-exports`.
  `cli::render_frame` still owns it to the end of the frame only because the binding
  lives that long.
- **`fixed::decode` clones the IR plane** onto the film RGB image. Moving it instead
  is the same saving for the plane, but `decode` takes the image by reference; decide
  whether to change that signature or only drop the image after the call.
- **`pipeline/memory.rs` moves in the same change.** Its render and encode phases
  count the decoded image ("held to the frame's end"); the model has to drop that
  term, and a new calibration replaces the measured rows it falsifies. Nothing tests
  the model against the code; left alone it over-estimates and refuses frames that
  now fit.
- **No pixel moves.** Where a buffer lives is not an image change; output stays
  byte-identical and the drift gate does not trip.
- Overlaps [the stage seams task](buffer-strategy.md), which owns where the IR plane
  travels; keep this to releasing the decoded image and leave the plane's route to it.

## Open questions

- Does dropping the image early actually return the memory to the OS on both
  platforms (`allocator.rs` maps big blocks), so measured peak RSS falls by the full
  term? **Linux: yes** — 16 B/px, measured 2026-10-08. **macOS: not yet measured.**
- Is the IR-plane clone worth removing here, or left to `nf-core/buffer-strategy`?
  **Removed here (2026-10-08).** Releasing the scan needed the decode to work in place,
  which moves the plane instead of cloning it; its route through the chain is unchanged.

## How to Verify

- Peak RSS on a real 5000 dpi HDRi frame drops by about the decoded image's size,
  for a u16 TIFF and a gain-map JPEG destination; the memory model's estimate stays
  slightly under measured, inside its allowance.
- Outputs are byte-identical before and after.
- The CI gates pass.

## Dependencies

- [Per-frame side exports from `roll`](../nf-verification/roll-side-exports.md)

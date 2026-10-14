# Label the charts in a frame

## Goal

A tool to mark where each colour target sits in a frame, check that the mark is right,
and write it into `manifest.json` as the frame's chart entries. Charts will appear in
bracket frames and, from now on, in ordinary pictures, so labelling is routine work, not a
one-off.

## Known

- Agreed with the user 2026-10-14 (`calibration-frame-capture`'s "Registration"): one entry
  per target, many per frame — the target, the passport it belongs to, and four corners in
  the target's own orientation (top-left first, clockwise). Each target gets its own
  corners: a passport's pages are not coplanar and the card is not parallel to the lens.
- A separate tool, not a mode of `tools/review-app`: the job is placing and checking a
  projected grid, not comparing renders.
- It overlays the patch grid projected from the corners (perspective), and checks the
  placement — each patch's sampled area uniform, the gaps on the patch borders, the neutral
  series monotonic — storing the result with the label.
- A bracket shot on a tripod starts each frame from the previous frame's corners.
- It labels a render, never the scan itself (renders keep the scan's dimensions, so
  fractions carry over); the agent never reads the pixels.
- First targets: the ColorChecker Passport Photo 2's Classic (24) and Creative Enhancement
  (28, irregular middle rows) — patch geometry as per-patch rectangles in target
  coordinates, committed once with the reference values.

## Open

- Stack and whether it shares `tools/review-app`'s build.
- How the import keeps hand edits and refuses a label whose check failed.
- Where the target geometry and reference values live, and their sources (X-Rite's
  post-2014 Classic values; the Creative target has none published).

## How to Verify

- 2127–2133 of `2026-10-12-Gold200` labelled, both targets, every check passing, and the
  entries in `manifest.json` (`manifest validate` clean).
- A deliberately misplaced corner fails the check.

## Dependencies

- [Asset manifest](asset-manifest.md) — the inventory the chart entries are written into

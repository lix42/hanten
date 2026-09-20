# Goldens for the new stages

## Goal

Give each new stage its own curated per-pixel golden vector, so a stage boundary
cannot move silently. `stages::golden` is the crate's cross-platform bit-identity
substrate today; the new chain needs the same substrate built for it.

## Design

- **Small, curated, decode-independent.** The existing vectors are a handful of
  per-pixel values captured from the reference code and fed straight into the
  stage — no file, no decode, no assets. That is what makes them runnable
  anywhere and readable when they fail.
- **The standing rule is a hard constraint, not advice:** never checksum a full
  frame, an encoded file, or post-colour-transform pixels in a cross-platform
  gate. Reconstruction's transcendental FP differs ~1 ULP across libm
  implementations, and the lcms2 transform plus the embedded ICC bytes differ by
  target outright. Anything past `color::to_output`'s successor is verified by
  same-machine before/after, not by a committed vector.
- **Where a value cannot be pinned bit-exactly, bound it by enumeration** —
  `golden::reachable_window`, which renders every intermediate a 1-ULP-accurate
  libm can return and takes the widest excursion. Two attempts to derive a "safe"
  threshold from a published error bound were both unsound; don't try a third.
  Cover every libm call in the chain, not the last one.
- **Give the new stages their own vectors rather than re-pointing
  `golden::pixels()`.** It is shared with every historical fingerprint row, so
  moving it shifts what those rows mean.
- Open: whether one vector threaded through the whole chain is more useful than a
  vector per stage. A per-stage vector localizes a failure; a threaded one is the
  only thing that catches a stage-ordering mistake.

## How to Verify

- Each new stage has a golden that fails when its arithmetic is perturbed, and
  the failure names the stage.
- The full set passes on macOS/aarch64 and x86_64 Linux, with every windowed
  sample's window derived rather than hand-picked.

## Dependencies

- [A minimal end-to-end render](../nf-core/minimal-end-to-end.md)

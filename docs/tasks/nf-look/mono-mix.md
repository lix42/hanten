# Mono as a rendering

## Goal

Render a dye negative mono: a colour negative, or a chromogenic B&W negative scanned
in RGB. Pool the three channels with user weights as the first step of the look, after
scene correction, like NLP's "Color Model: B+W". The decision behind this task is
[`docs/design/mono.md`](../../design/mono.md). A silver negative does not take this
route: it goes through [`mono-input`](../nf-reconstruction/mono-input.md).

## Design

What is decided (`mono.md`):

- **First in the look, after scene correction.** White balance acts before pooling,
  so it still changes the grey tones even though it can no longer tint. It is not the
  same thing as the weights. Don't fold a stated white balance into the weights.
- **One enum knob**: colour, or mono with weights. Never a bool beside a weight array.
  `algo/bw-support`'s value rules carry over: normalise the weights, and refuse
  non-finite or negative weights or a sum that is not positive.
- **The grey value is computed once per pixel** and copied into all three channels,
  so R = G = B holds exactly when this stage writes it.
- **The rest of the look is neutral-preserving** on the result, except
  `channel_grade`, which would tint it. Whether to refuse `channel_grade` beside mono
  or keep it as toning is open; toning itself is out of scope.

Open:

- The default weights (luminance or equal thirds), and whether the chromogenic B&W
  case wants a different default. No chromogenic scan exists to judge by.
- The recipe key and flag spelling.
- The report echoes the resolved model and the normalised weights (`bw-support`
  required it); where, and in what shape.

## How to Verify

- With the stage on, every pixel leaves it with R = G = B exactly. Measured on the
  output, R = G = B holds within the destination transform's tolerance.
- With the stage off (the default), the render is bit-exact with today's. No
  fingerprint moves.
- Weight normalisation, refusal of bad weights, and the merge arm are tested through
  `merge`.
- `docs/using-nc.md` documents the knob.

## Dependencies

- [Where black-and-white fits the new chain](../nf-reconstruction/mono-decode.md)

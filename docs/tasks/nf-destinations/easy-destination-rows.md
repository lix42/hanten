# Easy destination rows

## Goal

Add the destination-table rows a survey of lossless × HDR × gamut (2026-09-27, with
the user) classed as easy: each a row in `destination::ROWS` plus a small
generalization, no new container or colorimetry research. Kept so that anyone who
wants one of these outputs gets it, not because a workflow depends on it.

## Design

The rows, in the order their prerequisites allow:

1. **Linear 32-bit float HDR TIFF in Adobe RGB and Display P3** (`--range hdr
   --transfer linear --gamut adobe-rgb|display-p3`). Lossless HDR for an editor, in
   the gamut the user works in. Fit gamut already maps into either gamut at any peak;
   the work is generalizing the linear HDR hand-off off BT.2020, where it is baked in
   today: `hdr::LinearBt2020Hdr`, the `from_new_chain` caller's BT.2020-only check in
   `cli`, the report's pixel-contract and linear-domain strings, the content-light
   luma, and the ICC builder (`color::hdr_linear_bt2020_icc`). The existing BT.2020
   row's bytes and report must not move.
2. **sRGB as a new-chain gamut** (SDR 16-bit TIFF, `--gamut srgb`) — the counterpart
   of the current chain's `compatibility` preset, which `flow.rs` names as
   `Counterpart::Unnamed { what: "an sRGB gamut" }` and whose test fails, by design,
   once an axis spells it. The same shape of work `output/adobe-rgb-gamut` did for
   Adobe RGB (the colorimetry, `REC709`, already exists).
3. **Once sRGB exists:** the linear float HDR row in sRGB, and a gain-map JPEG on an
   sRGB base (the gain-map destination's path with another base gamut; re-run the
   ISO decoder oracle, since it has only been observed on a Display P3 base).

Not here: the SDR JPEG rows — `output/sdr-jpeg-preset` owns them.

What is known:

- A new ICC profile's description is written into every file, so it is an
  identifier once shipped (CLAUDE.md's `(Hanten)` / `(nc)` rows).
- `nctool`'s `metrics.space_for_destination` keys on (gamut, transfer); each new pair
  needs a verified space or an explicit refusal.
- Memory: the float rows share `NewFlowF32Tiff`'s buffers; the SDR row
  `NewFlowU16Tiff`'s — both claims to confirm by measurement
  (`memory-profiles`).

Open:

- **`--transfer linear` alone becomes ambiguous** once a second linear row exists:
  the table's gamut default (Display P3) would now match a linear row, so it
  resolves to P3 rather than BT.2020 — a silent change of meaning — or, with Adobe
  RGB only, refuses. Options: refuse and ask for `--gamut` (explicit, uniform,
  recommended), or keep BT.2020 through a
  range-dependent gamut default (the asymmetry `preset-set` flagged as a risk).
- Whether Display P3 linear is worth a row beside BT.2020 linear, which contains it.
- Whether the sRGB gain map is read as HDR by Apple ImageIO, and what the legacy
  chain's `compatibility` counterpart message becomes.

## How to Verify

- Each new row resolves from its flags, writes its container, and states every
  resolved axis in `chain.destination`; the destination table's exhaustive remedy
  tests still pass.
- The BT.2020 linear TIFF's bytes and report are unchanged.
- Each float file's embedded profile names its gamut and linear transfer; values above
  1.0 survive.
- `docs/using-nc.md` updated by running the binary.

## Dependencies

- [The destination set](preset-set.md)
- [The gain-map destination](gain-map-destination.md)

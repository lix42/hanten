# Hanten — nf-destinations Progress Log

Execution log for the `nf-destinations` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status; this file is the
narrative beside it.

One `##` section per task in this epic, named by the bare task name. Read this
whole file before starting a task in this epic, and read other epics' `Epic
summary` sections when you depend on them. Append entries — don't rewrite earlier
ones.

## Epic summary

Where a render can go: the destination set, the two renderings (`direct` and `default`), memory profiles, and which destination the default resolves.

**The destination set has landed** (`preset-set`, 2026-09-26). Under `--new-flow` a
destination is four separate knobs — `--range sdr|hdr`, `--transfer
native|linear|pq|hlg`, `--gamut display-p3|adobe-rgb|bt2020`, `--container
tiff|jpeg|avif` (recipe `output.display`) — or `--film-master` (recipe `output`:
`"film-master"`). What other epics need:

- **One table, `destination::ROWS`**, drives resolution, refusals, remedies and the
  container. Adding a destination is adding a row (a `NotYet` row is refused naming its
  task). An unset axis is derived — its default when a consistent row has it, else the
  one value left, else a refusal — independent of which rows are ready. The report's
  `new_flow.destination` states every resolved axis and replays exactly.
- **HDR** is peak `1000/203` in BT.2020; `hdr::from_new_chain` clamps to the peak and
  counts (`new_flow.peak_clamp`, folded into `loss`), then the legacy HDR encoders run.
- **The gain-map JPEG has landed** (`gain-map-destination`, 2026-09-27) and is what
  `--range hdr` alone resolves to: a Display P3 SDR base plus a half-resolution,
  per-channel map to the HDR rendition, ISO 21496-1 only, in an MPF container nc
  writes (`io::iso_gain_map`, no libultrahdr). A flat map is `new_flow.gain_map.flat`,
  never a warning. Verified with the Apple oracle on the CLI's own output.
- **The film master refuses every stage it does not run**, keyed on "asks for" (neither
  default nor identity) per stage.
- Memory arms per buffer shape: `NewFlowU16Tiff` (measured for SDR), `NewFlowF32Tiff`,
  `NewFlowAvif` and `NewFlowGainMapJpeg` provisional.
- nctool keys metrics on (gamut, transfer); a review matrix may state a `destination`.

## preset-set

**Status:** done
**Updated:** 2026-09-26

- 2026-09-19: created with the new-flow plan. Goal: the destination set.
- 2026-09-25: started. The user settled the three open questions: a destination is
  **separate knobs** (range × gamut × container, perhaps the gain-map dialect), not a
  name per combination, with unsupported combinations refused at the CLI; the selectors
  are **new-flow only**, with their own `output` recipe section, and `--output-preset`
  is refused under `--new-flow`; **HDR destinations stay available** before the flip
  where possible (gain map via `render_pair` + `gain_ratio`, PQ, HLG, linear float).
  Recorded in the task file with the design risks to settle before coding.
- 2026-09-25: shape agreed with the user — four axes (`--range`, `--transfer`,
  `--gamut`, `--container`) driven by one table, unset axes derived from it,
  `film-master` its own selection. The per-channel gain-map JPEG split out to
  `gain-map-destination`, because the tree's gain-map writer packages only a
  single-channel luminance map (the legacy XMP dialect cannot signal more).
- 2026-09-25: implemented, awaiting review. What landed:
  - **`src/destination.rs`** is the set: four axes, one table (`ROWS`), and a pure
    `resolve`. Unset axes are derived in order range → transfer → gamut → container
    (default if a consistent row has it, else the one value left, else refuse), **ignoring
    readiness**, so `--range hdr` names the gain-map row and is refused as not yet rather
    than silently becoming a TIFF. Every remedy a refusal offers is proven to resolve by
    an exhaustive test over all stated combinations; offers are the fewest flags to add,
    computed over what the run stated (a flag cannot remove a recipe axis).
  - Written today: SDR `native` TIFF in Display P3 (default, byte-identical to before) or
    Adobe RGB; HDR BT.2020 `linear` f32 TIFF, `pq`/`hlg` u16 TIFF or AVIF.
    `DestinationGamut::Bt2020` reuses the pinned `ACESCG_TO_BT2020` / `BT2020_LUMA`.
  - **HDR hand-off** `hdr::from_new_chain` clamps to `[0, 1000/203]` and counts what it
    clamped (`new_flow.peak_clamp`, folded into `loss`, so `--strict` sees it), then the
    legacy HDR encoders and report blocks are reused unchanged.
  - **`film-master` refuses every stage it does not run**, one rule per stage, keyed on
    "asks for" (not the default and not the identity): scene correction, the look, fit
    range. The legacy chain already refused the same; the look-only rule the task named
    would have silently ignored a roll's measured white balance.
  - The HDR "SDR-range signal" warning is flow-aware (new flow names `--exposure` /
    `--range sdr`); legacy text byte-identical.
  - Memory: `NewFlowSdrTiff` → `NewFlowU16Tiff` (SDR and coded HDR share it);
    `NewFlowF32Tiff` and `NewFlowAvif` are **provisional, counted not measured** —
    `memory-profiles` owes the measurement.
  - nctool: `metrics.space_for_destination` keys on (gamut, transfer) and refuses an axis
    left to derivation; a review matrix may state `destination` instead of
    `output_preset`. One matrix still cannot hold a reference cell and a new-flow cell.
  - Review: nc-reviewer plus a cold stand-in (Codex was out of credits); 16 findings and
    two follow-ups fixed.
- 2026-09-26: done. Rebased onto `nf-retire/regional-balance`; the ship review (Codex,
  back online, and `ship:diff-reviewer`) found three more, all fixed:
  - **A roll's per-frame `output.display` now merges axis by axis.** `DisplayAxes`
    writes only stated axes, so two one-key axis objects looked like an
    externally-tagged enum switch to `cli::merge_json` and replaced the shared recipe's
    axes; `is_variant_switch` now exempts `destination::AXIS_KEYS`. Any future sparse
    (`skip_serializing_if`) struct in a recipe hits the same heuristic.
  - The suffix refusal names the film master the way the user chose it (flag or recipe).
  - `pipeline::hdr::sdr_range_warning` takes an `SdrRangeLevers` enum, not `Flow`:
    `Flow` never reaches a stage module.
  - For dependents: `direct-preset` is now a rendering question (the gamut is a knob);
    `memory-profiles` owes measurements for `NewFlowF32Tiff` and `NewFlowAvif` (and the
    coded-HDR use of `NewFlowU16Tiff`); `default-destination` moves axis defaults, not a
    name; `gain-map-destination` adds a ready row, after which `--range hdr` alone
    resolves to it.

## direct-preset

**Status:** not started
**Updated:** 2026-09-27

- 2026-09-19: created with the new-flow plan. Goal: the direct destination for external editing.
- 2026-09-27: **re-planned with the user** (`docs/design-update.md`, Part 2, "Two
  renderings"). Since `roll-white-rule`, the roll's measurements live in rendering, so the
  task is now `--rendering direct|default`: `direct` loses as little as possible and
  applies only what the container needs (roll section unapplied, white balance identity,
  `look.contrast` pinned at `2.0 / 1.8`, desaturation off, display black 6 stops and
  reinhard at 6 stops pinned, SDR Adobe RGB when the axes are unset); `default` is our
  code plus the roll's measurements, with today's defaults for the rest and a warned
  fallback without a roll section. Explicit knobs build on either base: white balance
  multiplies, everything else (contrast included, until `nf-look/contrast-definition`)
  replaces. Decisions along the way: display black stays on in `direct`, because it
  stretches the shadows rather than compressing them; `direct` stays SDR, because Adobe
  RGB has no HDR row and `film-master` is already the lossless output; `direct`'s values
  are pinned in a test that names the procedure for re-deciding them. Filed
  `nf-calibration/roll-section` (now a dependency), `nf-calibration/no-roll-defaults`,
  `nf-look/contrast-definition` and `nf-core/three-step-pipeline`. Shipped as a three-PR
  stack: this re-plan, then `roll-section`, then this task.
- 2026-09-27: **`direct` defaults to HDR** (user changed their mind: range matters more
  than gamut). The unset axes resolve to the linear 32-bit float BT.2020 TIFF — the least
  lost, and the one HDR row reachable before the gain-map row is ready — with Adobe RGB
  as `direct`'s gamut when SDR is stated. The by-eye form, and the one the calibration
  loop holds, is `--rendering direct --range sdr`. The memory profile to measure is now
  `NewFlowF32Tiff` as well.
- 2026-09-28: rebased onto `gain-map-destination`. `--range hdr` alone now resolves to
  the gain-map JPEG, so `direct`'s float TIFF rests on its own `linear` transfer default,
  and its reason is only "least lost". `easy-destination-rows` plans the float row in
  Adobe RGB; `direct`'s unset gamut is already Adobe RGB, so it will resolve there
  unaided — a move of `direct`'s output, to log in `nf-calibration`'s progress when it
  lands.

## memory-profiles

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: a memory profile per destination.

## default-destination

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: which destination the default resolves.

## gain-map-destination

**Status:** done
**Updated:** 2026-09-27

- 2026-09-25: split out of `preset-set` (user decision). Goal: the new chain's HDR JPEG
  with a per-channel ISO 21496-1 gain map.
- 2026-09-27: started. The user settled the design questions: an **ISO-only file in a
  container nc writes** (JFIF, ICC, ISO segments, MPF typed `030000`/`050000`; no XMP,
  no libultrahdr, since `package()` always adds legacy XMP that cannot describe an RGB
  map); a **half-resolution** three-channel map (full resolution was considered first
  and dropped by the user); a flat map is a **report field only**, never a warning.
  `--range hdr` alone resolves here once the row is ready. Order of work: the pure
  gain-map encoder, the container, the destination row and dispatch, the report and
  memory arm, then the Apple oracle on the CLI's own output — which also answers the
  one open risk, whether ImageIO needs anything libultrahdr's container wrote.
- 2026-09-27: implemented. What landed:
  - **`pipeline::gain_encode`** quantizes `gain_ratio::GainRatios` into a
    half-resolution, 8-bit, three-channel map: each channel over its own exact `log2`
    window, gamma 1, centre-aligned bilinear in the `log2` domain, offset `1/64`. A
    constant channel states `min == max` and writes code 0, so a flat map is all zeros
    with a zero window — never widened to look live.
  - **`io::iso_gain_map`** writes the file with no libultrahdr: base `SOI · JFIF · ICC ·
    ISO version · MPF · SOF0…`, gain map `SOI · JFIF · ISO metadata · SOF0…`, every APP
    segment in the header block by construction (the encoder writes them there; nothing
    is spliced). MPF follows libultrahdr's layout but types the gain map `050000`; it is
    written as a same-sized placeholder and filled in once the base's length is known.
    The JPEG encode and 8-bit quantize moved to `io::jpeg`, shared with `io::ultra_hdr`
    (bytes unchanged).
  - `iso::fields` builds the ISO field set from plain values; `iso::project` (the
    current chain's) feeds it. The dead `encode_iso_gain_map` is gone.
  - The row is `Ready(Encoding::GainMapJpeg)`, so `--range hdr` alone resolves here and
    `--container jpeg` alone is the not-yet SDR JPEG, offering `--range hdr`.
    `hdr::clamp_to_peak` is the clamp-and-count `from_new_chain` used, now shared. The
    report's chain account is the HDR rendition's; `new_flow.gain_map` adds the range,
    `flat`, the map's size and `base_fit_range`. No SDR-range warning, as for the legacy
    gain map. `--output-preset gain-map-hdr` under `--new-flow` now points at `--range
    hdr`; `ultra-hdr-v1` gets a new `Counterpart::Nearest` saying how the file differs.
  - Memory: `RunProfile::NewFlowGainMapJpeg`, **provisional** — render `3 × image +
    12 B/px` gains, encode that plus 12 B/px byte staging (`memory-profiles` owes the
    measurement).
  - **Oracle (Apple ImageIO, macOS 26.5), on the CLI's own output**: `PRESENT` with three
    *different* `ChannelMetadata` entries — the fixture at `+3 EV` reads `GainMapMax`
    2.2827 / 2.2177 / 2.2827 log2; a real Ektar frame (2026-09-09-Ektar100 `1605`) at the
    **default** exposure reads 1.918 / 1.486 / 1.192, matching nc's report (`max`
    3.778 / 2.801 / 2.284 linear). So ImageIO reads a container libultrahdr did not
    write, and the `050000` type code — the task's one open risk. The flat fixture at
    `-5 EV` reads `GainMapMax = 0` on every channel. ImageIO hands the map back as
    `420f` (biplanar YCbCr), where the legacy map is `L008`. Chrome and an HDR display
    were **not** checked.
  - A JPEG round-trip test decodes both images with the `image` crate; on a hard 0→255
    step inside one 8×8 DCT block, q95 and RGB→YCbCr ring the gain by ~5 codes, so the
    bound is 8 codes on that worst case.
- 2026-09-27: review (`/code-review high`, 10 findings, 7 fixed, 3 declined):
  - **`flat` is now within rounding, not exact**: `gain_ratio::FLAT_TOLERANCE_LOG2`
    (1/510 stop). The two branches round differently where they agree by construction —
    a real frame measured `min` 0.9999999 — so the exact test could call a flat frame
    live, and the map would spread that noise across 0–255. A flat map is now written
    inert (all zeros, zero window); `min`/`max` stay exact in the report.
  - `gain-map-hdr`'s `--new-flow` counterpart is `Nearest` too: the new file drops the
    Ultra HDR v1 XMP that preset writes, so "same kind of file" was false.
  - One exhaustive match on `Encoding` in the render dispatch (`render_one` /
    `render_gain_map`), removing an unreachable runtime-error arm.
  - The IR plane is dropped before `render_pair` (`AcesCgImage::without_ir`) rather than
    copied into the split and discarded: `NewFlowGainMapJpeg`'s render term is now
    `2 × image + 2 × 12 B/px` (was `3 × image + 12 B/px`).
  - `io::jpeg::encode_jpeg` takes its APP2 segments by value; a stale `flow.rs` doc; the
    user guide's unclosed fence.
  - Declined: counting base highlights above 1.0 as `loss` (the SDR base is a
    deliverable an SDR viewer sees clipped, and `gain-map-hdr` counts them the same
    way); routing the downsample through `pipeline::pixels` (a resample with no
    reduction, deterministic — commented instead); "no decoder evidence for `min ==
    max`" (the oracle read the flat −5 EV file `PRESENT` with `GainMapMax = 0`, above).
- 2026-09-27: ship review (`ship:diff-reviewer` + Codex; Codex found nothing). Fixed: the
  gain map's `loss` counted the HDR rendition's peak clamp against the SDR base's
  sample count alone, so a far-overexposed frame reported 200 % loss — the total now
  includes the second rendition's samples (`NewFlowRender::second_rendition_samples`),
  pinned by a `--exposure 12` test. The guide now says `gain-map-hdr` gets a "nearest"
  counterpart too. Raised for the user rather than applied: CLAUDE.md's "where the
  detail lives" row should name `io/iso_gain_map.rs` and `pipeline/gain_encode.rs`;
  and `memory-profiles` should measure `NewFlowGainMapJpeg` on a grainy full-size frame,
  since q95 4:4:4 JPEGs of grain can exceed the provisional 12 B/px staging bound.

## easy-destination-rows

**Status:** not started
**Updated:** 2026-09-27

- 2026-09-27: created from a survey (with the user) of lossless × HDR × gamut: the rows
  classed easy — linear float HDR in Adobe RGB and Display P3, sRGB as a gamut, then
  sRGB's float and gain-map rows. Motivated by keeping a lossless Adobe RGB HDR output
  available, not by a workflow that needs it: for Lightroom HDR editing the gamut
  matters little (Lightroom converts to its own wide space), and the user is taking
  `direct-preset` themselves. Classed harder and left out: PQ/HLG in P3 or sRGB
  (signalable, uncommon), a gain map on an Adobe RGB or BT.2020 base (reader support
  unknown), ProPhoto (D50: the gamut stage is D65), lossless AVIF and lossless
  gain-map containers (JPEG XL, AVIF with a gain map); PQ/HLG in Adobe RGB or ProPhoto
  have no standard signalling code at all.

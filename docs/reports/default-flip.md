# Render defaults v7 → v8: the chain flip

Measured baseline for the `pipeline_version` 8 default (2026-09-27,
`nf-core/default-flip`). The rendering chain of [`design-update.md`](../design-update.md)
— the fixed decode, scene correction, the look, fit range and fit gamut — became the
only one, and with it the default output moved:

| default | v7 | v8 |
|---|---|---|
| chain | the removed preset chain (print stage, display tone, preset renderers) | the fixed decode → the four rendering stages, `default` rendering |
| output | `gain-map-hdr`: dual-dialect gain-map JPEG | **SDR Display P3 16-bit TIFF** |
| recipe | unversioned (the removed chain's) | `"recipe_version": 2` |

Like [v3](render-defaults-v3.md), this is a **container** change as well as a render
one: `hanten convert -o out.jpg` with no destination flag is now a usage error naming
`--range hdr --container jpeg`, and `-o out` is completed to `out.tiff`. A recipe or
sidecar written before v8 is refused whole; its film base carries across by hand
(`docs/using-nc.md` §5).

Produced by [`scripts/default-flip/measure.py`](../../scripts/default-flip/measure.py)
against the pre-flip `main` (`d03e471`, v7) and this branch's release build, on the
first real frame of each roll the asset manifest can freeze. The film base is measured
once per roll from its unexposed frame and stated in every run. Re-run it before
quoting anything here.

## v8's default is v7's `--new-flow` render, byte for byte

| roll/frame | v7 default: mean RGB (JPEG) | v8 default: mean RGB (TIFF) | clipped v7 → v8 | v8 = v7 `--new-flow` | est. peak memory v7 → v8 |
|---|---|---|---|---|---|
| `2026-07-15-Ektar100/971.tif` | 0.4242, 0.4258, 0.4512 | 0.3897, 0.3915, 0.4141 | 4.277% → 3.978% | yes | 1.85 → 0.95 GB |
| `2026-07-23-Portra160/1102.tif` | 0.4047, 0.3881, 0.4176 | 0.3619, 0.3463, 0.3748 | 3.523% → 2.777% | yes | 1.85 → 0.95 GB |
| `2026-07-24-Gold200/1137.tif` | 0.3538, 0.3344, 0.3319 | 0.3014, 0.2844, 0.2832 | 3.422% → 2.810% | yes | 1.85 → 0.95 GB |
| `2026-09-09-Ektar100/1605.tif` | 0.4457, 0.5341, 0.5881 | 0.4353, 0.5242, 0.5763 | 0.000% → 0.000% | yes | 1.66 → 0.86 GB |
| `2026-09-11-Portra400/1641.tif` | 0.1710, 0.1582, 0.1427 | 0.0832, 0.0738, 0.0624 | 0.000% → 0.000% | yes | 1.65 → 0.85 GB |
| `2026-09-13-Portra400/1675.tif` | 0.1998, 0.2183, 0.2134 | 0.1277, 0.1432, 0.1398 | 0.000% → 0.000% | yes | 1.65 → 0.85 GB |
| `2026-09-14-Ektar100/1713.tif` | 0.3777, 0.4549, 0.5010 | 0.3552, 0.4340, 0.4793 | 0.000% → 0.000% | yes | 1.67 → 0.86 GB |
| `2026-09-18-Gold200/1774.tif` | 0.2652, 0.2748, 0.2603 | 0.2086, 0.2181, 0.2050 | 0.000% → 0.000% | yes | 1.65 → 0.85 GB |
| `2026-09-20-Portra400/1867.tif` | 0.2465, 0.2325, 0.2310 | 0.1684, 0.1581, 0.1554 | 0.000% → 0.000% | yes | 1.66 → 0.86 GB |

`mean` is `output_stats.mean`: for v7 the normalized 8-bit SDR base handed to the JPEG
encoder, for v8 the u16 samples normalized to `[0, 1]` — comparable as rendered values,
not as stored bytes.

- **The flip moves no pixel the new chain renders.** Every v8 TIFF is identical, byte
  for byte, to the same frame under v7 `--new-flow` (the `default` rendering, no roll
  measurement). What changed for a user is which chain runs without a flag, not the
  chain.
- **Against the old default every frame renders darker**, by 0.01–0.09 in encoded
  mean. The largest drops are the low-key frames (`1641`, `1675`, `1867`), where display
  black moves the film base down to 6 stops under mid-grey and the look's contrast
  deepens the shadows; the brightest, high-key Ektar frames move least. This is the new
  chain's look, reviewed under `nf-display-stages` and `nf-look`, not a defect of the
  flip; a roll measured with `hanten measure-roll` renders at its own white instead of
  the fallback contrast used here.
- **Three older rolls clip at defaults, slightly less than before** (4.3 → 4.0%,
  3.5 → 2.8%, 3.4 → 2.8%); the other six clip nothing under either.
  `--display-tone-headroom` or a measured roll white is the control
  (`docs/using-nc.md` §7).
- **The estimated peak roughly halves** (1.65–1.85 → 0.85–0.95 GB at 16–19 MP),
  because the default no longer builds an SDR/HDR pair and a gain map. These are the
  memory preflight's model estimates, not RSS; `pipeline/memory.rs` holds the
  measured calibration of both profiles.

## What else moved

- **A no-roll warning at defaults.** Under the `default` rendering a run with no roll
  measurement warns that it fell back (`nf-calibration/roll-section`), so a plain
  `convert` is no longer `--strict`-clean until the roll is measured, the white balance
  and contrast are stated, or `--rendering direct` is chosen.
- **No sidecar.** v7 wrote `<output>.json` beside every image; v8 writes none and
  removes a stale one it replaces (`nf-core/report-contract` owns what comes back).
- **The report** carries the chain's `new_flow` block and no `recipe` echo,
  `output_render`, `reconstruction_result` or `identity.params_hash`; telemetry is
  `schema_version` 8 with `conversion.destination`.
- **The drift gate** (`version::PIPELINE_FINGERPRINTS`) hashes the fixed decode for v8,
  so its `render` fingerprint moved with the code it covers; `base` did not.

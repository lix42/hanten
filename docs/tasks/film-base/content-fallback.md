# Content-based film-base fallback (Tier 3)

> **Re-scoped 2026-10-01** for the new chain. The original plan was a per-frame
> `convert --base-content` / `calibration.film_base = "content"`, reached when
> `--auto-base` failed. `--auto-base` is gone
> (`film-base/holder-masked-measurement`), and the film base is now a **roll** value:
> `measure-roll` refuses a per-frame base ("a base estimated per frame would measure
> each frame under a different decode"). The original plan is in git.

## Goal

An explicit, opt-in way to measure a roll's `Dmin` from its exposed **content** when
the roll has no unexposed frame and no visible rebate: the design-spec §9 acquisition
ladder's Tier 3. The deepest black in a scene is the thinnest part of the negative,
close to the true base. **Never a silent fallback**: the user opts in, and the report
records where the base came from.

## Background

Real-scan verification (Phoenix / Ektar rolls, 2026-07) found exposed frames with no
clean rebate on any edge. Two facts from it still hold:

- **Colour-agnostic.** The estimate is per channel, so it works for any mask colour
  (Ektar base R/B ≈ 2.75, Phoenix ≈ 0.84).
- **Spatial accuracy is the risk.** Without a rebate the per-channel maximum is the
  scene's deepest shadow: slightly denser than base, and badly wrong if anything
  brighter than the base is in frame (clear overscan, a light leak), which washes the
  output out.

## Design

- **A `measure-roll` option** (`--base-content`): a per-channel high percentile
  (~p99, `film_base::percentile`) pooled over every picture frame's **effective area**
  (holder cut from the IR plane, then the inset). Pooling over the roll lands nearer
  the true base than one frame does, and gives the roll one base, as the decode needs.
- **It writes an explicit base.** `--out` writes `calibration.film_base` as the
  measured values; the report records that they came from content, with the per-frame
  spread. Replay is then an ordinary explicit base.
- **The no-base refusal names it** beside `--film-base`, `--base-region` and
  `measure-base`.
- **Guard the over-bright failure**: warn (`--strict` promotes) when a frame's or the
  pool's estimate sits near or above a plausible transmission ceiling, or when one
  frame disagrees with the rest.
- Document the wash-out failure (foggy or high-key roll → no near-black → raised,
  cast blacks), recoverable downstream as a global cast.

## Open questions

- **Does `convert` get a per-frame form at all** (`--base-content` /
  `calibration.film_base = "content"`) for a single frame with no roll? It would be a
  per-frame base, which the roll workflow avoids. If not, no recipe value is needed.
- Can it combine with `--unexposed` or `--leader` (refuse, or cross-check)?
- The percentile and the guard thresholds — tuned on real scans.

## How to Verify

- Synthetic frames with a known deepest-black patch → the pooled base ≈ that patch;
  an over-bright patch trips the warning.
- Real roll with no rebate (e.g. Ektar `971`): a finite base and a plausible convert;
  measure the bias against the roll's true `Dmin` from its unexposed frame `963`.
- The no-base refusal names the option; the report records the source.

## Dependencies

- [Film-base / Dmin estimation](estimation.md)
- [`measure-base`, and `measure-roll` as the one-stop measurement](../core/measure-base.md)
  — the command this option joins.

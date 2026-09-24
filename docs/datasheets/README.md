# Manufacturer datasheets

The published sheets the film-stock registry is derived from, kept in-repo so every pinned
number can be re-checked against its source without hunting for the PDF again. Manufacturer
publications are revised, and a revision can change the artwork a curve was read off, so the
**file is the provenance** — a URL is not.

They are third-party publications, reproduced here unmodified for verification. Nothing in
this directory is nc's work.

## How the numbers get out of them

```
docs/datasheets/*.pdf
        │  python3 scripts/analysis/digitize_datasheets.py       (needs poppler; run by hand)
        ▼
src/film_stock/curves.json               ← the digitized extraction
        │  …--emit-rust
        ▼
src/film_stock/curves.rs                 ← the pinned literals
```

`film_stock::tests::curves_match_the_digitized_json` audits the last arrow on every
`cargo test`, so the Rust literals cannot drift from the extraction. The first arrow needs
poppler and is *not* in CI — re-run it by hand when a sheet is added or replaced, and commit
both outputs. This mirrors `pipeline/colorimetry/`, where the generator is also manual and
only the correspondence is automated.

Extraction works because the Kodak still-film sheets are **vector art with no raster layer**:
the plot frame gives the density calibration and the axis ticks the exposure calibration, so
the curves are read exactly rather than eyeballed.

## What reads them

The registry's only runtime reader is the `characteristic` reconstruction curve, which
inverts a stock's tables and retires with `nf-retire/characteristic`. The data stays after
it (`nf-look/stock-data-home`), for two reasons:

- **It is the evidence for the fixed decode's constants.** `algo::fixed::MID_ABOVE_BASE` is
  `generic-c41`'s mid aim, and the case for fixed, stock-agnostic values rests on these
  sheets' own spread (`docs/design-update.md` Part 1). `film_stock`'s tests check both on
  every run, reading the tables forward so they outlive the inversion.
- **Its natural future reader is the look stage**, as an optional per-stock normalization
  — planned, not scheduled, and never again a per-stock decode.

## In the registry

| File | Publication | Stocks |
|---|---|---|
| `e4046-2025-01-ektar-100.pdf` | E-4046 | `ektar-100` |
| `e4051-2025-01-portra-160.pdf` | E-4051 | `portra-160` |
| `e4050-2025-01-portra-400.pdf` | E-4050 | `portra-400` |
| `e4040-2025-01-portra-800.pdf` | E-4040 (2025) | `portra-800` |
| `e4040-2009-02-portra-160nc-160vc-400nc-400vc-800.pdf` | E-4040 (2009) | `portra-160vc`, `portra-400vc` |
| `e7022-2023-06-gold-200.pdf` | E-7022 | `gold-200` |
| `e7023-2016-02-ultramax-400.pdf` | E-7023 | `ultramax-400` |
| `e7024-2007-12-ultramax-800.pdf` | E-7024 | `ultramax-800` |

`generic-c41` is derived — the average of all nine, resampled on their common exposure
range — and has no sheet of its own.

**E-4040 names two different documents**: the 2009 five-stock Portra sheet and the 2025
Portra 800 one. The identifier is therefore the `(publication, revision)` pair, which is why
both are reported.

The Feb-2016 revisions of E-4046 / E-4050 / E-4051 / E-7022 were also checked: their
*Judging Negative Exposures* aim tables are identical to the revisions above, so nothing in
the registry turns on which was used. Only one revision per publication is kept, to avoid a
second copy that looks equally authoritative.

## Collected, not yet consumed

Black-and-white sheets, for `algo/bw-support`: Kodak T-Max 100 / 400, Tri-X 320/400 and the
`ED-BWF` processing sheet; Ilford Delta 100 / 400 / 3200, FP4+, HP5+, Pan F+, SFX 200, Ortho
Plus, XP2 Super; Kentmere Pan 100 / 200 / 400.

B&W cannot use the registry's shape. A B&W film has **no fixed gamma** — contrast index is
set by developer, time and temperature, which is why those sheets are mostly development
tables (Tri-X's recommended times target CI 0.56). A B&W profile is therefore
`stock + developer + time → CI`, not `stock → curve`.

Two further limits worth recording, both established while building this:

- **Ilford, Harman and Kentmere sheets are raster**, not vector, so this extractor cannot
  read their curves at all — they would need image processing.
- **Harman Phoenix has no published curve anywhere**: its datasheet carries no characteristic
  curve and no densities, only ISO, a wedge spectrogram, reciprocity and scanner settings. It
  is also unmasked. A Phoenix roll can never be datasheet-backed.

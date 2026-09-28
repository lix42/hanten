---
name: perf-telemetry
description: >-
  How Hanten's embedded performance + context telemetry works — collecting, reading,
  and extending it. Use when adding a telemetry field or event to a new feature,
  turning on / collecting perf logs from `hanten convert` (`--telemetry`,
  `--telemetry-file`, `NC_TELEMETRY_LOG`), reading or analyzing the telemetry JSONL
  log (jq over per-stage timing / megapixels), bumping the record
  `schema_version`, or reasoning about the determinism and fail-soft invariants the
  telemetry code must preserve.
---

# Hanten perf telemetry

`hanten convert` can emit one JSON **telemetry record** per **successful** run — image
facts, per-stage timings, a compact conversion summary, and the outcome — to a
local append-only JSONL log and/or a one-off file. It is **opt-in**,
**best-effort**, and never perturbs the converted image. A record's existence is
the success signal (there is no `outcome.success` field), so a run that exits
non-zero — including a `--strict` warning promotion — writes **no** record. Full design: design-spec §9 (record shape) and §12
(roadmap). Code: `src/telemetry.rs` (record + builder + sinks), wired from
`cli::run_convert` / `cli::emit_telemetry`. Per-stage timings are a `TimingInfo`, one
field per `crate::stage::StageKind`: it is the `StageClock` the orchestrator
(`cli::convert_frame`, `render_frame`, `render_destination`) and `pipeline::chain`
time each stage through, so the stages themselves never read a clock.

## 1. Adding telemetry when you build a feature

The record is built in one place: `telemetry::build_record(RecordInputs) ->
TelemetryRecord` (`src/telemetry.rs`). To add a field:

1. Add it to the right nested struct — `ImageInfo`, `TimingInfo`, `ConversionInfo`,
   or `OutcomeInfo` (or `TelemetryRecord` for run-context). They derive
   `Serialize`; the JSON key is the field name verbatim (no `rename`). Use
   `#[serde(skip_serializing_if = "Option::is_none")]` for a field that is absent in
   some runs (as `timing_ms.ir_export` does).
2. Feed it in: add a field to `RecordInputs<'a>` and set it in `build_record`; then
   populate it at the call site in `cli::emit_telemetry` (which gathers everything
   after the conversion has succeeded). A new stage is a `StageKind` member, a
   `TimingInfo` field and a `TimingInfo::add` arm, timed with `clock.time(stage, ||
   …)` where it runs — never an `Instant` inside a stage.
3. **Bump `SCHEMA_VERSION`** (`src/telemetry.rs`) whenever the wire shape changes —
   a new/removed/renamed field, or a changed type. Note this also applies to the
   embedded domain types (`OutputSection`, `FilmBaseSource`, `SilverFastFormat`,
   and `StageKind` through the timing field names): if *their* serialization changes,
   bump too. The server keys ingestion off this.
4. Prefer fixed-width wire types (`u32`/`u64`, not `usize`) and reuse domain enums
   rather than restringifying them.

### Two invariants every addition MUST preserve

- **Determinism — never touch the deterministic path.** The record must not enter
  the recipe or change the output image bytes. Telemetry on vs off ⇒ byte-identical
  output (guarded by the `telemetry_does_not_perturb_the_output` test). Telemetry is
  gathered/written *last*, after the image is on disk, and only *reads* its finished
  facts. Never route a telemetry value back into a stage or the recipe.
- **Fail-soft — telemetry must never fail a conversion.** A telemetry write/serialize
  failure warns on stderr and is swallowed (exit stays 0). It must NOT enter
  `report.warnings` (that would let `--strict` promote it), and it is surfaced even
  under `--quiet` (via `Log::warn_always`, used by the `warn` closure in
  `emit_telemetry`, mirroring the `non_finite` precedent). The one loud exception
  is a `--telemetry-file`/log path
  that *collides* with a real artifact (input/output/IR export/report-file) — that's a
  config error caught up front (exit 2), so telemetry can't clobber real data.

Do NOT add these flags to `ResolvedConfig`/`*Params`/`merge`/`validate`: telemetry
is **operational**, not a conversion knob (the four-coupled-spots rule does not
apply).

## 2. Collecting perf logs

Both sinks are opt-in; telemetry is collected iff at least one flag is present, and
both may be combined.

```bash
# Append one record (one line) to the persistent JSONL log.
hanten convert in.tiff -o out.tiff --film-base 0.9,0.55,0.42 --telemetry

# Also write this run's record to a one-off file (overwrites). `-` = stdout.
hanten convert in.tiff -o out.tiff --film-base 0.9,0.55,0.42 --telemetry-file run.json

# Both sinks at once; send the one-off to stdout (pair with --report none so
# stdout carries only the telemetry line, since the report is on stdout by default).
hanten convert in.tiff -o out.tiff --film-base 0.9,0.55,0.42 \
  --telemetry --telemetry-file - --report none
```

Default log path (first match wins): `$NC_TELEMETRY_LOG` → `$XDG_DATA_HOME/nc/telemetry.jsonl`
→ `%APPDATA%\nc\telemetry.jsonl` (Windows) → `$HOME/.local/share/nc/telemetry.jsonl`.
Point a whole batch at a scratch log with `NC_TELEMETRY_LOG=/tmp/nc.jsonl`. The log
is create-append with parent dirs created; **one compact JSON object per line**, so
N runs append N lines. `--telemetry-file <path>` overwrites (a single record).

## 3. Reading the logs

Each line is a standalone JSON object with this shape (see `src/telemetry.rs`):

```json
{ "schema_version":9, "timestamp_ms":1790633724299,
  "nc_version":"0.1.0", "target":"aarch64-apple-darwin", "cpu_count":11,
  "image":{"format":"hdri","width":502,"height":462,"megapixels":0.231924,
           "bit_depth":16,"channels":3,"ir_present":true,
           "input_bytes":2017230,"output_bytes":1392366},
  "timing_ms":{"total":59.9,"decode":15.3,"film_base":0.0,"reconstruction":5.6,
               "scene_correction":0.0,"look":1.7,"fit_range":4.0,"fit_gamut":7.8,
               "destination":5.8,"encode":6.0,"ir_export":6.2},
  "conversion":{"destination":{"display":{"range":"sdr","transfer":"native",
                                          "gamut":"display-p3","container":"tiff"}},
                "params_hash":"a6bcbaf9b33f4480",
                "film_base_source":{"explicit":[0.9,0.55,0.42]},
                "output_depth":"u16"},
  "outcome":{"warnings":1,"clipped":0,"non_finite":0} }
```

`timing_ms.ir_export` appears only when `--export-ir` ran, and the four chain stages
(`scene_correction` … `fit_gamut`) not for the film master, which runs none; a gain
map's `fit_range` and `fit_gamut` sum its two renditions (the copy that splits them
counts only toward `total`), and `scene_correction` and `look` include the film base's
one-pixel grade. **v9**
(`nf-core/report-contract`) replaced `algorithm` with `reconstruction` and split `color`
into the four chain stages and `destination`. (Schema v2 replaced v1's
`conversion.algorithm` with the `reconstruction` + `curve` pair; **v5** dropped
`reconstruction` when `simple` retired and made `curve` always present. Records
written before it carry `reconstruction` and may name `simple` or `sigmoid`. **v6**
dropped `conversion.dmax` when the roll reference density retired; older records may
carry it. **v7** dropped `conversion.curve` when the `characteristic` curve retired and
left it one-valued; older records carry it.)
`conversion.destination` is the resolved recipe `output` — `"film-master"` or
`{"display": {range, transfer, gamut, container}}` with every axis resolved. **v8**
(`nf-core/default-flip`) replaced **v3**'s `conversion.preset` with it; older records
carry a preset name (`gain-map-hdr`, `display-p3`, …, and before
`nf-retire/legacy-custom` `legacy` or `custom`). **v4** (2026-08-09) renamed
`conversion.output_hdr` to `conversion.output_depth` (`u8`|`u10`|`u16`|`f32`); it reports
the **primary image's** depth (`cli::primary_depth`), which for the JPEG and AVIF
destinations is the container's fixed 8/10-bit.
`params_hash` is a stable FNV-1a (`version::stable_hash`) of the canonical
effective-recipe JSON — the `"recipe_version": 2` document, the exact bytes
`--dump-params` writes — so identical conversions share a hash, and it equals the
report's `identity.params_hash`. Records before v8 hashed the removed chain's recipe,
so the two never compare. The sample value above is
**illustrative**: it covers the whole recipe and changes when any key is added, removed,
or re-defaulted. Do not assert it as a constant.

`jq` recipes over the JSONL log (`jq -c` reads it line by line):

```bash
LOG="${NC_TELEMETRY_LOG:-${XDG_DATA_HOME:-$HOME/.local/share}/nc/telemetry.jsonl}"

# Per-stage timing for every run.
jq -c '{ts:.timestamp_ms} + .timing_ms' "$LOG"

# Where a run's time went: the chain's four stages together, beside the rest.
jq -c '.timing_ms | {total, decode, reconstruction, destination, encode, \
         chain: ([.scene_correction, .look, .fit_range, .fit_gamut] | map(. // 0) | add)}' "$LOG"

# Only film-master runs.
jq -c 'select(.conversion.destination == "film-master")' "$LOG"

# Megapixels vs total ms (TSV — feed a scatter / spot the slow ones).
jq -r '[.image.megapixels, .timing_ms.total] | @tsv' "$LOG"

# Throughput: megapixels per second per run.
jq -r '[.image.megapixels, (.image.megapixels / (.timing_ms.total/1000))] | @tsv' "$LOG"

# Runs that clipped or hit a non-finite sample.
jq -c 'select(.outcome.clipped > 0 or .outcome.non_finite > 0) \
       | {ts:.timestamp_ms, destination:.conversion.destination, clipped:.outcome.clipped}' "$LOG"

# Group timing stats by nc_version (across runs).
jq -s 'group_by(.nc_version)[] | {version: .[0].nc_version, runs: length, \
        avg_total_ms: (map(.timing_ms.total) | add/length)}' "$LOG"
```

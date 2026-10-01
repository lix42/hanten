---
name: perf-telemetry
description: >-
  How Hanten's embedded performance + context telemetry works — collecting, reading,
  and extending it. Use when adding a telemetry field or event to a new feature,
  turning on / collecting perf logs from `hanten convert` (`--telemetry`,
  `--telemetry-file`, `NC_TELEMETRY_LOG`), reading or analyzing the telemetry JSONL
  log (jq over per-stage timing / megapixels / failures), bumping the event
  `schema_version`, working on the opt-in uploader (`hanten telemetry`), or reasoning
  about the determinism and fail-soft invariants the telemetry code must preserve.
---

# Hanten perf telemetry

`hanten convert` can emit one JSON **telemetry event** per run that parses — a
**success**, or a **failure** naming the stage it ended in, its `error_kind` and exit
code — with image facts, per-stage timings and a compact conversion summary as far
as the run got, to a local append-only JSONL log and/or a one-off file. It is
**opt-in**, **best-effort**, and never perturbs the converted image. A `--strict`
promotion is a failure of kind `strict`. Full design: design-spec §9 (event shape)
and §12 (roadmap). Code: `src/telemetry.rs` (event + builder + sinks), wired from
`cli::run_convert` (which gathers facts into a `ConvertAttempt`) and
`cli::emit_telemetry`. Per-stage timings are a `TimingInfo`, one field per
`crate::stage::StageKind`, filled by `telemetry::StageTimer` — the `StageClock` the
orchestrator (`cli::convert_frame`, `render_frame`, `render_destination`) and
`pipeline::chain` time each stage through, so the stages themselves never read a
clock. The clock's closures return `Result`, which is how it knows the failed stage
and leaves that stage's time out of its field.

## 1. Adding telemetry when you build a feature

The event is built in one place: `telemetry::build_event(EventInputs) ->
TelemetryEvent` (`src/telemetry.rs`). To add a field:

1. Add it to the right nested struct — `ImageInfo`, `TimingInfo`, `ConversionInfo`,
   or `OutcomeInfo` (or `TelemetryEvent` for run-context). They derive
   `Serialize`; the JSON key is the field name verbatim (no `rename`). A fact a
   failed run may not have reached is an `Option` with
   `#[serde(skip_serializing_if = "Option::is_none")]` — absent, never zeroed.
2. Feed it in: add a field to `EventInputs<'a>` and set it in `build_event`; record
   it on `cli::ConvertAttempt` where the run learns it, and read it in
   `cli::emit_telemetry`. A new stage is a `StageKind` member, a `TimingInfo` field
   and a `TimingInfo::add` arm, timed with `clock.time(stage, || …)` where it runs —
   never an `Instant` inside a stage. A stage whose `Err` is not the run's failure (a
   fallback) returns `Ok` of its inner result.
3. **Bump `SCHEMA_VERSION`** (`src/telemetry.rs`) whenever the wire shape changes —
   a new/removed/renamed field, or a changed type. Note this also applies to the
   embedded domain types (`OutputSection`, `FilmBaseSource`, `SilverFastFormat`,
   and `StageKind` through `stage` and the timing field names): if *their* serialization changes,
   bump too. The server keys ingestion off this.
4. Prefer fixed-width wire types (`u32`/`u64`, not `usize`) and reuse domain enums
   rather than restringifying them.
5. **The upload side moves with it.** `source_schema_version` is the local version,
   so a bump also changes `telemetry::upload` and the contract in
   `contracts/telemetry/upload-v1/` (schema, corpus, README) — and the Worker, whose
   accepted set widens rather than moves. Queued lines of an older version are then
   dropped, not uploaded (`telemetry::spool::project`). A new
   field is not uploaded unless the contract README's manifest adds it, which needs a
   consent-version bump; a new stage or timing field needs its `timing_ms` key there.

### Two invariants every addition MUST preserve

- **Determinism — never touch the deterministic path.** The event must not enter
  the recipe or change the output image bytes. Telemetry on vs off ⇒ byte-identical
  output (guarded by the `telemetry_does_not_perturb_the_output` test). The event is
  written *last*, once the run's outcome is fixed, and only *reads* the facts the run
  recorded. Never route a telemetry value back into a stage or the recipe.
- **No error text.** A failure event gets the error's kind and exit code, never its
  message — messages carry paths and values.
- **Fail-soft — telemetry must never change the exit code.** A telemetry
  write/serialize failure warns on stderr and is swallowed (a closed stdout pipe for
  `--telemetry-file -` is not a failure: `-v` notes it). It must NOT enter
  `report.warnings` (that would let `--strict` promote it), and it is surfaced even
  under `--quiet` (via `Log::warn_always`, used by the `warn` closure in
  `emit_telemetry`, mirroring the `non_finite` precedent). The one loud exception
  is a `--telemetry-file`/log path
  that *collides* with a real file (input, `--params` recipe, output/IR
  export/report-file) — that's a config error caught up front (exit 2), so telemetry
  can't clobber real data. A run that fails *before* that guard writes its event only
  if the sink is clear of every such file it knew of
  (`cli::telemetry_sink_collision`).

Do NOT add these flags to `ResolvedConfig`/`*Params`/`merge`/`validate`: telemetry
is **operational**, not a conversion knob (the four-coupled-spots rule does not
apply).

## 2. Collecting perf logs

Both sinks are opt-in; telemetry is collected iff at least one flag is present, and
both may be combined.

```bash
# Append one event (one line) to the persistent JSONL log.
hanten convert in.tiff -o out.tiff --film-base 0.9,0.55,0.42 --telemetry

# Also write this run's event to a one-off file (overwrites). `-` = stdout.
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
N runs append N lines — unless `hanten telemetry enable` selected this log as its
upload queue, which drains it: then keep a local history in another
`NC_TELEMETRY_LOG`. `--telemetry-file <path>` overwrites (a single event).

## 3. Reading the logs

Each line is a standalone JSON object with this shape (see `src/telemetry.rs`):

```json
{ "schema_version":11, "event_id":"5f0c3a9e81d24b7c9e0a6d3f2b1c8e47",
  "event":"conversion", "command":"convert", "timestamp_ms":1790633724299,
  "nc_version":"0.1.0", "target":"aarch64-apple-darwin", "cpu_count":11,
  "stage":"finalize",
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
  "outcome":{"status":"success","error_kind":"none","exit_code":0,
             "warnings":1,"total_samples":695772,"clipped":0,"non_finite":0} }
```

A failure has the same keys as far as the run got: `image` is absent before decode,
`conversion` before the destination resolved, `outcome.total_samples`/`clipped`/`non_finite` unless
the frame finished, and each `timing_ms` stage field until that stage completed (a failed
stage's time is only in `total`). `stage` is a `StageKind` name or `setup` /
`preflight` / `finalize` (`telemetry::EventStage`); `error_kind` is `usage`,
`decode`, `unsupported`, `write`, `resource`, `other` or `strict`. **v10**
(`telemetry/schema-v2`) introduced the event: before it, only successful runs wrote a
record, with no `event_id`/`event`/`command`/`stage` and an `outcome` of just the
three counts. **v11** (`telemetry/upload-schema`) added `outcome.total_samples`, the
denominator of the clip counts (a gain map counts both renditions).

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
effective-recipe JSON — the versioned recipe document, the exact bytes
`--dump-params` writes — so identical conversions share a hash, and it equals the
report's `identity.params_hash`. Records before v8 hashed the removed chain's recipe,
so the two never compare. The sample value above is
**illustrative**: it covers the whole recipe and changes when any key is added, removed,
or re-defaulted. Do not assert it as a constant.

`jq` recipes over the JSONL log (`jq -c` reads it line by line):

```bash
LOG="${NC_TELEMETRY_LOG:-${XDG_DATA_HOME:-$HOME/.local/share}/nc/telemetry.jsonl}"

# Failure rate, and where failures happen (v10+ lines only).
jq -s 'map(select(.schema_version >= 10)) | {runs: length, \
        failed: map(select(.outcome.status == "failure")) | length}' "$LOG"
jq -r 'select(.outcome.status == "failure") | [.stage, .outcome.error_kind] | @tsv' "$LOG" \
  | sort | uniq -c

# Per-stage timing for every run.
jq -c '{ts:.timestamp_ms} + .timing_ms' "$LOG"

# Where a run's time went: the chain's four stages together, beside the rest.
jq -c '.timing_ms | {total, decode, reconstruction, destination, encode, \
         chain: ([.scene_correction, .look, .fit_range, .fit_gamut] | map(. // 0) | add)}' "$LOG"

# Only film-master runs.
jq -c 'select(.conversion.destination == "film-master")' "$LOG"

# Megapixels vs total ms (TSV — feed a scatter / spot the slow ones). Successes
# only: a failure's total is time-to-failure, and it may have no image.
OK='select(.outcome.status != "failure")'
jq -r "$OK"' | [.image.megapixels, .timing_ms.total] | @tsv' "$LOG"

# Throughput: megapixels per second per run.
jq -r "$OK"' | [.image.megapixels, (.image.megapixels / (.timing_ms.total/1000))] | @tsv' "$LOG"

# Runs that clipped or hit a non-finite sample.
jq -c 'select(.outcome.clipped > 0 or .outcome.non_finite > 0) \
       | {ts:.timestamp_ms, destination:.conversion.destination, clipped:.outcome.clipped}' "$LOG"

# Group timing stats by nc_version (across runs).
jq -s 'map(select(.outcome.status != "failure")) | group_by(.nc_version)[] | {version: .[0].nc_version, runs: length, \
        avg_total_ms: (map(.timing_ms.total) | add/length)}' "$LOG"
```

## 4. The uploader

`hanten telemetry enable` (persistent consent) makes every `convert` append its event
to one selected queue and start a detached `upload-once` helper; `status`, `preview`,
`flush`, `disable` and `purge` manage it (docs/using-nc.md §11). The rules — consent
generations, the four leases and their order, the spool's crash-safe batches, caps —
are `docs/telemetry-strategy.md`; the lock order is on `telemetry::maintenance`.

- **Tests never touch the machine's own consent:** `tests/pipeline.rs` runs every
  binary with `NC_TELEMETRY=0`; `tests/telemetry_upload.rs` gives each test its own
  `XDG_CONFIG_HOME`/`XDG_DATA_HOME` and a loopback fake endpoint.
- **Diagnostic environment:** `NC_TELEMETRY_ENDPOINT` (`https://…`,
  `http://<loopback>…`, `file:<path>`, `none`) overrides the build's endpoint;
  `NC_TELEMETRY_HELPER=0` starts no helper, so only `flush` uploads.
- **Managed collection is silent** (no stderr) and every wait in a `convert` is
  bounded: a busy lock skips the event rather than delay the run.

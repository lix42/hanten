# Negative Converter — telemetry Progress Log

Execution log for the `telemetry` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status (the checkboxes);
this file is the narrative beside it.

One `##` section per task in this epic, named by the bare task name (the part
after the `/`). Read this whole file before starting a task in this epic, and
read other epics' `Epic summary` sections when you depend on them. Append
entries — don't rewrite earlier ones. (Condensed once, 2026-09-13: the two
finished tasks keep only what the four open ones still build on; the full
review-by-review record is in git history before that date.)

## Epic summary

What other epics need to know about `telemetry` (refreshed 2026-09-13):

- **Telemetry is operational, never a conversion knob.** `--telemetry`,
  `--telemetry-file`, and `NC_TELEMETRY_LOG` live on the CLI arg struct only —
  they are **not** recipe keys (a `telemetry` key in a recipe is rejected exit 2)
  and must never reach `ResolvedConfig`, the sidecar, or `merge`/`validate`. This
  is the documented exception to the "every knob is a flag *and* a recipe key"
  rule, alongside `--report` and `--max-memory`.
- **It must not perturb output.** The record is emitted last, after the artifacts
  and sidecar are written, and only reads their facts. Per-stage timings ride a
  report-only channel. An e2e test asserts byte-identical output **and** sidecar
  with telemetry on vs off — keep that true.
- **Fail-soft, deliberately.** A telemetry *write* failure warns on stderr (even
  under `--quiet`) and never fails the run, and is kept out of `report.warnings`
  so `--strict` can't promote it. The one exception is a `--telemetry-file` or
  log path colliding with a real artifact, which is a config error caught up
  front (exit 2).
- **The local record is `SCHEMA_VERSION` 4** (`src/telemetry.rs`), serialize-only,
  with a pinned wire-shape snapshot test — any field or ordering drift fails a
  test, which is your signal to bump the version. The v1→v4 history is in the
  `## perf-telemetry` section below and on the constant's rustdoc. It carries
  **no pixels and no file paths**; keep that invariant. *Adding* an
  `OutputPreset` member has never bumped it; whether it should is still owned by
  `output/sdr-preset-followups`.
- **Records only exist for successful converts today.** There is no `success`
  field — its existence implies success. Typed failure events arrive with
  `telemetry/schema-v2`.
- **The JSONL log is the queue to drain.** Appends are a single `write_all` to an
  `O_APPEND` handle so concurrent runs can't interleave lines.
- **`docs/telemetry-strategy.md` is the authoritative contract** for everything
  downstream: Cloudflare Worker + D1 ingestion, a separately versioned
  privacy-minimized upload projection with an exact field allowlist, persistent
  consent separate from local collection, the lease/spool/drain model, and
  sanitized function/module-only panic frames. Read it before implementing any of
  the four remaining tasks — it went through six review passes on race and
  ownership edge cases, and the four task files restate its runtime rules in
  full.
- **Explicitly rejected by the user:** persistent install identity, and uploading
  `params_hash`. Backend spend is capped at $10/month.
- **`telemetry/perf-instrumentation` is parked, not pending** — the criterion
  lab-benchmark approach was superseded by real-world telemetry and survives only
  on the remote branch `origin/prototype/perf-bench-instrumentation` (no local
  branch; `docs/prototypes/perf-bench-instrumentation.md` exists only there).


## perf-instrumentation
**Status:** parked (superseded by `perf-telemetry`)
**Updated:** 2026-07-15

- Original goal: per-stage timings in the JSON report, tracing spans to stderr
  behind `-v`, and criterion benches for the hot kernels — local-only,
  report-side (byte-identical output untouched). Pre-release performance
  visibility.
- **Parked, not merged.** On review (2026-07-15) we decided the LAB
  micro-benchmark framing answers the wrong question: we don't primarily want to
  bench kernels on a synthetic image in a controlled setting — we want to know how
  `nc` behaves **in the real world** on the user's actual scans, emit that as
  machine-readable metadata, and eventually ship it to a server. That is now
  `perf-telemetry` (below).
- The prototype is preserved on branch `prototype/perf-bench-instrumentation`
  (see its `docs/prototypes/perf-bench-instrumentation.md`). Reusable parts (the
  per-stage `Instant`-pair timing in `stages::render` + orchestrator) were lifted
  into `perf-telemetry`; the criterion benches, the lib/bin split, and the
  `tracing` spans were **not** brought over.
- No baseline bench numbers were ever recorded here (the task file says to record
  them in this log); none exist because the benches never merged.


## perf-telemetry
**Status:** done (2026-07-17; condensed 2026-09-13)
**Updated:** 2026-09-13

What shipped, and the parts the open tasks build on:

- **Flag surface (operational, NOT recipe keys):** `--telemetry` (append to the
  JSONL log) and `--telemetry-file <path>` (`-` = stdout; overwrites a one-off
  file); may be combined; collected iff at least one is present. On `ConvertArgs`
  only. Default log path: `NC_TELEMETRY_LOG`, else `$XDG_DATA_HOME/nc/telemetry.jsonl`,
  else `%APPDATA%\nc\telemetry.jsonl` (Windows), else
  `$HOME/.local/share/nc/telemetry.jsonl` — a hand-rolled resolver
  (`telemetry::resolve_log_path`, pure and unit-tested), no `directories` crate.
- **Record (serialize-only, one compact JSON object per line):** `schema_version`,
  `timestamp_ms`, run context (`nc_version`, `target` via `build.rs` → `NC_TARGET`,
  `cpu_count`), `image` (format/dims/megapixels/bit_depth/channels/ir_present/
  input_bytes/output_bytes), `timing_ms` (total + decode/film_base/algorithm/
  color/encode, `ir_export` only when it ran), `conversion` (preset,
  reconstruction, curve, `params_hash`, film_base_source, dmax when applied,
  `output_depth`), `outcome` (warnings/clipped/non_finite). `build_record` is pure:
  `timestamp_ms` and `cpu_count` are injected via `RecordInputs`.
- **Schema history** (the rustdoc on `SCHEMA_VERSION` is the canonical copy):
  - v1 (2026-07-15): the shape above with `conversion.algorithm` and
    `conversion.output_hdr`. The dropped `outcome.success` (a hardcoded `true`)
    did not bump — unreleased, no records in the wild.
  - v2: `conversion.algorithm` → `reconstruction` + optional `curve`
    (`algo/negative-reconstruction-density-curves`).
  - v3: added `conversion.preset`; `output_hdr` derived from `OutputParams::depth()`
    (`color/film-master-render-pipeline`).
  - v4 (2026-08-09): `output_hdr` (bool) → `output_depth` (`u8|u10|u16|f32`, the
    **primary image's** depth), with the `output.hdr` → `output.depth` rename
    (`output/presets`). A renamed field is a wire change; adding a preset enum
    member has never bumped, and `output/sdr-preset-followups` owns that policy.
- **`params_hash`** is `version::stable_hash` (FNV-1a) over the canonical
  effective-recipe JSON — the same function behind the report's
  `identity.params_hash`, so record and report agree; it is not the sidecar's
  bytes (the sidecar is the `{meta, params}` envelope).
- **Determinism boundary (tested):** emitted last, after output + sidecar; per-stage
  timings ride `stages::StageTimings` (algorithm + color — film-base timing is
  measured in the orchestrator since `auto-base-redesign` moved estimation out of
  `render`) plus orchestrator `Instant` pairs; never serialized into the sidecar.
  `telemetry_does_not_perturb_output_or_sidecar` pins it.
- **Fail-soft (tested):** write failure ⇒ `Log::warn_always` on stderr, exit 0,
  not in `report.warnings`. Collision of a telemetry path with input/output/
  sidecar/report-file ⇒ exit 2 up front, **including case-only collisions**
  (`keys_collide` compares ignoring ASCII case, a deliberate over-reject: the
  alternative was clobbering the just-written output on a case-insensitive FS at
  exit 0).
- **Atomic append:** one `write_all` of `line + '\n'` to an `O_APPEND` handle; the
  guarantee is `O_APPEND` offset-then-write atomicity on a local FS (not the
  `PIPE_BUF` bound, which governs pipes). `append_jsonl_is_atomic_under_concurrency`
  (8 threads × 200 appends, payloads padded past a 4 KiB page) pins it.
- **Tests worth knowing exist** (`src/telemetry.rs`, `tests/pipeline.rs`):
  `record_wire_shape_is_pinned` (exact JSON for a full and a minimal record — the
  bump signal), `telemetry_key_in_recipe_is_rejected`, outcome wiring
  (`telemetry_outcome_reports_clipping_and_warnings`,
  `telemetry_outcome_counts_ir_ignored_warning`), sigmoid/params-hash coverage,
  both sinks, fail-soft under `--strict`, collision usage error.
- **Handed to the open tasks:** the JSONL log is the queue to drain (crash-safe
  append, one object per line); upload must stay off the conversion critical path,
  honor an `NC_TELEMETRY=0`-style off switch, and key ingestion off
  `schema_version`; records carry no pixels and no paths — keep that invariant.
  The stdout report's broken-pipe panic is `core/stdout-broken-pipe-safety`, not
  this task (the `--telemetry-file -` sink is already fail-soft, but `emit_report`
  runs first and panics first).


## strategy
**Status:** done (2026-07-23; condensed 2026-09-13)
**Updated:** 2026-09-13

- Deliverable: `docs/telemetry-strategy.md` (approved by the user 2026-07-23) plus
  four children — `schema-v2`, `ingestion-service`, `upload`, `panic-hook`. No
  standalone usage-event task: feature adoption is not a selected goal, and the
  coarse algorithm/output fields ride in the conversion event.
- **Alternatives rejected, and why** (the strategy doc records the decision, this
  is the evidence): OTLP logs transport is stable and vendor-neutral but durable
  delivery still needs a Collector with persistent storage or nc-owned
  queue/ack/retry, so it does not replace the uploader design; Cloudflare
  Analytics Engine can sample at high volume and fixes retention at three months,
  both wrong for exact failure-rate analysis — hence Worker + D1 (free tier fits
  expected volume; paid Workers floor $5/month).
- **User interview (2026-07-23):** priorities are real-world performance and
  failure rates; initial backend spend capped at $10/month; persistent install
  identity and remote `params_hash` rejected; sanitized function/module-only panic
  frames selected; a detached `nc telemetry upload-once` helper accepted; upload
  consent persistent and separate from local collection, and enabling it may
  transmit the previously accumulated opted-in queue.
- **Privacy threat model flagged:** a stable `params_hash` plus precise
  timestamps, dimensions and file sizes are path-free but can still correlate or
  fingerprint workflows — which is why the upload projection buckets and omits
  them. A Rust panic hook is not general native crash capture, and raw
  payloads/backtraces can disclose paths.
- **Six review-fix passes** (all 2026-07-23) hardened the doc without changing
  status or dependencies; their outcomes are now the normative text of the four
  task files (durable raw/batch/temp recovery and fsync order; per-panic atomic
  ready files; request-by-request fail-closed consent; one consent-stored active
  JSONL plus a derived private spool; shared/exclusive request lease so disable
  waits for in-flight HTTPS; invocation-lifetime collection lease separate from
  request-lifetime network consent; non-stranding A→B retarget; lock-stable
  inactive purge; idempotent same-path enable with a helper handoff that acquires
  collection-exclusive before drain). Read the task files, not this list.


## schema-v2
**Status:** not started
**Updated:** 2026-07-23

- Goal: add typed local success/failure events and a separately versioned,
  allowlisted upload projection with random per-event deduplication IDs.


## ingestion-service
**Status:** not started
**Updated:** 2026-07-23

- Goal: implement the validating Cloudflare Worker + D1 ingestion, exact
  deduplication, retention, and initial performance/failure analysis queries.


## upload
**Status:** not started
**Updated:** 2026-07-23

- Goal: implement generation-bound invocation collection and request leases for
  a selected active JSONL/private spool, durable rotation/recovery, detached
  draining, non-stranding retarget, lock-stable inactive purge, and
  retry/quarantine maintenance.


## panic-hook
**Status:** not started
**Updated:** 2026-07-23

- Goal: capture consented Rust panics through an isolated spool with only
  sanitized `nc` function/module frames and unchanged normal panic behavior.

# Hanten — telemetry Progress Log

Execution log for the `telemetry` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status (the checkboxes);
this file is the narrative beside it.

One `##` section per task in this epic, named by the bare task name (the part
after the `/`). Read this whole file before starting a task in this epic, and
read other epics' `Epic summary` sections when you depend on them. Append
entries — don't rewrite earlier ones.

> **Consolidated 2026-09-13** (user-authorised; see CLAUDE.md's exception to the
> append-only rule). Sections of *done* tasks were rewritten as summaries keeping the
> decisions, gotchas and every measurement an open task cites; the full history is in
> git before that date. Sections of open and parked tasks are unchanged except:
> `perf-instrumentation` gained one line noting no bench numbers were ever recorded.

## Epic summary

What other epics need to know about `telemetry` (refreshed 2026-09-28):

- **Telemetry is operational, never a conversion knob.** `--telemetry`,
  `--telemetry-file`, and `NC_TELEMETRY_LOG` live on the CLI arg struct only —
  they are **not** recipe keys (a `telemetry` key in a recipe is rejected exit 2)
  and must never reach `ResolvedConfig`, the sidecar, or `merge`/`validate`. This
  is the documented exception to the "every knob is a flag *and* a recipe key"
  rule, alongside `--report` and `--max-memory`.
- **It must not perturb output.** The event is emitted last, once the run's
  outcome is fixed, and only reads facts the run recorded. Per-stage timings ride a
  report-only channel (`telemetry::StageTimer`). An e2e test asserts byte-identical
  output with telemetry on vs off — keep that true.
- **Fail-soft, deliberately.** A telemetry *write* failure warns on stderr (even
  under `--quiet`) and never fails the run, and is kept out of `report.warnings`
  so `--strict` can't promote it. The one exception is a `--telemetry-file` or
  log path colliding with a real artifact, which is a config error caught up
  front (exit 2).
- **The local event is `SCHEMA_VERSION` 11** (`src/telemetry.rs`), serialize-only,
  with a pinned wire-shape snapshot test — any field or ordering drift fails a
  test, which is your signal to bump the version. The history is on the
  constant's rustdoc. It carries **no pixels, no file paths and no error text**;
  keep that invariant.
- **Every `convert` that parses writes one event** (`telemetry/schema-v2`): a
  success, or a failure with its `stage`, `error_kind` and exit code, carrying only
  what the run reached. `StageKind` names the stages; `setup` / `preflight` /
  `finalize` the phases around them. A stage's clock closure returns `Result`, which
  is how the failed stage is known. `roll` and the other commands write none.
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
- **Explicitly rejected by the user:** persistent install identity, uploading
  `params_hash`, and (2026-09-27) uploading any legacy local record. Backend spend
  is capped at $10/month.
- **The upload contract is `contracts/telemetry/upload-v1/`** (`telemetry/upload-schema`):
  JSON Schema, a corpus Rust and the Worker both test against, and the README that is
  now the upload field manifest (the strategy's is history).
  `telemetry::upload::to_upload_event` is the only projection; the local event
  carries its own `event_id`. Bumping the local schema touches that contract too.
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
**Status:** done
**Updated:** 2026-09-28

- Goal: add typed local success/failure events with random per-event
  deduplication IDs (the upload projection moved to `upload-schema`, 2026-09-28).

### 2026-09-27 — start review: blocked, no code
- The strategy (2026-07-23) predates six local schema bumps (now v7), the
  retirement of `simple`/`sigmoid`/`characteristic`, `u8`/`u10` outputs and exit 6
  (`Resource`); its stage list is the legacy chain's, which `nf-core/default-flip`
  deletes, and `--new-flow` still refuses `--telemetry` pending
  `nf-core/report-contract`.
- **User decisions:** block this task on `nf-core/report-contract` rather than
  build the stage/timing contract on the old chain; **never upload legacy local
  records** (only the new local schema projects). Recorded in the task file's
  Decisions, the strategy's Amendments and `upload.md`; the stale manifest items
  are the task file's open questions.

### 2026-09-28 — split, and typed local events
- **User decisions:** split the task — this one ships the local events,
  [`upload-schema`](../tasks/telemetry/upload-schema.md) the projection, JSON Schema
  and corpus, with the revised upload manifest approved there; every local event
  carries its `event_id` (the uploader assigns none); clap parse failures wait for
  persistent consent in `telemetry/upload` (before parsing, `--telemetry` is
  unknown); a failed stage's time counts only toward `total`.
- **Local schema 10.** `event_id` (`getrandom`, already locked, now direct),
  `event`, `command`, `stage`, and `outcome.status` / `error_kind` / `exit_code`.
  `image`, `conversion`, `outcome.clipped` / `non_finite` and every `timing_ms`
  stage field are absent until reached. `ErrorKind` adds `resource` (exit 6) beside
  the strategy's list.
- **How the failed stage is known.** `StageClock::time` now takes a closure returning
  `Result`, so `telemetry::StageTimer` records the stage that returned `Err` and
  leaves its time out; a check *between* stages (input semantics after decode, the
  commit after encode) belongs to the stage last entered. Two call sites wrap a
  non-failure in `Ok`: `ir_separability`, and `effective_area`, whose `Err` is a
  warning, not the run's failure. The unreadable-file case fails in `preflight`:
  the memory preflight's header probe reads the file before decode does.
- **Orchestration.** `run_convert` is a thin wrapper: `convert_attempt` fills a
  `ConvertAttempt` (phase, the frame's `FrameFacts`, the resolved output, the
  conversion summary) and `emit_telemetry` builds the event from it, whatever the
  result. `convert_frame`'s `memory_out` became `FrameFacts` (preflight decision,
  decode info, stage clock), so a failed frame keeps them; `roll` reads only
  `memory`.
- **Gotcha: a failure before the write-target guard.** The guard (which covers the
  telemetry sinks) runs after recipe load and validation, so a failure event from
  there has unchecked sinks. `sinks_are_distinct` writes it only if no sink lands
  on the input or on an output the run knew of (the resolved output once known, else
  `-o` as typed); otherwise it warns and writes nothing — the input is never
  overwritten (`a_failure_event_never_lands_on_the_input`).
- **Found, not fixed:** the guard does not count `--params` (a file the run
  *reads*) as a target, so `--telemetry-file` or `--dump-params` naming the recipe
  overwrites it. Pre-existing; making it a target would also refuse
  `--dump-params X --params X`.

### 2026-09-28 — review round (`/code-review`)
- **Fixed the found-not-fixed `--params` hole**, since failure events made it easy
  to hit (a bad recipe named as `--telemetry-file` was overwritten by the event
  about it): `telemetry_sink_collision` (was `sinks_are_distinct`) keeps telemetry's sinks off the input, the
  `--params` recipe, the outputs and each other, as a usage error once targets are
  known and as a skipped event before. `--params` is not a `write_targets` entry, so
  `--dump-params X --params X` still works.
- **A flag `--export-ir` path** was unchecked before the recipe merged; the
  pre-guard check now falls back to the flag's value.
- **`StageTimer.failed` dropped:** every clock caller propagates `Err`, so the
  stage last entered is always the failed one. The frame's finish (`loss`, warning
  count, `total_ms`) is one `ConvertPhase::Finalize(FinishedFrame)` rather than three
  parallel `Option`s.
- **The decode stage has its own test:** a scan truncated after its header passes
  the memory probe and fails inside decode (`stage: "decode"`).
- **Pushed back:** skipping the attempt's bookkeeping when telemetry is off — one
  recipe hash per run, and gating it would couple the run's warning buffer to
  telemetry.

### 2026-09-28 — pre-ship review (`ship:diff-reviewer`; Codex failed to start)
- **Two more pre-guard overwrite holes, both reproduced:** a recipe's
  `input.export_ir` (it was recorded only after the destination resolved, so a
  validation failure missed it — now recorded right after `merge`), and the path
  `-o out` completes to (`out.tiff`), unknown until resolution — now any sink that
  `completes` the typed `-o` is off-limits while the output is unresolved.
- `clipped` / `non_finite` are absent unless the frame *finished*, not merely "before
  the encode": a failure in the commit after encode carries `stage: "encode"` with
  `timing_ms.encode` but no loss counts. Prose corrected.

### 2026-09-28 — done
- **Landed:** local schema 10 (`telemetry::TelemetryEvent`, `build_event`), the
  Result-aware `StageClock`, `cli::ConvertAttempt` + `emit_telemetry`, and
  `telemetry_sink_collision`. Verified by pinned full/minimal wire snapshots and
  end-to-end events for success and usage, decode (in `preflight` and in `decode`),
  unsupported, write and strict failures; telemetry on/off byte-identical; a
  telemetry write failure keeps the run's exit code; five overwrite cases.
- **For `upload-schema`:** project from `TelemetryEvent`'s wire form. `stage` never
  holds `unknown`; `error_kind` includes `resource`; a success always has
  `stage: "finalize"`. `outcome.clipped`/`non_finite` are absent unless the frame
  finished.
- **For `upload`:** parse-failure events (`EventStage::Parse`, reserved) and ID
  handling are recorded in its task file's 2026-09-28 amendment.


## upload-schema
**Status:** done
**Updated:** 2026-09-28

- Goal: the privacy-minimized upload projection of `schema-v2`'s local events, the
  upload-v1 JSON Schema and the shared valid/invalid corpus. Split from `schema-v2`
  (2026-09-28); the task file holds the user-approved manifest revision.

### 2026-09-28 — implemented
- **User decisions at start** (task file's Decisions): local schema **11** adds
  `outcome.total_samples` (the clip fraction's denominator; a gain map counts two
  renditions, so `width × height × 3` is wrong for it); `film_base_source` is
  `region`/`explicit` (`auto` retired with #195); the projection takes the typed
  `TelemetryEvent`; the contract lives in `contracts/telemetry/upload-v1/`, whose
  README is now the field manifest.
- **Landed:** `telemetry::upload` (`UploadEvent`, `to_upload_event` →
  `Result<_, NotUploadable>`: wrong local version, a non-release `nc_version`, a
  destination with no writable row, `effective_area`, or an outcome breaking the
  schema's pairing rules); the schema, 21 valid and 260 invalid requests (each
  invalid one names the Worker's answer), responses, and the local `panic-ready`
  fixture.
- **Absent vs `unknown`:** a block the run never reached is absent, as locally;
  `unknown` only for a reached value that cannot be classified. So `image.format`,
  `image.ir_present`, `non_finite` and `conversion.*` lost the strategy's `unknown`
  member.
- **Validator: `boon`, not `jsonschema`.** `jsonschema` 0.58 raised the lockfile's
  `zmij` (serde_json's float formatter, in the binary), `num-bigint` and
  `wasm-bindgen`; `boon` adds 40 dev-only crates and moves nothing locked. Gotcha:
  `cargo add` re-resolves and bumps unrelated crates — restore `Cargo.lock` and let a
  plain build add only the new ones.
- **A panic records its active stage** (`panic-hook.md` asks for it), `unknown` only
  when the hook cannot tell. A bad `event_id` is an HTTP 400: a rejection must name
  a valid one.
- `ingestion-service.md`'s "algorithm" cohort became `encoding` (the field is gone).

### 2026-09-28 — done
- **Review:** `nc-reviewer` (three rounds), `ship:diff-reviewer` and a user-run
  `/code-review`; Codex could not run (workspace out of credits). What it changed:
  the schema's `$defs/envelope` (validatable on its own; a bad `event_id` fails it),
  the projection refusing outcomes that break the pairing rules, invalid cases
  rebuilt so each fails only its named rule, `out_of_range` pinned to numeric
  bounds by a test, and the encoding looked up through `destination::resolve`.
- **Verified:** pinned full/minimal upload snapshots equal the corpus; every
  projection (each writable row, every failure kind in every stage, finished-frame
  failures) passes the schema; hostile local values never reach the wire; the
  envelope rejects exactly the `http_400` cases; e2e `total_samples` equals
  renditions × pixels × 3.
- **For `ingestion-service`:** build from the contract README, not the strategy's
  manifest — it defines the 400 cases (envelope, bad or duplicate `event_id`, body
  over 262,144 bytes) and the rejection precedence.
- **For `upload`:** `to_upload_event` takes the typed event; reading queued lines
  back, and whether older local versions still project, is yours. Split batches by
  size: 100 schema-valid panic events can exceed the body cap.
- **For `panic-hook`:** the local fixture fixes the panic event's shape (with its
  active `stage`); whether adding `EventName::Panic` bumps the local schema is
  yours to settle (`SCHEMA_VERSION`'s rustdoc).


## ingestion-service
**Status:** not started
**Updated:** 2026-09-29

- Goal: implement the validating Cloudflare Worker + D1 ingestion, exact
  deduplication, retention, and initial performance/failure analysis queries.
- 2026-09-29: marked **low priority** (user), so it is not suggested first.

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

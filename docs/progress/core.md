# Hanten — core Progress Log

Execution log for the `core` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status (the checkboxes);
this file is the narrative beside it.

One `##` section per task in this epic, named by the bare task name (the part
after the `/`). Read this whole file before starting a task in this epic, and
read other epics' `Epic summary` sections when you depend on them. Append
entries — don't rewrite earlier ones.

> **Consolidated 2026-09-13** (user-authorised; see CLAUDE.md's exception to the
> append-only rule). Sections of *done* tasks were rewritten as summaries keeping the
> decisions, gotchas and every measurement an open task cites; the full history is in
> git before that date. Sections of open and parked tasks are unchanged except: two
> cross-references appended under `value-domain-terminology` and
> `unfrozen-auto-mode-warning` moved to `conversion-versioning` /
> `recipe-replay-fidelity`, with dated pointers left.

## Epic summary

What other epics need to know about `core`:

- **The product is Hanten; the binary is `hanten`.** The crate/package stays `nc`,
  and so does every identifier — `nc-film-rgb-v1`, `nc_version`, the telemetry
  schema, recipe keys, report fields, exit codes, `NC_*`, `nctool`'s `--nc`/`$NC`,
  `../nc-assets`. **The authoritative boundary is in CLAUDE.md** ("Hanten outside,
  `nc` inside"); read it before renaming anything. Practical fallout for other
  epics: it is `cargo test --bin hanten` (not `--bin nc`), `target/debug/hanten`,
  and the stderr diagnostic prefix is now `hanten:`. Command lines in
  `docs/progress/`, `docs/reports/`, `docs/spike/` and closed task files keep the
  old spelling deliberately — they record what was *run* — but this `Epic summary`
  does not, because it is curated current guidance rather than history.

- **`cli` is the only orchestrator; stages stay pure.** Decode (stage 1) and
  encode (stage 5) are I/O — they live in `src/io/` and are driven from `cli`;
  `pipeline::stages::render` is the pure reconstruction→named-output core.
  Film-base estimation was deliberately pulled *out* of `render` into the
  orchestrator (so its warnings surface before a fallible render) — `render`
  takes an already-resolved `&FilmBase`. Every command that decodes runs the
  stage-0 memory preflight first (`io/memory-preflight`, exit 6 over budget).
- **`ResolvedConfig` is the recipe.** One nested per-stage struct doubles as the
  recipe, `--dump-params`, and `hanten params` output, so the three can't drift.
  Merge model is `defaults ← recipe ← CLI` (flags win, **by source rather than by
  value**); an absent presence flag never clobbers a recipe value. Every recipe
  struct uses `deny_unknown_fields`, so a misplaced key is a loud error — keep
  new knobs in the section design-spec §9 assigns them. `params` and `meta` are
  reserved top-level keys (the sidecar envelope), pinned by test.
- **A knob spans four coupled spots**: the CLI `*Overrides` field, the recipe
  `*Params` field, a `merge` arm, and usually a `validate` check. A forgotten
  `merge` arm makes the flag a silent no-op — add a merge test. `--preset` is the
  one conversion flag with no recipe key: it only *sets* knobs
  (`algo/conversion-presets`).
- **Validation happens at the CLI boundary; pure stages trust their inputs**
  for *config* values. `validate` reads only the resolved config and is shared
  verbatim by `convert`, `roll` and every per-frame override; `convert` must
  call **`validate_convert`**, which adds the flag-presence rules on top.
  `calibration.film_base` has **no default** — `convert`/`roll` refuse an unstated
  one (exit 2), and that rule runs **last**, being the least specific diagnosis.
  Runtime-derived values (an estimated film base, a measured anchor) are guarded
  where they're consumed.
- **The roll's measured values live in `calibration`** (`core/calibration-recipe-section`,
  2026-09-22): `calibration.film_base` and `calibration.dmax`, replacing the top-level
  `film_base` section and `reconstruction.curve.dmax`, both of which are now migration
  errors. A pipeline profile is a recipe with no `calibration`; a roll calibration is a
  recipe with nothing else. **The section is open** — a roll content white is expected to
  join it (`docs/spike/white-placement.md`), so nothing may treat it as a closed pair. The
  inclusion test is *measured from the film + fixed across the roll + consumed by a rule
  that lives elsewhere*, which is why `curve.anchor` stayed in the curve. A stated value a
  resolved look will not read (`characteristic`, `simple`) is **carried and warned about**,
  never refused — that is what lets one calibration compose with any profile.
- **Exit codes (design-spec §11):** Usage=2, Decode=3, Unsupported=4, Write=5,
  Resource=6, Other=1. `NcError::exit_code()` is the single mapping.
- **stdout is report-only**; logs and warnings go to stderr. Reports emit
  *before* any `--strict` gate, so the machine-readable record always lands and
  the signal is the exit code. Known gap: the report and `hanten params` writes still
  use `println!` and panic on a closed pipe (`core/stdout-broken-pipe-safety`).
- **lcms2 gotcha:** `transform_in_place` is infallible and Little CMS's default
  error handler silently swallows faults, so `cli` installs the *process-global*
  handler via `lcms2-sys` FFI at startup and `run_convert` checks the flag around
  the render. Don't move colour transforms somewhere that skips this.
- **`nc roll` is a separate subcommand**, not a mode of `convert`, and shares the
  per-frame core `convert_frame` — so a roll frame is **byte-identical** to the
  equivalent single `convert`. Config errors fail up front (exit 2/4); per-frame
  runtime errors are recorded and the roll continues, exiting 1. A roll whose
  recipe isn't an explicit film base warns loudly rather than failing. Roll is
  recipe-only today (`--frames`, `--out-dir`, `--params`, `--strict`,
  `--max-memory`, reporting) — `core/recipe-composition` gives it convert's
  override flags.
- **Conversion identity is stamped into every report** (`src/version.rs`,
  `core/conversion-versioning`, shipped 2026-07-28): build identity (semver +
  git commit + dirty + target), the behavioral **`pipeline_version`**, and a
  `params_hash` of the canonical resolved recipe. The label is **independent of
  semver** and bumps only when *default* conversion behavior changes.
  **`PIPELINE_VERSION` is 4** as of 2026-09-09; the history table in
  `version.rs` is the authoritative record of what each version's default render
  was (1: three post-`v0` default changes collapsed into one label; 2: nominal
  `Dmax` 1.3 + sigmoid default; 3: `gain-map-hdr` default preset; 4:
  `density.scale` `[1, 0.90, 0.86]`). The golden drift gate
  (`version::PIPELINE_FINGERPRINTS`) hashes three small, deliberately
  target-independent things — the curated per-pixel vectors in
  `pipeline::stages::golden` (stages 3–4), `film_base::estimate` over the frozen
  scan in `pipeline::film_base::golden` (stage 2), and the default recipe
  *values*. Read `version::PipelineFingerprint` for what it does NOT cover
  (decode, stage-1b semantics, the lcms2 transform, encode, non-default film-base
  sources, the IR path, real-scan geometry, and every non-default curve). **Never
  edit a historical row's `render`/`base` in place**; `recipe` is the one field
  sanctioned for an in-place refresh, and only when no default pixel moved.
- **The sidecar is `{ "meta": {…identity…}, "params": {…recipe…} }`.**
  `--params` accepts the envelope *and* a bare legacy recipe. Identity must never
  become a recipe key (`deny_unknown_fields` would reject every new sidecar), and
  identity / `output_stats` / `compare` are **operational** like `--report` and
  telemetry: no recipe keys, no `merge` arms, no effect on output bytes.
- **`pipeline_version` covers the default path only.** A recipe opting into a
  non-default curve can replay under a new build with the same label and
  different pixels; the stopgap is `cli::curve_default_warning` (via
  `unpinned_curve`), and the policy is `core/recipe-replay-fidelity`.


## product-naming

**Status:** done (2026-09-21)
**Updated:** 2026-09-21

- 2026-09-21: filed after deciding the product should be called Hanten while `nc` stays
  the internal name. The survey that motivated it: almost every `nc` in the tree is an
  identifier rather than branding — `nc-film-rgb-v1` is a *versioned* colour-space id in
  every report, `nc_version` sits in a snapshot-tested telemetry schema, and the recipe
  keys are the scripting contract. The branding surface is four files. The task carries
  the boundary until it is executed, at which point CLAUDE.md becomes its home.

- 2026-09-21: executed. The boundary now lives in CLAUDE.md ("Hanten outside, `nc`
  inside"); the task file points at it and no longer restates it.
  **The binary became `hanten`** — a user decision against the task file's own
  lean, on the grounds that nc is unreleased so the window closes at first release.
  Done with a Cargo `[[bin]]` section so the *package* stays `nc` and `NC_VERSION` /
  the `nc_version` report field never move.
- 2026-09-21: what the rename actually cost, against the task file's "small change"
  framing: **362** `nc <subcommand>` occurrences across ~100 files. 195 were rewritten
  in live docs, skills and open task files; 24 closed task files plus `docs/progress`,
  `docs/reports` and `docs/spike` keep the old spelling because they record what was
  *run*. `cargo --bin nc` was the one exception fixed everywhere — it is build
  machinery and would simply fail.
- 2026-09-21: three couplings the task file did not name, each a silent failure.
  (a) **`nctool`'s `is_nc` identifies a binary by its `--version` banner** (it was
  `startswith("nc ")`, which came from clap's `name`). Renaming clap's name alone
  would make `find_nc()` return `None` with *no error* and degrade every manifest,
  metrics and review command to exiftool. It now accepts both banners — the
  pre-rename one must stay, because the reference rendition is produced by building
  the **git-tagged** binary. Auto-discovery is deliberately `hanten`-only, so a stale
  `target/debug/nc` is never picked up silently; `TestBinaryResolution` pins both
  halves and both were checked falsifiable.
  (b) **Naming the product in `--version` would double it** — clap prints
  `{name} {version}`, so the binary name already carries it and prefixing
  `version_string()` prints `hanten Hanten 0.1.0`. Only `about` was changed.
  (c) **The stderr prefix was a hardcoded `"nc: "`** in five `eprintln!`s. It moved
  to `hanten:`; safe only because `scripts/real-scan-verify/harness.sh` greps the
  *message* ('input carries an IR plane'), never the prefix. A negative assertion in
  `tests/pipeline.rs` (`!stdout.contains("nc: decoded")`) would have gone vacuous and
  was moved with it.
- 2026-09-21: two things deliberately **not** renamed, both because the rename would
  change bytes rather than prose — recorded as boundary rows so they are not "tidied
  up" later: the `(nc)` suffix in `pipeline::color`'s synthesized coded-HDR ICC
  descriptions (written into the profile bytes of every `hdr-pq-tiff` /
  `hdr-hlg-tiff`; the only test on them asserts self-stability, so nothing would have
  caught it), and the telemetry log directory `<data-dir>/nc/telemetry.jsonl` (moving
  it orphans existing logs).
- 2026-09-21: the mechanical sweep is not sufficient, and running the binary is what
  caught it. A `\bnc (convert|inspect|…)` regex misses: `--bin nc`, paths built from
  components (`ROOT / "target" / "debug" / "nc"`, which red the harness test), a
  remedy broken across a line continuation (`` `nc \`` + `convert` `), and prose
  verbs — the default-preset suffix error still read "**nc writes** `gain-map-hdr`"
  with all six gates green. Eight more such strings were found only by grepping
  `\bnc\b` in non-comment lines and reading each.
- 2026-09-21: the repository is now **`lix42/hanten`** and private. Two task-file
  claims corrected while executing: renaming is *possible* and cheap (GitHub redirects
  the old URL permanently), and `origin` is repointed **once** — the linked worktrees
  share `main/.git/config`, they do not each need it by hand.
- 2026-09-21: `docs/using-nc.md` re-verified by running the binary per
  `update-usingnc-doc`, not by reading the diff. §2's caveat that "a bare `nc` is
  netcat" was **deleted**: the clash was caused by the old name and the rename removes
  it. Exit 2 for a missing film base, `--strict` promotion to exit 1, the
  default-preset suffix rule and the `hanten: warning:` prefix were each re-run.
- 2026-09-21: three independent reviews (Codex, `ship:diff-reviewer`, `/code-review`)
  found what the gates could not, all of it prose or examples:
  the §1 workflow **diagram in `using-nc.md` was visibly broken** — `nc` → `hanten`
  grew the box contents by 4 columns while the `┌──┐` borders stayed, so two rows
  pushed the arrows and the later boxes right; **both README quickstart examples
  exited 2** (the default `gain-map-hdr` wants `.jpg`, and `--out-depth f32` is
  refused by flag presence against an atomic preset) — pre-existing, but rewritten
  here without being run, which is the exact failure `update-usingnc-doc` exists to
  prevent; and seventeen live command lines in five files still said `nc`, because
  the sweep regex covered the *subcommands* but not `nc --version`, `nc telemetry …`
  or the `nc … | head` pipe form. Also repaired: this task's own filing commit had
  orphaned `core/unfrozen-auto-mode-warning`'s rationale body in the dependency list
  by inserting a new entry between a header and its `—` body — the trap CLAUDE.md
  documents, committed by the commit that documents it.
- 2026-09-21: `find_nc`'s **positive** half had no test — every assertion was
  negative, so a typo in a candidate path would have passed all four. Added and
  falsified. Finding it cost a round to a **stale `__pycache__`**: the falsification
  edit was a same-length transposition restored within the same mtime second, so
  CPython kept the old bytecode and produced a failure whose traceback quoted the
  *corrected* source. Recorded in CLAUDE.md.

## project-foundation
**Status:** done (2026-06-13)

- `cargo init` binary crate `nc` (edition 2024); module tree per design-spec §10
  with `todo!()` stubs so downstream tasks had a stable shape. CI
  (`.github/workflows/ci.yml`) has run `fmt --check` → `clippy --all-targets -D
  warnings` → build → test on every PR since the first commit — the gate is
  strict.
- Durable decisions: `types.rs` is the neutral contract (no crate-specific
  image/TIFF types in it — conversions live in `io/*`); `NcError::exit_code()`
  is the **one** place the §11 mapping lives; every param struct is
  `#[serde(default)]` + `Default`, so a **partial** recipe fills the rest from
  defaults; `OutDepth {U16,F32}` and `BigTiff {Auto,On,Off}` serialize lowercase.
- The `image`, `palette` and `kamadak-exif` crates were declared here on the
  design spec's word and never gained a production caller (`image` is used by one
  `#[cfg(test)]` Ultra HDR assertion); removing the unused ones is
  `core/dependency-hygiene`.


## cli-framework
**Status:** done (2026-06-18, PRs #2/#5/#6)

`cli.rs` holds the whole agent-facing surface; `cli::run()` parses and
dispatches. What later tasks build on:

- **Recipe = nested per-stage objects** (user decision), not a flat bag — the
  only layout under which `#[serde(deny_unknown_fields)]` rejects typos at every
  level (`serde(flatten)` would silently defeat it). `cli::ResolvedConfig` *is*
  that shape and doubles as the recipe, the `--dump-params` output and the
  `nc params` output.
- **Merge model:** clap arg structs use `Option<T>` per knob; `merge(cfg,
  &ConvertArgs)` is a pure fn applying `defaults ← recipe ← CLI`. **Flags beat
  the recipe by source, not by value** — an explicit `--white-balance 1,1,1`
  over a recipe's auto mode means neutral gains, not re-estimation (the existing
  white-balance precedence test is the one `core/recipe-composition` extends to
  the layered case). Orchestration consumes the returned `ResolvedConfig`; it
  never re-reads CLI args.
- **Mutually-exclusive choices are one enum field, never parallel
  `Option`/bool fields** (`FilmBaseSource`, `InputColor`, later `DmaxSource`,
  `OutDepth`): independent fields can encode illegal combinations and silently
  break the flags-win merge — the #5/#6 bugs were exactly that. Within a group
  the flags are a clap `conflicts_with` set, and `merge` maps the one present
  flag onto the enum, replacing the recipe's choice wholesale.
- **Validation at the boundary:** `validate(&ResolvedConfig)` → `NcError::Usage`
  (exit 2). Recipes bypass clap value-parsers, so `validate` is their only guard
  against smuggled bad values — test that path, not only the flag path.
- **clap error handling:** `Cli::parse()` lets clap exit directly (`--help`/
  `--version` 0, usage/value errors 2); everything else routes through
  `NcError`.
- **stdout writers** (for `core/stdout-broken-pipe-safety`): `emit_json` (the
  `println!("{json}")` branch shared by convert/inspect/estimate/roll reports)
  and `run_params` (`nc params`). `--dump-params` writes a *file*. The
  telemetry `--telemetry-file -` sink already uses a fail-soft
  `writeln!(std::io::stdout(), …)` — the in-tree pattern to reuse — but
  `emit_report` runs before telemetry, so on a closed pipe the report write
  panics first.
- `--dump-params` (for `core/profile-authoring`): it serializes the resolved
  config after a full decode → render → encode, yet its bytes are the sidecar's
  `params` body and carry nothing the image produced — the same flags over two
  different scans emit identical files. Measured 2026-08-11; that task deletes
  it.
- Design-spec §8/§9 document the nesting, the enum wire forms and the merge
  precedence; keep them in step with `types.rs`.


## pipeline-orchestration
**Status:** done (2026-07-14, PR #16 + ship). Step-1 MVP closed here.

- **`pipeline/stages.rs`** is the pure core (then stages 2–4, now 3–5a — see
  CLAUDE.md's architecture); `cli.rs` orchestrates decode → guards → render →
  lcms check → optional IR export → encode → sidecar → report → `--strict` gate.
  Output and sidecar are written **before** the strict gate because clip counts
  are only known post-encode: a `--strict` failure leaves honestly-reported
  files on disk and the exit code is the signal.
- **One flat `Report` struct serves every command**; per-command irrelevant
  fields are `None` and omitted via `skip_serializing_if`. A tagged per-command
  `ReportBody` enum was proposed in review and deferred as beyond MVP — it is
  still the shape that would make "field set for the wrong command"
  unrepresentable, if the report ever grows a second consumer.
- **Exit codes exercised end to end** (§11): Usage=2, Decode=3, Unsupported=4
  (`--export-ir` on an IR-less scan fails *before* any output), Write=5, Other=1
  (degenerate estimated base, `--strict` promotion, lcms fault).
- **lcms2 global handler** (CLAUDE.md records the mechanism). It latches on
  *any* lcms log because it cannot see severity, so a benign recoverable
  ICC-parse warning during a custom `--output-profile` fails the run loudly —
  kept as the fail-safe posture; refine by inspecting error codes if it bites.
- **IR notice gating:** the "IR present but not consumed" report warning is
  emitted only when `--export-ir` is absent, so `--strict --export-ir` is a
  usable workflow on HDRi input.
- **Verbosity:** `-v` enables stderr progress lines, `--quiet` silences them;
  warnings always land in the report, and a non-finite fault is echoed to stderr
  even under `--quiet`.
- **Data-loss guards (PR #16):** every write target (`--output`, sidecar,
  `--dump-params`, `--report-file`, `--export-ir`) is checked against the input
  and against each other before anything is decoded — previously `-o <input>`
  destroyed the negative at exit 0. `--input-profile` / `input.color.profile`
  was a parsed-but-never-applied no-op and is rejected with exit 4 until
  input-side colour management exists.
- Real-scan verification used the committed 502×462 fixtures
  (`tests/fixtures/{hdr-48bit,hdri-64bit}.tif`); `../nc-assets` came later
  (`analysis/real-scan-verification`).
- **For `core/value-domain-terminology`:** the task file's "`--out-depth f32`"
  example, once obsolete, is current again — `output/presets` replaced
  `--output-hdr`/`--output-sdr` with `--out-depth u16|f32` on 2026-08-09.
- The "unify the two `Algorithm` enums" follow-up recorded here is **moot**:
  `algo::Algorithm`, `AlgoParams` and the `Converter` trait were all removed by
  `algo/negative-reconstruction-density-curves` (tagged `reconstruction`
  object). `core/dependency-hygiene` keeps only its crate-removal half.


## roll-conversion
**Status:** done (2026-07-21, PRs #36/#37 era)

`nc roll` replays one provided frozen recipe over N frames — the **apply** half
of plan → recipe → apply. The **plan** half is `core/base-acquisition-planner`.

- **Surface:** positional inputs (files / directories / shell globs; sorted and
  deduped, directories expand to their sorted `.tif`/`.tiff`) **or** `--frames
  <manifest.json>`; required `-o/--out-dir`; `--params`; `--strict`;
  `--max-memory`; `ReportArgs`. No new recipe keys. `input.export_ir` is
  rejected in roll mode (one path, N frames).
- **`convert_frame`** is the shared per-frame core (decode → film-base → render
  → IR export → encode + sidecar), so a roll frame is byte-identical to a single
  `convert` — pinned by a byte-diff test. Report emission, the `--strict` gate
  and telemetry stay in the orchestrator (telemetry is `convert`-only).
- **Roll report:** `command:"roll"`, the shared `recipe` once, `frames[]` of
  `FrameStatus` (`Ok { film_base, dmax, white_balance, balance_range, loss,
  output_stats, identity, … }` / `Failed { error }`, internally tagged on
  `status`, warnings/overrides common), `summary { total, succeeded, failed }`,
  and roll-level `warnings`. Each frame echoes its *resolved* base/`Dmax`.
- **Per-frame overrides — the planner hand-off contract** (for
  `core/base-acquisition-planner` and `core/recipe-composition`): a manifest
  entry is `{ input, output?, params? }`; `params` is a *partial* recipe
  deep-merged (`merge_json`) onto the shared recipe's JSON and re-deserialized
  under `deny_unknown_fields`. **`merge_json` replaces an externally-tagged enum
  variant switch instead of unioning tags** (`is_variant_switch`: both sides
  single-key objects with different keys ⇒ wholesale replace; same tag ⇒ deep
  merge) — the answer `core/recipe-composition`'s open question 1 should reuse.
- **Roll-fixed invariant warns, never fails** (user course correction): a
  shared recipe whose `film_base.source` is not `explicit`, or a per-frame
  override that sets `film_base`, converts with a loud `--strict`-promotable
  roll-level warning. `density.dmax` overrides are allowed silently — the gap
  `core/unfrozen-auto-mode-warning` closes (auto `dmax`, auto WB, auto balance
  range re-measure per frame with no warning today).
- **Config vs runtime errors:** a bad shared recipe or override fails up front
  (exit 2/4) before any frame converts; a per-frame runtime error is recorded
  and the roll continues, exiting 1 after the report. Memory preflight runs per
  frame and follows the same per-frame handling.
- **Naming:** `<out-dir>/<stem>_positive.<ext>` with the suffix derived from the
  frame's resolved preset (`output/presets`, 2026-08-09); an explicit manifest
  `output` goes through `reject_suffix_mismatch`.
- **Safety:** all outputs, sidecars, `--report-file` and the `--frames` manifest
  are collision-checked up front (`ensure_roll_targets_distinct`); an unreadable
  directory entry is a loud usage error, never a silently short batch.
- **Sequential, not parallel** — per-frame output is independent, so
  `rayon`-parallelizing the loop is a safe future optimization left out to keep
  the scaffold lean.
- Design-spec §12 item 6/13 record the roll workflow; the recipe-workflow
  redesign (`nc calibrate`, `nc profile`, layered `--params`) is §8's target
  subsection, decided 2026-08-11 (#94).


## base-acquisition-planner

**Status:** not started
**Updated:** —

- Goal: Implement the automatic **acquisition cascade** that resolves a roll's `Dmin` and `Dmax` from whatever the user provides, emits a **frozen recipe with provenance + confidence**, and decides when to fall back from roll to single conversion.


## conversion-versioning

**Status:** done (2026-07-28, PR #60; four review rounds, ~30 findings fixed)
**Updated:** 2026-09-13 (consolidated)

Stamps every conversion with a machine-readable identity and a behavioral
`pipeline_version`, gated against silent drift, plus a two-build comparison
harness. Output pixels were byte-identical to the pre-work binary across 10
conversions; the default `params_hash` was `3575c9feb5d42b2b` at the time.

### The three identity layers (`src/version.rs`)

1. **Build identity** — `NC_VERSION`, `git_commit`, `git_dirty`, `TARGET`, from
   `build.rs` (`NC_GIT_COMMIT` / `NC_GIT_DIRTY` beside the existing `NC_TARGET`).
   `git_commit`/`git_dirty` are **omitted, not `"unknown"`**, when the tree has
   no usable git; `git_dirty` may be unknown while the commit is known, never the
   reverse (`nc --version` prints `commit: <hash> (dirty unknown)` then).
2. **`PIPELINE_VERSION`** — the behavioral label, independent of semver. Started
   at **1, not 0** (the task file said 0): `film-base/dmax-reference`,
   `film-base/auto-base-redesign` and `io/input-data-semantics` had already
   moved the default render after `v0-baseline.md`, so v1 **collapses three
   default changes into one label**. `0` predates the constant and cannot be
   fingerprinted from the tree. The history table in `version.rs` is the record
   of every version since; `PIPELINE_BEHAVIOR` is paired to the current row by a
   gate (the row must carry it, and no two rows may share a behavior string).
3. **`params_hash`** — `version::stable_hash` (hand-rolled FNV-1a, since
   `DefaultHasher` is not stable across toolchains) over the canonical
   resolved-recipe JSON. `telemetry::params_hash` delegates to it: one hasher,
   homed in `version` so the core report never depends on the opt-in telemetry
   module. The only other FNV site is `fnv1a_hex` in `tests/pipeline.rs`, a
   deliberate independent pin from outside the crate.

`inspect` and `estimate` stamp identity too (`params_hash` **omitted**, not
null); roll frames each carry `identity` + `output_stats` inside `FrameStatus::Ok`.

### The sidecar envelope

`{ "meta": {…identity…}, "params": {…recipe…} }`. `load_recipe` accepts both the
envelope and a bare legacy recipe, told apart by the top-level `params` key —
which is why **`params` (and `meta`) can never become recipe keys**
(`params_and_meta_are_not_recipe_keys`). Rules that were each a shipped bug once:

- `meta` is a raw `Value` (an older build must tolerate a newer build's fields),
  but when present it must be an **object**, and `params` must be an object —
  `{"params": []}` once converted with all defaults at exit 0 and the default
  hash; a non-object `meta` once silently disabled the skew check.
- `meta.pipeline_version` is read with `u32::try_from`; present-but-unreadable
  (`1.0`, `"1"`, `-1`, `null`, `2^32+1`) is a loud exit 2, never "absent".
- **Replaying a recipe from another `pipeline_version`** warns
  (`pipeline_version_warning`, `--strict`-promotable) on `convert` and as a
  roll-level warning on `roll`. On `convert` the warning reaches only stderr if
  the frame then fails (the report is never emitted); `roll` keeps it.
- `canonical_params_json` is the single producer of the sidecar body,
  `--dump-params` and the hash input. The hash is reproducible from a
  `--dump-params` **file** (the sidecar body is the same document indented two
  spaces deeper; `nc params` adds a trailing newline).

### The golden drift gate (`PIPELINE_FINGERPRINTS` + `mod drift_gate`)

A whole-file or whole-frame checksum passes locally and fails CI (libm ~1 ULP,
lcms2/ICC bytes per target), so the gate hashes three small target-independent
things keyed by version:

- `render` — `reconstruct_and_print` over the curated `stages::golden` vectors
  under the default reconstruction + print params; stops **before** lcms2.
- `base` — `film_base::estimate` for the default (`auto`) source over the frozen
  synthetic scan `film_base::golden::scan()`. Added in review: the first gate
  handed `render` a hardcoded base and never touched stage 2, though
  `auto-base-redesign` had landed in exactly that stage one day after the v0
  baseline. Safe cross-platform because `film_base` has no transcendental. The
  frozen band carries a 7% along-edge ripple so retuning `SAMPLE_PERCENTILE`
  moves the hash (a flat band returns the same value at any percentile).
- `recipe` — canonical JSON of `ResolvedConfig::default()`; catches default
  changes outside stages 2–4 (`output.depth`, `output.preset`, input defaults).

Rules the failure messages state:

- **A bump with no row panics** — "bumping passes" (task file) is deliberately
  not what happens; the message prints the row to paste.
- **`render`/`base` of a recorded row are history — never edit in place.** One
  label for two behaviors makes every stamped output unattributable.
- **`recipe` is the one field sanctioned for an in-place refresh**, when only an
  opt-in knob with a neutral default was added and no default pixel moved. Note
  it here each time (the `version.rs` message asks for that). Refreshes so far:
  2026-07-27 (`print.linear_range`, `output.preset: legacy` from #59,
  `e1bd4fb5cb789ded` → `8a5b874faa30d391`); v1 when `film_base.source` lost its
  default (a contract change, not a render change — bumping would have told
  archived-sidecar users their output would differ when it matches exactly);
  **2026-09-01** (`print.display_tone` from `output/linear-render`,
  `5b22d0505ed4fb79` → `a26e8ec6434e8ebc`, `render`/`base` byte-identical, so
  `PIPELINE_VERSION` stayed 3).
- The detection test perturbs one default per fingerprint (`print_exposure +
  ε`, the reconstruction curve, an explicit base) and re-asserts the unperturbed
  defaults match the row — the first version compared two differently-shaped
  strings and could not fail.

**What the gate does not cover** (`PipelineFingerprint` doc, design-spec §9):
stage 1 decode, stage 1b `input_semantics`, the lcms2 transform and ICC bytes,
`io::encode`, the `Region`/`Explicit` film-base sources, the IR path
(`golden::scan()` has no IR plane), real rebate geometry, and every non-default
curve. `scripts/real-scan-verify/` and `nctool compare` are the other half.

**For `algo/split-default-migration`:** a fingerprint row hashes raw f32 bits
with no ULP window, so the new default's `render` vector must be chosen for
cross-target bit identity — read that task's portability note.

### Comparison harness (`scripts/analysis/benchmark.json`, `nctool compare`)

- `io::encode` returns `EncodeOutcome { loss, stats }`; `OutputStats { mean }`
  is the per-channel mean of the samples as written (u16 accumulated in integer
  `u64`; f32 over finite samples only). It is a **second** pass, paid even under
  `--report none`. Mean ΔRGB between builds is the difference of two means, so
  `compare` never re-reads pixels.
- `compare run` converts a set (`fixtures`: the committed 502×462 decoder
  fixtures, runnable anywhere; `rolls`: six real Ektar/Phoenix frames resolved
  through `manifest.json` with sha256 verification) and writes a run record;
  `compare diff` keys on both identities. Timings come from the telemetry record.
- **Exit codes invert the `manifest` convention deliberately:** 0 = verdict
  delivered (identical or not), 1 = comparison failed or the pipeline is
  provably non-deterministic, 2 = usage/malformed record.
- **The determinism accusation is precondition-guarded**
  (`determinism_blockers()`): it fires only when the same clean commit, target
  and `pipeline_version`, identical input digests, identical `params_hash`,
  identical output depth and the same frame set have all been ruled out;
  otherwise `identical: false` at rc 0 with a `determinism_check_blocked: …`
  note. Round 1 had guarded one route (dirty identity) and called ordinary
  iteration a broken contract. **Adding a record field that can independently
  change the numbers means adding it to `determinism_blockers()`.**
- `validate_record` refuses hollow input field by field (a diff report re-diffed
  once returned `identical: true`), duplicate frame/case names, non-finite or
  non-numeric measurements, a missing `output_hdr` marker (u16 and f32 means are
  different units), and a record with no usable identity.
- Every frame carries `input_sha256` + `checksums: verified|computed|skipped`;
  `diff` refuses (exit 2) when digests differ. `checksums_skipped: false` is an
  affirmative claim and must be substantiated by a digest.
- **Two bug shapes swept, worth re-checking when the record schema grows:**
  *(A)* a validated leaf inside an unvalidated container — 8 further sites in
  `compare.py`, three on the path that reads an `nc` report (`(x.get("k") or
  {})` rescues only *falsy* non-dicts); *(B)* absent evidence rendering as an
  affirmative negative — exactly one instance, every other boolean's polarity
  audited.
- **Not covered by the fixed set: the product default.** `benchmark.json`'s
  fixture cases pin `--output-preset legacy` to stay comparable with pre-flip
  records, so no cross-build comparison sees the container users actually get
  (recorded 2026-08-09 in `output/sdr-preset-followups`; changing the fixed set
  is this task's successor's call).

### `build.rs` gotchas (all verified in throwaway repos)

- `git rev-parse` walks up; `is_nc_repository` requires `--show-toplevel` to be
  the package dir, else identity degrades to unknown (a future workspace layout
  would trip this too).
- A linked worktree's `.git` is a **file**, so `rerun-if-changed=.git/HEAD` names
  nothing; paths come from `rev-parse --git-path`. Watching `HEAD` alone never
  notices a commit on a branch (it is a `ref:` pointer), so `refs`, `packed-refs`
  and `index` are watched, and the source dirs are named explicitly rather than
  the package root (whose scan would sweep `target/`). Residual: a new untracked
  file outside the named dirs can leave `git_dirty` stale until the next watched
  change; `git status --porcelain` counts untracked files, so a stray scratch
  file pins `dirty: true`.

### Deferred — recorded, not implemented

- `DETERMINISTIC` is a hardcoded allowlist; the record schema is otherwise
  unpinned.
- `identical: true` is a signed per-channel mean — blind to a permutation or an
  exactly-compensating change. The docstring says so; the name is unchanged.
- `params_hash` identifies the **requested** config, not the resolved values
  (`auto` base, `percentile` WB); `--output-profile <path>` puts a machine-local
  path into it.
- `tests/pipeline.rs::report_carries_every_identity_layer` fails from a source
  tarball (the build degrades; the test does not).
- ΔE2000/SSIM stay design-spec §12 item 7; timings are excluded from the
  `identical` verdict (±1.4 s between runs of one build).
- `docs/reports/v0-baseline.md` was left as the historical record; a `v1`
  baseline measured with `nctool compare` was never written (v2/v3 have
  `render-defaults-v2.md` / `-v3.md`).
- The "Python suite runs in no CI gate" recommendation made here was **closed**
  by `analysis/harness-regression-tests`: CI now runs the `nctool` suite on
  Linux and macOS.
- The claim that `pipeline_version` was still absent from the `film-master`
  report (a `color/film-master-render-pipeline` carve-out) is unverified today —
  no test by that description was found on 2026-09-13; check before acting on it.

### 2026-08-04 — the contract has a known hole: `core/recipe-replay-fidelity`

`algo/reference-anchored-sigmoid` (2026-08-03) moved three sigmoid defaults
(`contrast` 1.0 → ≈2.0687, `shoulder` 0.2 → 0.6, a new `curve.anchor`) while the
default curve was still `exponential`, so `PIPELINE_VERSION` correctly did not
move and the gate correctly did not fail — yet every recipe selecting `sigmoid`
renders differently, because omitted keys take the new defaults. Filed as a
sibling rather than by reopening this task (reopening would have made
`output/presets` non-executable). The stopgap is `cli::curve_default_warning`
over `unpinned_curve` (originally `sigmoid_anchor_default_warning`; widened on
2026-09-09 to report an unstated `density.scale` after that default moved with
`pipeline_version` 4 and a bare `--params` recipe carried no label to warn on).
Two remedies were rejected and should not be re-derived: bumping
`reconstruction.schema_version` (versions *shape*, checked for exact equality, so
it rejects every archived recipe), and per-schema-version historical default
tables (a policy decision, which the new task owns).


## recipe-replay-fidelity

**Status:** not started
**Updated:** — (section added 2026-09-13 to hold the relocated cross-reference)

- Goal: decide and implement what `nc` owes a frozen recipe whose render
  changed because a non-default path's defaults moved. Filed 2026-08-04 out of
  `conversion-versioning` (see that section's closing entry for the first
  instance, the stopgap, and the two rejected remedies); the second instance
  (`density.scale`, `pipeline_version` 4, 2026-09-09) is recorded in the task
  file.


## dependency-hygiene

**Status:** not started
**Updated:** —

- Goal: Remove dead weight surfaced by the dependency/module-hygiene review (see [`_unassigned.md`](_unassigned.md)): three declared-but-unused crates and a duplicate algorithm selector enum.


## release-readiness

**Status:** not started
**Updated:** —

- Goal: Get `nc` ready for a public release: correct the public documentation that currently misstates the product, choose a license, add crate/release metadata, define supported platforms, and package binaries.


## stdout-broken-pipe-safety

**Status:** not started
**Updated:** —

- Goal: Make every stdout write in `nc` tolerate a **closed pipe** without a panic or backtrace, exiting cleanly instead.


## value-domain-terminology

**Status:** not started
**Updated:** —

- Goal: Make nc's value-domain terminology — especially `Dmin`/`Dmax` — easy to understand, use, and maintain for **both people and agents**.

### 2026-09-13 — relocated

The 2026-08-04 `conversion-versioning` cross-reference that had been appended under this
heading now lives in `## conversion-versioning` (and `## recipe-replay-fidelity`).

## recipe-composition

**Status:** not started
**Updated:** 2026-08-11

- Goal: `--params` repeatable (file or `-`), `roll` gains convert's override flags,
  one precedence chain. Enables the pipeline/calibration split.
- **No schema change needed** — verified 2026-08-11 that both halves already parse:
  a recipe with only `reconstruction`/`print`/`output` works when the base comes
  from a flag, and one with only `film_base` + `dmax` works with everything else
  defaulted. The single missing mechanic is that `--params` rejects repetition.
- Precedence extends the existing rule rather than replacing it: flags already beat
  the recipe **by source, not value**, and layering adds ordering among recipes.

## profile-authoring

**Status:** not started
**Updated:** 2026-08-11

- Goal: `nc params` → `nc profile`; takes overrides, validates config-only, writes
  annotated JSONC with `--out`, no image. Deletes `--dump-params`.
- Why `--dump-params` goes, measured 2026-08-11: its output is byte-identical to
  the sidecar every conversion already writes, and it carries nothing the image
  produced — the same flags over two *different* scans emit identical files. It
  records modes (`"auto"`, `"percentile"`), never the measurements the report holds
  beside it. So "freeze a recipe" ran a full decode/render/encode to echo the
  flags just typed, and the result still re-measured per frame.
- JSONC because it is a **superset**: existing recipes, sidecars and `--params`
  files stay valid, the schema's tagged enums keep working, and the machine
  contracts (stdout report, sidecar) stay plain JSON. Comments are generated from
  the schema and **not preserved** across a round trip, so nc must never rewrite a
  user's file in place.

## unfrozen-auto-mode-warning

**Status:** not started
**Updated:** 2026-08-11

- Goal: warn when a recipe applied to a roll still re-measures per frame.
- The gap, one run with `--base-region … --auto-wb percentile --auto-d-max`: the
  report held `film_base {0.163, 0.080, 0.038}`, `dmax 0.581`, `wb [1.228, 1.0,
  0.721]` while the recipe held `{"region": …}`, `"auto"`, `"percentile"`. Applying
  it to a roll re-derived all three per frame, with no warning beyond an incidental
  region-uniformity note.
- Precedent exists: roll already warns when the film base is not `explicit`.

### 2026-09-13 — relocated

The 2026-09-01 `conversion-versioning` cross-reference that had been appended under this
heading now lives in `## conversion-versioning` (and `## recipe-replay-fidelity`).

## calibration-recipe-section

**Status:** done (2026-09-22)
**Updated:** 2026-09-22

- Goal: a top-level `calibration` recipe section holding `film_base` and `dmax`;
  `dmax` leaves `reconstruction.curve`. Schema change with a migration error, no pixel
  change, `recipe` fingerprint refreshed in place.
- Filed 2026-09-13 because three workflow tasks (planner, profile authoring, layered
  composition) assumed the section and none owned it; absorbs
  `value-domain-terminology`'s item 3.

### 2026-09-22 — executed

**Status:** done. The recipe's sixth section is now `calibration`, holding
`film_base` and `dmax`; the top-level `film_base` section and
`reconstruction.curve.dmax` are gone, each rejected with a migration error naming the
new path. No pixel moved.

- **The section is designed open, not as a closed pair** (user decision, 2026-09-22,
  relayed through the main session). A third measured roll value is expected — the
  roll's **content white** (red p97 toward the roll's upper end,
  `docs/spike/white-placement.md`), which `nf-reconstruction/anchor-rule` needs if it
  adopts candidate B/C/D, and possibly a fourth for its per-frame spread (within-roll
  spread of per-frame p97 is 0.40, which is why the spread cannot be derived). So:
  per-key optionality rather than a section-level default, no closed-pair constructs,
  and a report fragment built key-by-key. The inclusion test written into
  `CalibrationParams`' rustdoc is **measured from the film + fixed across the roll +
  consumed by a rule that lives elsewhere** — which is exactly why `curve.anchor`
  stayed in the curve (the *rule* is a look; only the measurement moved), and why a
  future holder-polarity knob does not qualify (design-spec §14 item 9 retargeted from
  `film_base.holder` to `measure.holder`).
- **`calibration.film_base` is the bare `FilmBaseSource`** — the `{"source": …}`
  wrapper went with the section, per design-spec §8's own example. It keeps no default,
  so `validate`'s "you have not chosen a base" rule is unchanged and still runs **last**.
- **Four decisions taken while implementing**, all confirmed with the user first:
  1. Flatten the wrapper (above).
  2. **`--d-max` beside a curve that reads no reference is accepted, not refused.** The
     old flag-presence rejection for `characteristic` is deleted, and the four
     `--*d-max` flags left `active_density_domain_flag` so `simple` no longer refuses
     them either. Under the project's presence-rule tiebreaker they force nothing a
     branch cannot produce — the value is a roll measurement every branch can hold —
     and refusing them would break the composition the section exists for
     (`--params roll-cal.json --params characteristic-look.json`). Not silently
     ignored: `cli::unconsumed_dmax_warning` names the branch and is
     `--strict`-promotable, and the report still says `dmax.policy = "none"` for a
     curve that read nothing.
  3. **The report's `calibration` key is the section *body***, so the pipe is
     `jq '{calibration}'`, not `jq .calibration`; design-spec §8's target block was
     corrected to match. It replaces `film_base_recipe` and `d_max_recipe`; the two
     `*_flag` keys stay, since the flags did not move.
  4. **The structured `dmax` resolution stays under `reconstruction_result.curve`.**
     That block records what the *render* resolved and is paired with `anchor_value`;
     the recipe path it came from is `dmax.provenance`'s job.
- **The `recipe` fingerprint was refreshed in place on the v5 row, with no
  `pipeline_version` bump** — the case the task file sanctioned in advance. The default
  document changed *shape* while every default **value** stayed put (`film_base` still
  `null`, the reference still `"fixed"`), so `render` and `base` did not move; the drift
  gate asserts those two before `recipe`, and they held. This is not the version-identity
  worry recorded on that row for `--auto-d-max`: a recipe written against the old shape
  does not render differently, it is *rejected* with a migration error.
- **Byte-identity verified by same-machine before/after**, `2664a0d` against this branch,
  on `tests/fixtures/hdr-48bit.tif`: `legacy`, `film-master`, `gain-map-hdr`,
  `display-p3`, `hdr-pq`, plus `characteristic`, `--auto-d-max` and `simple` — eight
  configurations, all `cmp`-identical.
- **What the move deleted, and why that is the point.** `cli::preset_curve`,
  `DensityCurve::{dmax, dmax_mut}`, `DensityCurveType::takes_dmax` and
  `internally_tagged_switch`'s carry are all gone. Each existed to move the roll's
  reference across an object that was being replaced, and **none of them could cross a
  `characteristic` boundary**: gating the carry on the target (the fix for "the merge
  inserted a key the deserializer then rejected") silently *dropped* the calibration
  instead. A frame switched to a stock curve and back lost it, at exit 0. With the value
  outside the switched object there is nothing to carry and nothing to drop —
  `a_curve_switch_never_touches_the_roll_reference` pins the round trip the carry could
  never serve.
- **Two predicates needed a second condition, not just a new path.**
  `measures_over_region` now asks `calibration.dmax == Auto` **and**
  `curve.consumes_reference()`: a `characteristic` render never calls `resolve_dmax`, so
  the source alone would have resolved a measurement region for a run that measures
  nothing and reported an `effective_area` for it. `region_reaches_a_rendered_pixel` is
  written as "measured, *and* the placement reads it", so a condition added to the first
  cannot be forgotten in the second.
- **Threading.** `DmaxSource` reaches the stage as `types::DmaxInput { source, region }`.
  The region was already threaded through six `stages::` functions *only* for
  `resolve_dmax`, so pairing them keeps arity flat and makes "a region beside a non-`Auto`
  source" unrepresentable. The two calibration members arrive differently on purpose:
  the base is resolved *before* the stage (`film_base::estimate`), while `Auto` here needs
  the post-regional-balance densities and so cannot be.
- **Outside `src/`:** 18 `scripts/real-scan-verify/recipes/*.json` rewritten (and the
  `harness.sh` `jq` that generates them), `nctool roll`'s `_freeze_recipe`,
  `scripts/render-defaults-v3/measure.py` and `algo::curve_probe::frozen_base` — the last
  two read a frozen recipe by path and would otherwise have rotted silently, the probe
  behind `#[ignore]` where no gate looks.
- **Code review found seven real defects, five of them prose-versus-code** — the class
  no gate reads. Two were behavioural and worth recording:
  - **`unpinned_curve`'s floating-reference term had to go, not just change path.**
    Repointing `dmax_floats` at `calibration.dmax` made *every* pipeline profile warn —
    a look has no `calibration` section by definition — and fail under `--strict`, so
    the guide's own §4 shape contradicted the binary. It also warned on a calibration
    that measured only a base, which is exactly what `estimate` emits without
    `--d-max-region`. The term is now deleted: the population it served (recipes
    spelling `reconstruction.curve.dmax`) is rejected outright by the migration error,
    the more specific diagnosis, and post-move an absent reference is a legitimate
    composition rather than evidence of an older build. The three warning messages also
    told the user to pin `dmax` *inside the curve*, which now exits 2 — a remedy that
    does not work, the rule this project has broken five times.
  - **`unconsumed_dmax_warning`'s `simple` branch advised `--density-curve`, which
    `merge` refuses beside `simple`.** The remedy is per branch now, and the test
    asserts both that the working flag is named and that the refused one is absent.
  - The other five: a probe rendering its benchmark at `DmaxInput::default()` instead
    of its own stated reference (fixed by *deleting* `benchmark_sigmoid`'s vestigial
    `_dmax` parameter, so a site that forgets the reference cannot compile silently);
    two comments claiming a `validate` rule that D2 had just removed; and two rustdoc
    clusters still naming the moved paths. The sweep CLAUDE.md prescribes —
    `grep -rn 'curve\.dmax\|film_base\.source\|density\.dmax' src/` — found the rest
    in one pass, and caught one *pre-existing* false claim while there
    (`film_base::golden` said the base source "defaults to Auto"; it has no default).
- **Rebased again onto `9dbc8bb` (`nf-core/knob-availability-audit`, #139), and its model
  replaced the point fix below.** That commit generalised the new-flow refusals into a
  section inventory: `UNREAD_RECIPE_SECTIONS` refused whole, `READ_RECIPE_SECTIONS` read,
  and — decisively — a documented pattern for a section read only *in part*
  (`input` minus `export_ir`, refused by a `VALUE_ENTRIES` row rather than by widening
  the list). `calibration` is exactly that shape: the fixed decode divides by
  `film_base`, and reads no `dmax` at all. So `reject_recipe_calibration_dmax` was
  deleted and the key became a value row, per CLAUDE.md's rule to take *their* design
  rather than resolve line-by-line. **It closes strictly more:** a value rule is what
  `roll` reaches, so a per-frame override stating `calibration.dmax` is now refused too
  — the gap the ship review could only record as unfixed. One more of #139's test
  recipes spelled the old `film_base` shape; auto-merge was silent about that as before.
- **Rebased onto `f169fac`, and the rebase opened a hole a clean auto-merge hid.**
  `nf-core/fixed-decode` (#137) landed `flow::reject_recipe_reconstruction`, which
  refuses a recipe `reconstruction` section under `--new-flow` because the fixed decode
  reads its own `DecodeParams` and would parse that section and never read it. The
  reference density *was* inside that section, so a recipe stating one was already
  covered. Moving it to `calibration.dmax` put it out of that witness's reach — leaving
  `--d-max` refused by flag while the recipe key saying the same thing was accepted and
  ignored, the exact class the table exists to prevent. Closed with
  `flow::reject_recipe_calibration_dmax`, sharing `DMAX_REASON` so the two cannot drift
  into different stories. **`calibration.film_base` stays accepted** — the fixed decode
  still divides by the base — and that asymmetry is the assertion the new test makes.
  Also ported: three of #137's own test recipes arrived spelling the old `film_base`
  shape (auto-merge is silent about that), and `algo::fixed`'s `equivalent_legacy`
  helper took `dmax` only to bury it in the curve.
- **A second review (ship's diff-reviewer) found six more, two of them mutation-proved.**
  The one worth carrying forward: it replaced `measures_over_region`'s new
  `&& curve.consumes_reference()` with `true` and the whole suite — 823 + 223 — still
  passed, so the condition the commit message singles out could have been deleted by a
  later refactor with every gate green. The predicate test now covers
  `--density-curve characteristic --auto-d-max`, checked falsifiable. Also: the
  `base_level` probe in `shadow_metrics` had lost its per-candidate anchor to
  `DmaxInput::default()` (the nominal), so candidates differing only in anchor printed
  the same number behind `#[ignore]`; and `unconsumed_dmax_warning`'s characteristic
  remedy exited 2 whenever `--film-stock` was on the line — the same circular-advice
  class as the `simple` arm fixed one round earlier, which is now five instances, so
  the remedy names the stock too. Two comments stated inverted reasons and were
  rewritten to the honest trade: dropping `unpinned_curve`'s floating-reference term
  leaves a pre-move recipe silent on the reference unwarned (the migration error catches
  the key *present*, the term fired on it *absent* — disjoint populations), and that
  residual gap is `core/conversion-versioning`'s, beside the `"fixed"` one.
- `docs/using-nc.md` re-verified by running the binary (§2 estimate output, §3 the
  `--d-max-region` example, §4 the workflow recipe, §5 the default document and the
  "minimal recipe is byte-identical to the flag form" claim — re-run and still
  byte-identical, plus the new migration-error text). Header pin refreshed.

//! Embedded, opt-in performance + context telemetry for `hanten convert`.
//!
//! When a `convert` that got past argument parsing ends, the orchestrator gathers
//! one event — success or failure, with the stage it ended in, image facts,
//! per-stage timings and a compact conversion summary, each only as far as the run
//! got — and emits it as JSON to a persistent append-only JSONL log and/or a
//! one-off file (design-spec §8/§9). The JSONL log is the queue a future uploader
//! (`telemetry/upload`) drains; this module only *produces* the event and writes
//! the local sink(s).
//!
//! Two deliberate design boundaries:
//!
//! - **Determinism (critical):** the event (timings, timestamp, system info)
//!   never enters the recipe and never changes the image bytes. Telemetry on or
//!   off, the output is byte-identical; this module only reads facts the run
//!   recorded.
//! - **Fail-soft (a documented deviation from the house fail-loudly rule):** a
//!   telemetry failure never changes the run's exit code. Telemetry is
//!   non-critical observability, so the orchestrator warns on stderr and
//!   continues (`--strict` does not promote it). A telemetry-file that *collides* with a real output is the
//!   exception — that is a config error caught up front by the CLI, not a runtime
//!   write failure, and stays a loud usage error.
//!
//! The builder ([`build_event`]) is a pure function of its inputs: the caller
//! injects the event ID, wall-clock timestamp and CPU count (via [`EventInputs`], the way
//! [`default_log_path`] injects the environment into the pure [`resolve_log_path`]),
//! and the crate version + target triple are compile-time constants baked into the
//! binary. The sink writers are the only I/O.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::destination::OutputSection;
use crate::io::decode::{DecodeInfo, SilverFastFormat};
use crate::stage::{StageClock, StageKind};
use crate::types::{EncodeReport, FilmBaseSource, NcError, Result};

/// Telemetry event schema version. Bump on any change to [`TelemetryEvent`]'s
/// shape so a server can ingest old and new records side by side. Note the event
/// embeds domain types (`OutputSection`, `FilmBaseSource`, `SilverFastFormat`,
/// `StageKind`) whose serde representation lives elsewhere — a change to *their* wire form is also a schema change and must
/// bump this too.
///
/// v2: `conversion.algorithm` (`simple|density|sigmoid`) became
/// `conversion.reconstruction` (`simple|density`) + optional `conversion.curve`
/// (`exponential|sigmoid`), following the tagged-reconstruction recipe schema
/// (`negative-reconstruction-density-curves`).
///
/// v3: added `conversion.preset` (`legacy|film-master`), and
/// `conversion.output_hdr` meant "an f32 TIFF was written" for *every* branch —
/// derived from `OutputParams::depth()` rather than read off the `output.hdr`
/// switch, which a named preset pins at its default while still resolving f32
/// (`color/film-master-render-pipeline`).
///
/// v4: `conversion.output_hdr` (bool) became `conversion.output_depth`
/// (`u8`|`u10`|`u16`|`f32`), following the CLI/recipe rename of `output.hdr` →
/// `output.depth`. It reports the **primary image's** depth
/// (`OutputParams::primary_depth_label`), which for the JPEG and AVIF presets is
/// the container's fixed 8/10-bit — *not* `OutputParams::depth()`, whose value for
/// those presets is only the optional IR TIFF's. A **renamed field is a wire-shape change** under any reading of
/// the rule above, unlike *adding* an enum member — which `conversion.preset` has
/// now done eight times without a bump, and which
/// `output/sdr-preset-followups` still owns settling. `conversion.preset` carried
/// whichever output preset names the build of the time had (`OutputPreset::ALL`, gone
/// since v8); the list was never restated here, since that is exactly the rustdoc that
/// went stale at v3.
///
/// **Removing** members is not a bump either: `legacy` and `custom` left
/// `conversion.preset` with `nf-retire/legacy-custom` (2026-09-23). Records written
/// before carry them and stay readable; no new record can.
///
/// v5: `conversion.reconstruction` is gone and `conversion.curve` is always present,
/// because `simple` reconstruction retired (`nf-retire/sigmoid-and-simple`) and left
/// one reconstruction; `sigmoid` left `conversion.curve` in the same change.
///
/// v6: `conversion.dmax` is gone — the roll reference density retired with the
/// placements that read it (`nf-retire/dmax-machinery`).
///
/// v7: `conversion.curve` is gone — with the `characteristic` curve retired
/// (`nf-retire/characteristic`) the exponential is the only curve, so the field could
/// hold one value.
///
/// v8: `conversion.preset` is `conversion.destination` — the output presets retired
/// with the chain that rendered them (`nf-core/default-flip`), and a destination is the
/// recipe's `output` section (`"film-master"`, or `{"display": {range, transfer, gamut,
/// container}}` with every axis resolved). `params_hash` hashes the recipe that
/// replaced the old one (`crate::recipe`), and `timing_ms.algorithm` / `color` time the
/// fixed decode and the chain with its destination transfer.
///
/// v9: `timing_ms` names each stage (`crate::stage::StageKind`, `nf-core/report-contract`):
/// `algorithm` is `reconstruction`, and `color` splits into `scene_correction`, `look`,
/// `fit_range`, `fit_gamut` (absent for the film master, which runs none) and
/// `destination`.
///
/// v10: the record is an event (`telemetry/schema-v2`): `event_id`, `event`,
/// `command` and `stage` are new, `outcome` gains `status`, `error_kind` and
/// `exit_code`, and a failed run writes one too. What a failure had not reached is
/// absent: `image`, `conversion`, `outcome.clipped` / `non_finite`, and every
/// `timing_ms` stage field (all optional now) until that stage completes.
pub const SCHEMA_VERSION: u32 = 10;

/// Default local JSONL log path, honoring `NC_TELEMETRY_LOG` then the platform
/// data dir; `None` when no home/data dir can be located (the caller then warns
/// and skips the persistent sink, fail-soft). Reads the environment and defers
/// the precedence to the pure [`resolve_log_path`] (so the ordering is unit
/// testable without mutating process-global env vars).
pub fn default_log_path() -> Option<PathBuf> {
    // `APPDATA` is a Windows convention, so only consult it there; every other
    // platform falls through to the `HOME`/`.local/share` XDG default base.
    let appdata = if cfg!(windows) {
        non_empty_env("APPDATA")
    } else {
        None
    };
    resolve_log_path(
        non_empty_env("NC_TELEMETRY_LOG"),
        non_empty_env("XDG_DATA_HOME"),
        appdata,
        non_empty_env("HOME"),
    )
}

/// Pure log-path precedence (dependency-free, per the task's minimal-deps
/// preference), highest priority first:
/// 1. `NC_TELEMETRY_LOG` — explicit override (the full file path).
/// 2. `$XDG_DATA_HOME/nc/telemetry.jsonl`.
/// 3. `%APPDATA%\nc\telemetry.jsonl` (Windows; `None` on other platforms).
/// 4. `$HOME/.local/share/nc/telemetry.jsonl` — the XDG default base, and the
///    universal last-resort fallback on any platform with `HOME` set.
///
/// Returns `None` only when every source is absent.
fn resolve_log_path(
    explicit: Option<std::ffi::OsString>,
    xdg_data_home: Option<std::ffi::OsString>,
    appdata: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(PathBuf::from(p));
    }
    if let Some(x) = xdg_data_home {
        return Some(PathBuf::from(x).join("nc").join("telemetry.jsonl"));
    }
    if let Some(a) = appdata {
        return Some(PathBuf::from(a).join("nc").join("telemetry.jsonl"));
    }
    Some(
        PathBuf::from(home?)
            .join(".local")
            .join("share")
            .join("nc")
            .join("telemetry.jsonl"),
    )
}

/// `std::env::var_os` filtered to reject an empty value (an empty env var is
/// treated as unset, matching the XDG spec's handling of `XDG_DATA_HOME`).
fn non_empty_env(key: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(key).filter(|v| !v.is_empty())
}

// ---------------------------------------------------------------------------
// Event schema (serialize-only — nothing deserializes a telemetry event here;
// the upload projection is `telemetry/upload-schema`)
// ---------------------------------------------------------------------------

/// One telemetry event for a single `hanten convert` run that got past argument
/// parsing: a success, or a failure saying where and how the run failed
/// (design-spec §9).
///
/// A block the run never reached is absent, never zeroed: `image` before decode,
/// `conversion` before the destination resolved, a stage's timing before it
/// completed. The keys that are always present — `cpu_count`, `image.input_bytes`,
/// `image.output_bytes` — serialize an unknown value as JSON `null`.
#[derive(Clone, Debug, Serialize)]
pub struct TelemetryEvent {
    /// Event schema version ([`SCHEMA_VERSION`]) for server forward-compat.
    pub schema_version: u32,
    /// Random per-event ID, minted when the event is written: the upload
    /// deduplication key, never a correlation across events.
    pub event_id: EventId,
    pub event: EventName,
    pub command: CommandKind,
    /// Wall-clock time the event was built, UNIX epoch milliseconds.
    pub timestamp_ms: u64,
    /// `nc` crate version (`CARGO_PKG_VERSION`).
    pub nc_version: &'static str,
    /// Compile target triple (captured by `build.rs` into `NC_TARGET`).
    pub target: &'static str,
    /// Available parallelism; `None` when the platform can't report it.
    pub cpu_count: Option<u32>,
    /// Where the run ended: the failed stage, or `finalize` for a success.
    pub stage: EventStage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageInfo>,
    pub timing_ms: TimingInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversion: Option<ConversionInfo>,
    pub outcome: OutcomeInfo,
}

/// A random 128-bit event ID, serialized as 32 lowercase hex characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventId(pub [u8; 16]);

impl EventId {
    /// A fresh ID from the operating system's random source; `None` when it fails
    /// (the caller then writes no event).
    pub fn random() -> Option<Self> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).ok()?;
        Some(Self(bytes))
    }
}

impl Serialize for EventId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut hex = [0u8; 32];
        for (i, b) in self.0.iter().enumerate() {
            hex[2 * i] = HEX[usize::from(b >> 4)];
            hex[2 * i + 1] = HEX[usize::from(b & 0xf)];
        }
        // Every byte is an ASCII hex digit.
        s.serialize_str(std::str::from_utf8(&hex).expect("ASCII"))
    }
}

/// The event discriminator. A panic event joins it with `telemetry/panic-hook`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventName {
    Conversion,
}

/// The command an event describes; telemetry covers `convert` only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    Convert,
}

/// Where a run was when it ended: a [`StageKind`], or a phase around the stages.
/// Serialized as one flat string, so a `StageKind` rename moves it too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventStage {
    /// Argument parsing. Reserved: recorded only under persistent consent, since
    /// before parsing nc cannot know `--telemetry`.
    // Consumer: `telemetry/upload`'s parse-failure event.
    #[allow(dead_code)]
    Parse,
    /// Recipe load, merge and validation, output resolution, the write-target guard.
    Setup,
    /// A frame's checks before its first stage, the memory preflight among them.
    Preflight,
    Stage(StageKind),
    /// The report and the `--strict` gate; where every success ends.
    Finalize,
}

impl Serialize for EventStage {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            EventStage::Parse => s.serialize_str("parse"),
            EventStage::Setup => s.serialize_str("setup"),
            EventStage::Preflight => s.serialize_str("preflight"),
            EventStage::Stage(stage) => stage.serialize(s),
            EventStage::Finalize => s.serialize_str("finalize"),
        }
    }
}

/// How a run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeStatus {
    Success,
    Failure,
}

/// A failure's category: the [`NcError`] variant, or `strict` for a `--strict`
/// promotion. Never the error's text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    None,
    Usage,
    Decode,
    Unsupported,
    Write,
    Resource,
    Strict,
    Other,
}

impl ErrorKind {
    /// The category of `err`. Exhaustive, so a new `NcError` variant states its own.
    pub fn of(err: &NcError) -> Self {
        match err {
            NcError::Usage(_) => ErrorKind::Usage,
            NcError::Decode(_) => ErrorKind::Decode,
            NcError::Unsupported(_) => ErrorKind::Unsupported,
            NcError::Write(_) => ErrorKind::Write,
            NcError::Resource(_) => ErrorKind::Resource,
            NcError::Other(_) => ErrorKind::Other,
        }
    }
}

/// How a run ended, as the builder takes it: a success always ends in
/// [`EventStage::Finalize`], so only a failure names its stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Success,
    Failure {
        stage: EventStage,
        kind: ErrorKind,
        /// The process exit code the run returned.
        exit_code: u8,
    },
}

/// Image facts (from the decoder plus the on-disk file sizes).
#[derive(Clone, Debug, Serialize)]
pub struct ImageInfo {
    /// SilverFast variant (`"hdr"` / `"hdri"`).
    pub format: SilverFastFormat,
    pub width: u32,
    pub height: u32,
    /// `width * height / 1e6`, for quick scale bucketing on the server.
    pub megapixels: f64,
    /// Bits per sample of the primary image (16 for accepted scans).
    pub bit_depth: u8,
    /// RGB channels in the primary image (3).
    pub channels: u16,
    pub ir_present: bool,
    /// Input scan size in bytes; `None` if it couldn't be stat'd.
    pub input_bytes: Option<u64>,
    /// Written primary image size in bytes; `None` if it couldn't be stat'd or the
    /// run wrote none.
    pub output_bytes: Option<u64>,
}

/// Per-stage wall-clock timings in milliseconds, one field per [`StageKind`], each
/// absent until that stage **completes** (a failed stage's time counts only toward
/// `total`). `total` is the whole run up to the report, or up to the failure, so the
/// stages sum to less than it.
///
/// A gain map renders two renditions, so its `fit_range` and `fit_gamut` sum both
/// branches, and the copy that splits them counts only toward `total`.
/// `scene_correction` and `look` include the film base's one-pixel grade. The four
/// chain stages are absent for the film master, and `ir_export` without `--export-ir`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct TimingInfo {
    pub total: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decode: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub film_base: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reconstruction: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene_correction: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub look: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fit_range: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fit_gamut: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encode: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ir_export: Option<f64>,
}

impl TimingInfo {
    /// Add `ms` to `stage`'s field; a stage that runs twice sums.
    pub fn add(&mut self, stage: StageKind, ms: f64) {
        let field = match stage {
            StageKind::Decode => &mut self.decode,
            StageKind::FilmBase => &mut self.film_base,
            StageKind::Reconstruction => &mut self.reconstruction,
            StageKind::SceneCorrection => &mut self.scene_correction,
            StageKind::Look => &mut self.look,
            StageKind::FitRange => &mut self.fit_range,
            StageKind::FitGamut => &mut self.fit_gamut,
            StageKind::Destination => &mut self.destination,
            StageKind::Encode => &mut self.encode,
            StageKind::IrExport => &mut self.ir_export,
        };
        *field = Some(field.unwrap_or(0.0) + ms);
    }
}

/// The run's wall clock: a stage's time lands in its field only if the stage
/// returned `Ok`.
#[derive(Clone, Copy, Debug, Default)]
pub struct StageTimer {
    pub timings: TimingInfo,
    /// The stage most recently entered, completed or not.
    last: Option<StageKind>,
}

impl StageTimer {
    /// Where a failure inside a frame happened: the stage last entered — the one that
    /// returned `Err`, or, for a check between two stages, the one before it — else
    /// the frame's preflight.
    pub fn failed_stage(&self) -> EventStage {
        self.last.map_or(EventStage::Preflight, EventStage::Stage)
    }
}

impl StageClock for StageTimer {
    fn time<T>(&mut self, stage: StageKind, run: impl FnOnce() -> Result<T>) -> Result<T> {
        self.last = Some(stage);
        let started = Instant::now();
        let out = run();
        if out.is_ok() {
            self.timings
                .add(stage, started.elapsed().as_secs_f64() * 1000.0);
        }
        out
    }
}

/// Compact conversion summary. A full `params_hash` (over the effective recipe
/// JSON) lets the server dedup / group by exact parameters without the event
/// carrying the whole recipe; a few high-signal knobs ride alongside it.
#[derive(Clone, Debug, Serialize)]
pub struct ConversionInfo {
    /// The destination written, every axis resolved (the recipe's `output` shape).
    /// Recorded because it is the single biggest determinant of what the written pixels
    /// *are*: two f32 TIFFs (the film master, a linear HDR TIFF) are otherwise
    /// indistinguishable.
    pub destination: OutputSection,
    /// Stable 64-bit hash (hex) of the effective recipe JSON (`crate::recipe`), so
    /// identical conversions share a hash.
    pub params_hash: String,
    /// Film-base provenance (`"auto"` / `{"region":…}` / `{"explicit":…}`).
    pub film_base_source: FilmBaseSource,
    /// The primary image's sample depth as written (`u8` / `u10` / `u16` / `f32`),
    /// fixed by the destination's encoding.
    pub output_depth: &'static str,
}

/// How the run ended, and the quality signals a server watches for regressions.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct OutcomeInfo {
    pub status: OutcomeStatus,
    /// `none` exactly when `status` is `success`.
    pub error_kind: ErrorKind,
    /// The process exit code: 0 for a success.
    pub exit_code: u8,
    /// Warnings raised before the run ended (clipping, IR-ignored, BigTIFF promote…).
    pub warnings: u32,
    /// Finite samples clamped at a range end (`EncodeReport::clipped_total`); absent
    /// unless the frame finished.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clipped: Option<u64>,
    /// Non-finite (`NaN`/`±inf`) output samples — a numerical fault signal; absent
    /// unless the frame finished.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub non_finite: Option<u64>,
}

/// The decoded image's facts, for [`EventInputs::image`].
pub struct ImageFacts<'a> {
    pub info: &'a DecodeInfo,
    pub input_bytes: Option<u64>,
    pub output_bytes: Option<u64>,
}

/// Everything the orchestrator hands the pure [`build_event`] builder. Grouped
/// into one struct so the builder signature stays readable and the call site
/// names each field.
pub struct EventInputs<'a> {
    pub outcome: Outcome,
    /// Injected so the builder stays pure (see [`EventId::random`]).
    pub event_id: EventId,
    /// UNIX epoch milliseconds, injected (see [`now_unix_millis`]).
    pub timestamp_ms: u64,
    /// Available parallelism, injected (see [`cpu_count`]).
    pub cpu_count: Option<u32>,
    pub timings: TimingInfo,
    /// `None` when the run failed before decode.
    pub image: Option<ImageFacts<'a>>,
    /// `None` when the run failed before its destination resolved.
    pub conversion: Option<ConversionInfo>,
    /// `None` unless the frame finished.
    pub loss: Option<EncodeReport>,
    pub warnings: usize,
}

/// Build a [`TelemetryEvent`] from what the run learned. A pure function of
/// `inputs`: the ID, timestamp and CPU count are injected by the caller, and the
/// crate version + target are compile-time constants.
pub fn build_event(inputs: EventInputs<'_>) -> TelemetryEvent {
    let (stage, status, error_kind, exit_code) = match inputs.outcome {
        Outcome::Success => (
            EventStage::Finalize,
            OutcomeStatus::Success,
            ErrorKind::None,
            0,
        ),
        Outcome::Failure {
            stage,
            kind,
            exit_code,
        } => (stage, OutcomeStatus::Failure, kind, exit_code),
    };
    TelemetryEvent {
        schema_version: SCHEMA_VERSION,
        event_id: inputs.event_id,
        event: EventName::Conversion,
        command: CommandKind::Convert,
        timestamp_ms: inputs.timestamp_ms,
        nc_version: env!("CARGO_PKG_VERSION"),
        target: env!("NC_TARGET"),
        cpu_count: inputs.cpu_count,
        stage,
        image: inputs.image.map(|facts| {
            let info = facts.info;
            ImageInfo {
                format: info.format,
                width: info.width,
                height: info.height,
                megapixels: (info.width as f64 * info.height as f64) / 1_000_000.0,
                bit_depth: info.bits_per_sample,
                channels: info.channels,
                ir_present: info.ir_present,
                input_bytes: facts.input_bytes,
                output_bytes: facts.output_bytes,
            }
        }),
        timing_ms: inputs.timings,
        conversion: inputs.conversion,
        outcome: OutcomeInfo {
            status,
            error_kind,
            exit_code,
            warnings: warning_count(inputs.warnings),
            clipped: inputs.loss.map(|l| l.clipped_total()),
            non_finite: inputs.loss.map(|l| l.non_finite),
        },
    }
}

/// Saturate a warning count into the fixed-width wire field (a run never
/// realistically raises `u32::MAX` warnings).
fn warning_count(n: usize) -> u32 {
    n.min(u32::MAX as usize) as u32
}

/// UNIX epoch milliseconds now. Clamps a pre-epoch clock (`duration_since` errs
/// only if the system clock is before 1970) to 0 rather than failing — telemetry
/// is best-effort and a bogus clock must not abort the event. The orchestrator
/// reads this and injects it, keeping [`build_event`] pure.
pub fn now_unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

/// Available parallelism as the event's fixed-width `cpu_count`; `None` when the
/// platform can't report it. Injected like [`now_unix_millis`].
pub fn cpu_count() -> Option<u32> {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .ok()
}

// ---------------------------------------------------------------------------
// Sinks (the only I/O)
// ---------------------------------------------------------------------------

/// Append one compact JSON line to the persistent JSONL log, creating parent
/// directories and the file (create-append) as needed. One object per line so an
/// uploader can drain the queue line by line.
///
/// The record — body *and* its trailing newline — is assembled into one buffer and
/// emitted with a single [`write_all`](Write::write_all) to a file opened
/// `O_APPEND` (`append(true)`). Under `O_APPEND` each `write` syscall seeks to
/// end-of-file and appends atomically on a local POSIX filesystem, so a record
/// written in one `write` can't interleave its body with another writer's newline
/// and corrupt the one-object-per-line JSONL the uploader drains. Buffering the
/// whole `line + '\n'` into one `write_all` (vs. `writeln!`, which splits the body
/// and the newline into separate `write`s another appender could slip between)
/// keeps a small record — a single JSON line, well under the size at which a
/// regular-file `write` returns short — to one `write` in practice, preserving
/// that atomicity.
///
/// Boundary (why this is best-effort, not a hard guarantee): `write_all` *loops*
/// when a `write` returns short (EINTR / ENOSPC / rlimit), and only each
/// individual `write` is atomic under `O_APPEND` — so a partial write could let a
/// concurrent appender interleave. For a small line to a regular file that path is
/// vanishingly rare, and telemetry is best-effort/fail-soft, so we accept it here;
/// true cross-process serialization would need advisory file locking (`flock`),
/// deferred to the telemetry-infra spike. (Distinct from the `PIPE_BUF` bound that
/// governs *pipe* writes.)
pub fn append_jsonl(path: &Path, line: &str) -> io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let mut buf = String::with_capacity(line.len() + 1);
    buf.push_str(line);
    buf.push('\n');
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(buf.as_bytes())
}

/// Write the record as a single line to a one-off file (overwrite), creating
/// parent directories as needed.
pub fn write_oneoff(path: &Path, line: &str) -> io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let mut contents = String::with_capacity(line.len() + 1);
    contents.push_str(line);
    contents.push('\n');
    fs::write(path, contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_info() -> DecodeInfo {
        DecodeInfo {
            format: SilverFastFormat::Hdri,
            width: 2000,
            height: 3000,
            channels: 3,
            bits_per_sample: 16,
            ir_present: true,
            make: None,
            model: None,
            software: None,
            silverfast_xmp: None,
            embedded_icc: None,
            warnings: vec![],
        }
    }

    fn sample_timings() -> TimingInfo {
        TimingInfo {
            total: 100.0,
            decode: Some(40.0),
            film_base: Some(5.0),
            reconstruction: Some(20.0),
            scene_correction: Some(1.0),
            look: Some(4.0),
            fit_range: Some(3.0),
            fit_gamut: Some(2.0),
            destination: Some(5.0),
            encode: Some(18.0),
            ir_export: Some(2.0),
        }
    }

    fn sample_conversion() -> ConversionInfo {
        ConversionInfo {
            destination: sdr_p3_tiff(),
            params_hash: "deadbeef".into(),
            film_base_source: FilmBaseSource::Auto,
            output_depth: "u16",
        }
    }

    const ID: EventId = EventId([
        0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32,
        0x10,
    ]);

    /// A success with every block known.
    fn success_inputs(info: &DecodeInfo) -> EventInputs<'_> {
        EventInputs {
            outcome: Outcome::Success,
            event_id: ID,
            timestamp_ms: 1_752_566_400_000,
            cpu_count: Some(14),
            timings: sample_timings(),
            image: Some(ImageFacts {
                info,
                input_bytes: Some(12_345),
                output_bytes: Some(67_890),
            }),
            conversion: Some(sample_conversion()),
            loss: Some(EncodeReport {
                total_samples: 100,
                clipped_low: 1,
                clipped_high: 2,
                non_finite: 7,
            }),
            warnings: 4,
        }
    }

    /// Every `StageKind` wire name is a `timing_ms` key, so a rename moves both.
    #[test]
    fn every_stage_kind_names_a_timing_field() {
        // `sample_timings` sets every optional field.
        let timings = serde_json::to_value(sample_timings()).unwrap();
        for stage in StageKind::ALL {
            let name = serde_json::to_value(stage).unwrap();
            let name = name.as_str().unwrap();
            assert!(timings.get(name).is_some(), "no timing_ms.{name}");
        }
    }

    #[test]
    fn a_success_derives_image_fields_and_ends_in_finalize() {
        let info = sample_info();
        let ev = build_event(success_inputs(&info));

        assert_eq!(ev.schema_version, 10);
        assert_eq!(ev.stage, EventStage::Finalize);
        assert_eq!(ev.outcome.status, OutcomeStatus::Success);
        assert_eq!(ev.outcome.error_kind, ErrorKind::None);
        assert_eq!(ev.outcome.exit_code, 0);
        let image = ev.image.as_ref().unwrap();
        assert_eq!((image.width, image.height), (2000, 3000));
        // 2000 * 3000 = 6e6 pixels → 6.0 MP.
        assert_eq!(image.megapixels, 6.0);
        assert_eq!((image.channels, image.bit_depth), (3, 16));
        assert!(image.ir_present);
        assert_eq!(image.input_bytes, Some(12_345));
        assert_eq!(image.output_bytes, Some(67_890));
        assert_eq!(ev.outcome.clipped, Some(3)); // clipped_low + clipped_high
        assert_eq!(ev.outcome.non_finite, Some(7));
        assert_eq!(ev.outcome.warnings, 4);
        // Injected ambient values are echoed through verbatim (builder is pure).
        assert_eq!(ev.event_id, ID);
        assert_eq!(ev.timestamp_ms, 1_752_566_400_000);
        assert_eq!(ev.cpu_count, Some(14));
        // Compile-time build identity.
        assert_eq!(ev.nc_version, env!("CARGO_PKG_VERSION"));
        assert!(!ev.target.is_empty());
    }

    #[test]
    fn a_failure_before_decode_invents_no_image_timing_or_loss() {
        let ev = build_event(EventInputs {
            outcome: Outcome::Failure {
                stage: EventStage::Setup,
                kind: ErrorKind::Usage,
                exit_code: 2,
            },
            event_id: ID,
            timestamp_ms: 1,
            cpu_count: None,
            timings: TimingInfo {
                total: 3.0,
                ..TimingInfo::default()
            },
            image: None,
            conversion: None,
            loss: None,
            warnings: 0,
        });
        let json = serde_json::to_value(&ev).unwrap();
        for absent in ["image", "conversion"] {
            assert!(json.get(absent).is_none(), "{absent}: {json}");
        }
        assert_eq!(json["timing_ms"], serde_json::json!({"total": 3.0}));
        assert_eq!(
            json["outcome"],
            serde_json::json!({
                "status": "failure", "error_kind": "usage", "exit_code": 2, "warnings": 0
            })
        );
        assert_eq!(json["stage"], "setup");
    }

    #[test]
    fn every_error_kind_is_its_ncerror_variant() {
        let m = || String::new();
        let cases = [
            (NcError::Usage(m()), ErrorKind::Usage),
            (NcError::Decode(m()), ErrorKind::Decode),
            (NcError::Unsupported(m()), ErrorKind::Unsupported),
            (NcError::Write(m()), ErrorKind::Write),
            (NcError::Resource(m()), ErrorKind::Resource),
            (NcError::Other(m()), ErrorKind::Other),
        ];
        for (err, kind) in cases {
            assert_eq!(ErrorKind::of(&err), kind, "{err:?}");
        }
    }

    #[test]
    fn missing_ir_has_no_ir_export_timing() {
        let mut info = sample_info();
        info.format = SilverFastFormat::Hdr;
        info.ir_present = false;
        let mut inputs = success_inputs(&info);
        inputs.timings.ir_export = None;
        let ev = build_event(inputs);
        assert!(!ev.image.as_ref().unwrap().ir_present);
        assert!(ev.timing_ms.ir_export.is_none());
        // A serialized event omits the absent optional fields entirely.
        let json = serde_json::to_string(&ev).unwrap();
        assert!(
            !json.contains("ir_export"),
            "absent IR export must be omitted"
        );
    }

    #[test]
    fn event_ids_are_32_lowercase_hex_and_random() {
        let json = serde_json::to_value(ID).unwrap();
        assert_eq!(json, "0123456789abcdeffedcba9876543210");
        let (a, b) = (EventId::random().unwrap(), EventId::random().unwrap());
        assert_ne!(a, b, "two events must not share an ID");
    }

    #[test]
    fn event_stage_names_are_flat_strings() {
        let json = serde_json::to_string(&[
            EventStage::Parse,
            EventStage::Setup,
            EventStage::Preflight,
            EventStage::Stage(StageKind::FitGamut),
            EventStage::Finalize,
        ])
        .unwrap();
        assert_eq!(
            json,
            r#"["parse","setup","preflight","fit_gamut","finalize"]"#
        );
    }

    #[test]
    fn resolve_log_path_precedence() {
        use std::ffi::OsString;
        let os = |s: &str| Some(OsString::from(s));

        // Explicit override wins over everything.
        assert_eq!(
            resolve_log_path(os("/x/tel.jsonl"), os("/xdg"), os("C:/app"), os("/home")),
            Some(PathBuf::from("/x/tel.jsonl"))
        );
        // Then XDG_DATA_HOME.
        assert_eq!(
            resolve_log_path(None, os("/xdg"), os("C:/app"), os("/home")),
            Some(PathBuf::from("/xdg/nc/telemetry.jsonl"))
        );
        // Then APPDATA (the Windows tier) before the HOME fallback.
        assert_eq!(
            resolve_log_path(None, None, os("C:/app"), os("/home")),
            Some(PathBuf::from("C:/app/nc/telemetry.jsonl"))
        );
        // Then the HOME/.local/share default base (universal last resort).
        assert_eq!(
            resolve_log_path(None, None, None, os("/home")),
            Some(PathBuf::from("/home/.local/share/nc/telemetry.jsonl"))
        );
        // Nothing set → no path (caller warns and skips the persistent sink).
        assert_eq!(resolve_log_path(None, None, None, None), None);
    }

    #[test]
    fn a_stage_that_runs_twice_sums_and_one_that_never_ran_is_absent() {
        let mut t = TimingInfo::default();
        t.add(StageKind::FitRange, 1.5);
        t.add(StageKind::FitRange, 2.0);
        t.add(StageKind::Decode, 4.0);
        assert_eq!(t.fit_range, Some(3.5));
        assert_eq!(t.decode, Some(4.0));
        assert_eq!(t.look, None);
        let json = serde_json::to_string(&t).unwrap();
        assert!(!json.contains("look"), "{json}");
    }

    #[test]
    fn the_clock_times_into_the_stage_it_names() {
        let mut t = StageTimer::default();
        let out = t.time(StageKind::Look, || Ok(7));
        assert_eq!(out.unwrap(), 7);
        assert!(t.timings.look.is_some_and(|ms| ms >= 0.0));
        assert_eq!(t.timings.fit_gamut, None);
    }

    #[test]
    fn a_failed_stage_is_named_and_not_timed() {
        let mut t = StageTimer::default();
        assert_eq!(t.failed_stage(), EventStage::Preflight);
        t.time(StageKind::Decode, || Ok(())).unwrap();
        // A check between stages belongs to the stage before it.
        assert_eq!(t.failed_stage(), EventStage::Stage(StageKind::Decode));
        let out: Result<()> = t.time(StageKind::FilmBase, || Err(NcError::Other("x".into())));
        assert!(out.is_err());
        assert_eq!(t.failed_stage(), EventStage::Stage(StageKind::FilmBase));
        assert!(t.timings.decode.is_some());
        assert_eq!(
            t.timings.film_base, None,
            "a failed stage's time is not its field's"
        );
    }

    #[test]
    fn append_jsonl_appends_one_line_per_call() {
        let dir = std::env::temp_dir().join(format!("nc-tel-{}", std::process::id()));
        let path = dir.join("telemetry.jsonl");
        let _ = fs::remove_dir_all(&dir);
        append_jsonl(&path, r#"{"a":1}"#).unwrap();
        append_jsonl(&path, r#"{"a":2}"#).unwrap();
        let contents = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines, vec![r#"{"a":1}"#, r#"{"a":2}"#]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_jsonl_is_atomic_under_concurrency() {
        // Many threads, each opening its own O_APPEND handle per call (as separate
        // processes would), hammer one log. Every line must survive intact — no
        // interleaved body/newline — so the readback has exactly N well-formed
        // one-object-per-line records. This exercises the single-`write_all`
        // atomicity the JSONL contract depends on.
        //
        // Payloads are padded past a filesystem page (> 4 KiB) so the pre-fix
        // two-write shape (`writeln!` = body then a separate `\n`) would leave a
        // wide interleave window and actually corrupt a line — a ~30-byte record
        // is too small to reliably fail the buggy version this guards against.
        use std::sync::Arc;
        let dir = std::env::temp_dir().join(format!("nc-tel-atomic-{}", std::process::id()));
        let path = Arc::new(dir.join("telemetry.jsonl"));
        let _ = fs::remove_dir_all(&dir);

        const THREADS: usize = 8;
        const PER_THREAD: usize = 100;
        const PAD: usize = 6000; // comfortably past a 4 KiB page
        let handles: Vec<_> = (0..THREADS)
            .map(|t| {
                let path = Arc::clone(&path);
                std::thread::spawn(move || {
                    for i in 0..PER_THREAD {
                        // Distinct payload per write, padded past a page. The pad
                        // char differs per thread so a cross-thread splice is
                        // visible as a JSON parse failure below.
                        let pad = std::iter::repeat_n(char::from(b'a' + t as u8), PAD)
                            .collect::<String>();
                        let line = format!(r#"{{"thread":{t},"seq":{i},"pad":"{pad}"}}"#);
                        append_jsonl(&path, &line).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }

        let contents = fs::read_to_string(&*path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), THREADS * PER_THREAD, "one line per append");
        for line in lines {
            // A torn write would leave a line that isn't a standalone JSON object.
            serde_json::from_str::<serde_json::Value>(line)
                .unwrap_or_else(|e| panic!("corrupt JSONL line (len {}): {e}", line.len()));
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// The default destination, every axis stated as a report resolves it.
    fn sdr_p3_tiff() -> OutputSection {
        use crate::destination::{Container, DisplayAxes, Gamut, Range, Transfer};
        OutputSection::Display(DisplayAxes {
            range: Some(Range::Sdr),
            transfer: Some(Transfer::Native),
            gamut: Some(Gamut::DisplayP3),
            container: Some(Container::Tiff),
        })
    }

    #[test]
    fn event_wire_shape_is_pinned() {
        // Snapshot the exact serialized JSON for a fully-populated success and a
        // minimal failure. This catches silent wire-shape drift — a renamed/added/
        // removed field, a reordered struct, or a changed foreign-enum
        // representation (`FilmBaseSource`/`SilverFastFormat`/`StageKind`) — any of
        // which is a `SCHEMA_VERSION` bump. If this test fails, update the snapshot
        // *and* bump `SCHEMA_VERSION` (and the design-spec / SKILL examples).
        // `nc_version`/`target` are set to fixed literals here so the snapshot is
        // build- and platform-independent.
        let info = DecodeInfo {
            width: 100,
            height: 200,
            ..sample_info()
        };
        let mut full = build_event(EventInputs {
            timestamp_ms: 1_700_000_000_000,
            cpu_count: Some(8),
            timings: TimingInfo {
                total: 30.0,
                decode: Some(5.0),
                film_base: Some(1.0),
                reconstruction: Some(10.0),
                scene_correction: Some(0.5),
                look: Some(3.0),
                fit_range: Some(2.0),
                fit_gamut: Some(1.5),
                destination: Some(1.0),
                encode: Some(4.0),
                ir_export: Some(2.0),
            },
            image: Some(ImageFacts {
                info: &info,
                input_bytes: Some(1000),
                output_bytes: Some(2000),
            }),
            conversion: Some(ConversionInfo {
                destination: sdr_p3_tiff(),
                params_hash: "0123456789abcdef".into(),
                film_base_source: FilmBaseSource::Explicit([0.5, 0.25, 0.125]),
                output_depth: "u16",
            }),
            loss: Some(EncodeReport {
                total_samples: 10,
                clipped_low: 0,
                clipped_high: 2,
                non_finite: 0,
            }),
            warnings: 1,
            ..success_inputs(&info)
        });
        full.nc_version = "9.9.9";
        full.target = "test-triple";
        let expected_full = concat!(
            r#"{"schema_version":10,"event_id":"0123456789abcdeffedcba9876543210","#,
            r#""event":"conversion","command":"convert","timestamp_ms":1700000000000,"#,
            r#""nc_version":"9.9.9","target":"test-triple","cpu_count":8,"stage":"finalize","#,
            r#""image":{"format":"hdri","width":100,"height":200,"megapixels":0.02,"#,
            r#""bit_depth":16,"channels":3,"ir_present":true,"input_bytes":1000,"#,
            r#""output_bytes":2000},"#,
            r#""timing_ms":{"total":30.0,"decode":5.0,"film_base":1.0,"reconstruction":10.0,"#,
            r#""scene_correction":0.5,"look":3.0,"fit_range":2.0,"fit_gamut":1.5,"#,
            r#""destination":1.0,"encode":4.0,"ir_export":2.0},"#,
            r#""conversion":{"destination":{"display":{"range":"sdr","transfer":"native","#,
            r#""gamut":"display-p3","container":"tiff"}},"params_hash":"0123456789abcdef","#,
            r#""film_base_source":{"explicit":[0.5,0.25,0.125]},"output_depth":"u16"},"#,
            r#""outcome":{"status":"success","error_kind":"none","exit_code":0,"warnings":1,"#,
            r#""clipped":2,"non_finite":0}}"#,
        );
        assert_eq!(serde_json::to_string(&full).unwrap(), expected_full);

        // Minimal: a film-master run whose decode failed. Nothing past the failure is
        // present; `cpu_count` serializes as null. Snapshotted so the `"film-master"`
        // wire name and its `f32` depth are pinned too.
        let mut minimal = build_event(EventInputs {
            outcome: Outcome::Failure {
                stage: EventStage::Stage(StageKind::Decode),
                kind: ErrorKind::Decode,
                exit_code: 3,
            },
            event_id: EventId([0; 16]),
            timestamp_ms: 0,
            cpu_count: None,
            timings: TimingInfo::default(),
            image: None,
            conversion: Some(ConversionInfo {
                destination: OutputSection::FilmMaster,
                params_hash: "0".into(),
                film_base_source: FilmBaseSource::Auto,
                output_depth: "f32",
            }),
            loss: None,
            warnings: 0,
        });
        minimal.nc_version = "9.9.9";
        minimal.target = "test-triple";
        let expected_minimal = concat!(
            r#"{"schema_version":10,"event_id":"00000000000000000000000000000000","#,
            r#""event":"conversion","command":"convert","timestamp_ms":0,"#,
            r#""nc_version":"9.9.9","target":"test-triple","cpu_count":null,"stage":"decode","#,
            r#""timing_ms":{"total":0.0},"#,
            r#""conversion":{"destination":"film-master","params_hash":"0","#,
            r#""film_base_source":"auto","output_depth":"f32"},"#,
            r#""outcome":{"status":"failure","error_kind":"decode","exit_code":3,"warnings":0}}"#,
        );
        assert_eq!(serde_json::to_string(&minimal).unwrap(), expected_minimal);
    }

    #[test]
    fn write_oneoff_overwrites() {
        let dir = std::env::temp_dir().join(format!("nc-tel-oneoff-{}", std::process::id()));
        let path = dir.join("run.json");
        let _ = fs::remove_dir_all(&dir);
        write_oneoff(&path, r#"{"a":1}"#).unwrap();
        write_oneoff(&path, r#"{"a":2}"#).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"a\":2}\n");
        let _ = fs::remove_dir_all(&dir);
    }
}

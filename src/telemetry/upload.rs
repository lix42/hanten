//! The upload projection: the privacy-minimized, separately versioned form of a local
//! [`TelemetryEvent`] that may cross the network (`telemetry/upload-schema`).
//!
//! [`to_upload_event`] makes a conversion's [`UploadEvent`]. It copies the
//! event ID, keeps enums, and buckets or rounds every exact fact (time to a day, sizes
//! and counts to buckets, the target triple to an OS and an architecture); the local
//! record keeps the exact values. Nothing else — no `params_hash`, dimensions, byte
//! sizes, film-base values or free text — has a field to land in.
//!
//! The wire contract is the checked-in JSON Schema and corpus in
//! `contracts/telemetry/upload-v1/` (its README states the rules), which the Worker
//! reads too. A block the run never reached is **absent**; `unknown` is a value the
//! run reached but could not classify. Changing a field here means changing the
//! schema, the corpus and the Worker together.

use serde::Serialize;

use super::{
    CommandKind, ErrorKind, EventId, EventName, EventStage, OutcomeStatus, SCHEMA_VERSION,
    TelemetryEvent, TimingInfo,
};
use crate::destination::{Defaults, Encoding, OutputSection, resolve};
use crate::io::decode::SilverFastFormat;
use crate::types::FilmBaseProvenance;

/// The request envelope's `upload_schema_version`.
pub const UPLOAD_SCHEMA_VERSION: u32 = 1;

/// Why a local event has no upload form. The uploader drops a line of another
/// schema version and quarantines the rest (`telemetry::spool::project`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotUploadable {
    /// Not the local schema this build projects ([`SCHEMA_VERSION`]).
    SchemaVersion,
    /// `nc_version` is not a SemVer core with an optional prerelease.
    NcVersion,
    /// The destination does not state all four axes of a writable row.
    Destination,
    /// A film base no conversion resolves (`effective_area`).
    FilmBase,
    /// Status, error kind, exit code, stage and finished-frame facts break the
    /// schema's pairing rules.
    Outcome,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UploadEvent {
    pub source_schema_version: u32,
    pub event_id: EventId,
    /// UTC days since the Unix epoch.
    pub event_day: u16,
    pub event_name: EventName,
    pub nc_version: String,
    pub platform: Platform,
    pub command: CommandKind,
    pub stage: EventStage,
    pub outcome: Outcome,
    pub timing_ms: Timing,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<Image>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversion: Option<Conversion>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Platform {
    pub os: &'static str,
    pub arch: &'static str,
    pub cpu_bucket: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Outcome {
    pub status: OutcomeStatus,
    pub exit_code: u8,
    pub error_kind: ErrorKind,
    pub warning_bucket: &'static str,
    /// Absent unless the frame finished, like the local counts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clipped_fraction_bucket: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub non_finite: Option<bool>,
}

/// Whole milliseconds, rounded and saturated at [`MAX_MS`]; a stage is absent until it
/// completed, as locally.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Timing {
    pub total: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decode: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub film_base: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reconstruction: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene_correction: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub look: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fit_range: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fit_gamut: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encode: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ir_export: Option<u32>,
}

/// A day, the ceiling of every `timing_ms` field.
pub const MAX_MS: u32 = 86_400_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Image {
    pub format: SilverFastFormat,
    /// Megapixels rounded to a tenth, saturated at [`MAX_MEGAPIXELS_TENTHS`].
    pub megapixels_tenths: u32,
    pub input_size_bucket: &'static str,
    pub bit_depth: BitDepth,
    pub ir_present: bool,
}

/// 10,000 MP.
pub const MAX_MEGAPIXELS_TENTHS: u32 = 100_000;

/// Bits per sample: `16`, or `"unknown"` for anything else — so a per-pixel total
/// (48, 64) misread as a sample depth can never be uploaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitDepth {
    Sixteen,
    Unknown,
}

impl Serialize for BitDepth {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            BitDepth::Sixteen => s.serialize_u8(16),
            BitDepth::Unknown => s.serialize_str("unknown"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Conversion {
    pub encoding: &'static str,
    /// The kind only (`region` / `explicit`), never its coordinates or values.
    pub film_base_source: &'static str,
    pub ir_exported: bool,
}

/// Project a local event to its upload form. Pure: the same event always projects to
/// the same upload event.
pub fn to_upload_event(event: &TelemetryEvent) -> Result<UploadEvent, NotUploadable> {
    if event.schema_version != SCHEMA_VERSION {
        return Err(NotUploadable::SchemaVersion);
    }
    if !is_release_version(&event.nc_version) {
        return Err(NotUploadable::NcVersion);
    }
    if !outcome_is_consistent(event) {
        return Err(NotUploadable::Outcome);
    }
    let conversion = match &event.conversion {
        None => None,
        Some(c) => Some(Conversion {
            encoding: encoding(&c.destination)?,
            film_base_source: match c.film_base_source {
                FilmBaseProvenance::Region(_) => "region",
                FilmBaseProvenance::Explicit(_) => "explicit",
                FilmBaseProvenance::EffectiveArea => return Err(NotUploadable::FilmBase),
            },
            ir_exported: event.timing_ms.ir_export.is_some(),
        }),
    };
    let (os, arch) = platform(&event.target);
    let o = &event.outcome;
    Ok(UploadEvent {
        source_schema_version: SCHEMA_VERSION,
        event_id: event.event_id,
        event_day: event_day(event.timestamp_ms),
        event_name: event.event,
        nc_version: event.nc_version.to_string(),
        platform: Platform {
            os,
            arch,
            cpu_bucket: cpu_bucket(event.cpu_count),
        },
        command: event.command,
        stage: event.stage,
        outcome: Outcome {
            status: o.status,
            exit_code: o.exit_code,
            error_kind: o.error_kind,
            warning_bucket: warning_bucket(o.warnings),
            clipped_fraction_bucket: o
                .clipped
                .map(|clipped| clipped_fraction_bucket(clipped, o.total_samples.unwrap_or(0))),
            non_finite: o.non_finite.map(|n| n > 0),
        },
        timing_ms: timing(&event.timing_ms),
        image: event.image.as_ref().map(|i| Image {
            format: i.format,
            megapixels_tenths: megapixels_tenths(i.width, i.height),
            input_size_bucket: input_size_bucket(i.input_bytes),
            bit_depth: if i.bit_depth == 16 {
                BitDepth::Sixteen
            } else {
                BitDepth::Unknown
            },
            ir_present: i.ir_present,
        }),
        conversion,
    })
}

/// The schema's relational rules: a success ends in `finalize` with exit 0 and every
/// finished-frame fact; a failure's exit code is its kind's; the clip pair travels
/// together.
fn outcome_is_consistent(event: &TelemetryEvent) -> bool {
    let o = &event.outcome;
    let clip_pair = o.clipped.is_some() == o.non_finite.is_some();
    let paired = match o.status {
        OutcomeStatus::Success => {
            o.error_kind == ErrorKind::None
                && o.exit_code == 0
                && event.stage == EventStage::Finalize
                && event.image.is_some()
                && event.conversion.is_some()
                && o.clipped.is_some()
        }
        OutcomeStatus::Failure => failure_exit_code(o.error_kind) == Some(o.exit_code),
    };
    clip_pair && paired
}

/// The exit code a failure of `kind` ends with (design-spec §11).
fn failure_exit_code(kind: ErrorKind) -> Option<u8> {
    match kind {
        ErrorKind::None => None,
        ErrorKind::Usage => Some(2),
        ErrorKind::Decode => Some(3),
        ErrorKind::Unsupported => Some(4),
        ErrorKind::Write => Some(5),
        ErrorKind::Resource => Some(6),
        ErrorKind::Strict | ErrorKind::Other => Some(1),
    }
}

/// The encoding the destination's recorded axes resolve to. Looked up from the axes,
/// which a log line read back keeps, rather than carried beside them.
fn encoding(destination: &OutputSection) -> Result<&'static str, NotUploadable> {
    let axes = match destination {
        OutputSection::FilmMaster => return Ok("film_master"),
        OutputSection::Display(axes) => axes,
    };
    // A run records every axis; an unstated one would resolve from defaults.
    let stated = axes.range.is_some()
        && axes.transfer.is_some()
        && axes.gamut.is_some()
        && axes.container.is_some();
    if !stated {
        return Err(NotUploadable::Destination);
    }
    let resolved = resolve(axes, &Defaults::STANDARD).map_err(|_| NotUploadable::Destination)?;
    Ok(match resolved.encoding {
        Encoding::SdrTiff => "sdr_tiff",
        Encoding::HdrLinearTiff => "hdr_linear_tiff",
        Encoding::HdrCodedTiff(_) => "hdr_coded_tiff",
        Encoding::HdrAvif(_) => "avif",
        Encoding::GainMapJpeg => "gain_map_jpeg",
    })
}

fn event_day(timestamp_ms: u64) -> u16 {
    (timestamp_ms / 86_400_000).min(u64::from(u16::MAX)) as u16
}

/// The OS and architecture of a Rust target triple (`arch-vendor-os[-env]`).
fn platform(target: &str) -> (&'static str, &'static str) {
    if target.is_empty() {
        return ("unknown", "unknown");
    }
    let arch = match target.split('-').next().unwrap_or("") {
        "x86_64" | "x86_64h" => "x86_64",
        "aarch64" | "arm64" | "arm64e" | "arm64ec" => "aarch64",
        "i386" | "i586" | "i686" | "x86" => "x86",
        // A 32-bit-pointer ABI on a 64-bit core: neither `aarch64` nor `arm`.
        "arm64_32" => "other",
        a if a.starts_with("arm") || a.starts_with("thumb") => "arm",
        _ => "other",
    };
    let has = |part: &str| target.split('-').any(|p| p == part);
    let os = if has("darwin") {
        "macos"
    } else if has("linux") {
        "linux"
    } else if has("windows") {
        "windows"
    } else {
        "other"
    };
    (os, arch)
}

/// The largest power of two at or below `n`, up to `64_plus`.
fn cpu_bucket(n: Option<u32>) -> &'static str {
    match n {
        None | Some(0) => "unknown",
        Some(1) => "1",
        Some(2..=3) => "2",
        Some(4..=7) => "4",
        Some(8..=15) => "8",
        Some(16..=31) => "16",
        Some(32..=63) => "32",
        Some(_) => "64_plus",
    }
}

fn warning_bucket(n: u32) -> &'static str {
    match n {
        0 => "0",
        1 => "1",
        2..=3 => "2_3",
        _ => "4_plus",
    }
}

/// `clipped / total` against 0.1 %, 1 % and 10 %, in integers; `unknown` without a
/// denominator.
fn clipped_fraction_bucket(clipped: u64, total: u64) -> &'static str {
    let (c, t) = (u128::from(clipped), u128::from(total));
    if clipped == 0 {
        "0"
    } else if total == 0 {
        "unknown"
    } else if c * 1000 < t {
        "lt_0_1_pct"
    } else if c * 100 < t {
        "lt_1_pct"
    } else if c * 10 < t {
        "lt_10_pct"
    } else {
        "gte_10_pct"
    }
}

fn timing(t: &TimingInfo) -> Timing {
    Timing {
        total: ms(t.total),
        decode: t.decode.map(ms),
        film_base: t.film_base.map(ms),
        reconstruction: t.reconstruction.map(ms),
        scene_correction: t.scene_correction.map(ms),
        look: t.look.map(ms),
        fit_range: t.fit_range.map(ms),
        fit_gamut: t.fit_gamut.map(ms),
        destination: t.destination.map(ms),
        encode: t.encode.map(ms),
        ir_export: t.ir_export.map(ms),
    }
}

/// Rounded whole milliseconds in `0..=MAX_MS`; a non-finite value is 0.
fn ms(v: f64) -> u32 {
    if v.is_finite() {
        v.round().clamp(0.0, f64::from(MAX_MS)) as u32
    } else {
        0
    }
}

fn megapixels_tenths(width: u32, height: u32) -> u32 {
    let tenths = (u64::from(width) * u64::from(height) + 50_000) / 100_000;
    tenths.min(u64::from(MAX_MEGAPIXELS_TENTHS)) as u32
}

fn input_size_bucket(bytes: Option<u64>) -> &'static str {
    const MIB: u64 = 1 << 20;
    match bytes {
        None => "unknown",
        Some(b) if b < 8 * MIB => "lt_8_mib",
        Some(b) if b < 32 * MIB => "8_31_mib",
        Some(b) if b < 128 * MIB => "32_127_mib",
        Some(b) if b < 512 * MIB => "128_511_mib",
        Some(_) => "512_plus_mib",
    }
}

/// A SemVer 2.0 core with an optional prerelease and no build metadata, ASCII,
/// 1–64 bytes — the schema's `nc_version` pattern.
fn is_release_version(v: &str) -> bool {
    let numeric = |s: &str| {
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'))
    };
    let identifier = |s: &str| {
        !s.is_empty()
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && (!s.bytes().all(|b| b.is_ascii_digit()) || numeric(s))
    };
    if v.is_empty() || v.len() > 64 {
        return false;
    }
    let (core, pre) = match v.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (v, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| numeric(p))
        && pre.is_none_or(|p| p.split('.').all(identifier))
}

#[cfg(test)]
mod tests;

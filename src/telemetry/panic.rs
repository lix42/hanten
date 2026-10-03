//! Panic reporting: the local panic event and the frame sanitizer
//! (`docs/telemetry-strategy.md`, "Failure and panic collection").
//!
//! A panic event is one compact JSON object and a newline, at most [`MAX_BYTES`],
//! in its own `panic-ready-<id>.json` in the queue's spool — never a line of the
//! queue file (`contracts/telemetry/upload-v1/README.md`, "The local panic event").
//! It carries the build, the platform, the stage the run was in and up to
//! [`MAX_FRAMES`] `nc` function paths; never the panic's message, a source
//! location, an address or any other frame.
//!
//! This is panic reporting, not crash reporting: signals, access violations,
//! aborts, OOM kills and forced termination run no panic hook.
//!
//! [`install`] adds the hook, only for a `convert` under persistent consent
//! (`managed::keep_for_process`), whose snapshot and shared collection lease live
//! until the process ends, so the spool cannot be purged or retargeted under it.
//! The first panic of the process writes one event (none once the spool holds
//! [`MAX_READY`]) and starts the upload helper, then every panic runs the previous
//! hook unchanged. The hook takes no lock (a panic may happen while the queue lock
//! is held), not even the consent gate: the helper's drain checks consent before
//! every request. It never panics, since a panic inside a panic hook aborts, and
//! every failure abandons the event.

use std::borrow::Cow;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

use super::durable;
use super::managed;
use super::net::{self, Endpoint};
use super::spool::{self, Kind};
use super::{CommandKind, EventId, EventName, EventStage, SCHEMA_VERSION};
use crate::stage::StageKind;

/// The most frames an event keeps (innermost first).
pub const MAX_FRAMES: usize = 32;
/// The largest panic-ready file, newline included.
pub const MAX_BYTES: usize = 16 * 1024;
/// The most panic-ready files a spool holds before a hook abandons its event, so
/// they stay within 1 MiB (processes racing may each add one more).
pub const MAX_READY: usize = 64;
const MAX_FRAME_BYTES: usize = 192;
const MAX_SEGMENTS: usize = 15;

/// One local panic event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanicEvent {
    pub schema_version: u32,
    pub event_id: EventId,
    pub event: EventName,
    pub command: CommandKind,
    pub timestamp_ms: u64,
    pub nc_version: Cow<'static, str>,
    pub target: Cow<'static, str>,
    pub cpu_count: Option<u32>,
    pub stage: PanicStage,
    pub frames: Vec<String>,
}

/// The stage the run was in when it panicked, `unknown` when the hook cannot tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanicStage {
    At(EventStage),
    Unknown,
}

impl Serialize for PanicStage {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            PanicStage::At(stage) => stage.serialize(s),
            PanicStage::Unknown => s.serialize_str("unknown"),
        }
    }
}

impl<'de> Deserialize<'de> for PanicStage {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = Cow::<str>::deserialize(d)?;
        if s == "unknown" {
            return Ok(PanicStage::Unknown);
        }
        EventStage::deserialize(serde::de::value::StrDeserializer::<D::Error>::new(&s))
            .map(PanicStage::At)
    }
}

/// Build a panic event. Pure: the caller injects the ID, time and CPU count. Frames
/// that are not [`is_frame`] are dropped and the list is capped at [`MAX_FRAMES`].
pub fn build(
    event_id: EventId,
    timestamp_ms: u64,
    cpu_count: Option<u32>,
    stage: PanicStage,
    frames: Vec<String>,
) -> PanicEvent {
    PanicEvent {
        schema_version: SCHEMA_VERSION,
        event_id,
        event: EventName::Panic,
        command: CommandKind::Convert,
        timestamp_ms,
        nc_version: Cow::Borrowed(env!("CARGO_PKG_VERSION")),
        target: Cow::Borrowed(env!("NC_TARGET")),
        cpu_count,
        stage,
        frames: frames
            .into_iter()
            .filter(|f| is_frame(f))
            .take(MAX_FRAMES)
            .collect(),
    }
}

/// The panic-ready file's bytes: the compact event and a newline, at most
/// [`MAX_BYTES`]. `None` if it cannot be serialized within the cap.
pub fn ready_bytes(event: &PanicEvent) -> Option<Vec<u8>> {
    let mut bytes = serde_json::to_vec(event).ok()?;
    bytes.push(b'\n');
    (bytes.len() <= MAX_BYTES).then_some(bytes)
}

/// Whether `frame` is a normalized `nc` path the contract allows: ASCII, 2–192
/// bytes, `nc` then at most 15 `::identifier` segments.
pub fn is_frame(frame: &str) -> bool {
    let Some(rest) = frame.strip_prefix("nc") else {
        return false;
    };
    if frame.len() < 2 || frame.len() > MAX_FRAME_BYTES {
        return false;
    }
    if rest.is_empty() {
        return true;
    }
    let Some(rest) = rest.strip_prefix("::") else {
        return false;
    };
    let segments: Vec<&str> = rest.split("::").collect();
    segments.len() <= MAX_SEGMENTS && segments.iter().all(|s| is_identifier(s))
}

fn is_identifier(s: &str) -> bool {
    let mut bytes = s.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// The `nc` frames of a backtrace's text (`std::backtrace::Backtrace`'s `Display`),
/// innermost first, at most [`MAX_FRAMES`]. `crate_root` is the binary crate's name
/// (`hanten`), which a frame's path starts with and which becomes `nc`. A closure
/// counts as the function that encloses it, an inherent method `<Type>::m` as
/// `Type::m`; generic arguments and a hash suffix are stripped. Anything else that
/// is not a plain path in this crate is dropped (a trait method among them), as are
/// the panic module's own frames.
pub fn sanitize(backtrace: &str, crate_root: &str) -> Vec<String> {
    backtrace
        .lines()
        .filter_map(|line| frame(line, crate_root))
        .filter(|f| *f != "nc::telemetry::panic" && !f.starts_with("nc::telemetry::panic::"))
        .take(MAX_FRAMES)
        .collect()
}

/// One backtrace line (`  12: path`) as a normalized frame, if it is one.
fn frame(line: &str, crate_root: &str) -> Option<String> {
    let (index, symbol) = line.trim_start().split_once(": ")?;
    if index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let symbol = inherent(symbol)?;
    let mut path = symbol.strip_prefix(crate_root)?;
    // A v0-style crate disambiguator: `hanten[2358ed647de94a65]::…`.
    if let Some(rest) = path.strip_prefix('[') {
        let (hash, rest) = rest.split_once(']')?;
        if hash.is_empty() || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        path = rest;
    }
    let mut path = path.strip_prefix("::")?;
    loop {
        if let Some(enclosing) = strip_closure(path).or_else(|| strip_generic_args(path)) {
            path = enclosing;
        } else if let Some((enclosing, last)) = path.rsplit_once("::")
            && is_legacy_hash(last)
        {
            path = enclosing;
        } else {
            break;
        }
    }
    let normalized = format!("nc::{path}");
    is_frame(&normalized).then_some(normalized)
}

/// An inherent method's `<Type>::method` as `Type::method`; a symbol not starting
/// with `<` as it is. A trait method (`<Type as Trait>::method`) or a generic type
/// is `None`.
fn inherent(symbol: &str) -> Option<Cow<'_, str>> {
    let Some(rest) = symbol.strip_prefix('<') else {
        return Some(Cow::Borrowed(symbol));
    };
    let (ty, method) = rest.split_once('>')?;
    if ty.contains(['<', ' ']) || !method.starts_with("::") {
        return None;
    }
    Some(Cow::Owned(format!("{ty}{method}")))
}

/// `path` without a trailing `::<…>` generic argument list, which is discarded.
fn strip_generic_args(path: &str) -> Option<&str> {
    if !path.ends_with('>') {
        return None;
    }
    let mut depth = 0usize;
    for (i, b) in path.bytes().enumerate().rev() {
        match b {
            b'>' => depth += 1,
            b'<' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return path[..i].strip_suffix("::");
                }
            }
            _ => {}
        }
    }
    None
}

/// `path` without a trailing `::{closure#N}` (v0) or `::{{closure}}` (legacy).
fn strip_closure(path: &str) -> Option<&str> {
    if let Some(enclosing) = path.strip_suffix("::{{closure}}") {
        return Some(enclosing);
    }
    let (enclosing, last) = path.rsplit_once("::")?;
    let n = last.strip_prefix("{closure#")?.strip_suffix('}')?;
    (!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).then_some(enclosing)
}

/// A legacy-mangling hash segment: `h` and 16 hex digits.
fn is_legacy_hash(segment: &str) -> bool {
    segment
        .strip_prefix('h')
        .is_some_and(|h| h.len() == 16 && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

// ---------------------------------------------------------------------------
// The hook
// ---------------------------------------------------------------------------

/// Where the process is, for the event's `stage` ([`encode`]d; 0 is unknown).
static ACTIVE: AtomicU8 = AtomicU8::new(0);
/// Set by the first panic, so a bug hit on every worker thread counts once.
static REPORTED: AtomicBool = AtomicBool::new(false);

/// Record where the process is now: a phase of `convert`, or the stage entered.
pub fn enter(stage: EventStage) {
    ACTIVE.store(encode(stage), Ordering::Relaxed);
}

fn active() -> PanicStage {
    decode(ACTIVE.load(Ordering::Relaxed))
}

/// A stage is `16 +` its index in [`StageKind::ALL`], which is its discriminant.
fn encode(stage: EventStage) -> u8 {
    match stage {
        EventStage::Parse => 1,
        EventStage::Setup => 2,
        EventStage::Preflight => 3,
        EventStage::Finalize => 4,
        EventStage::Stage(kind) => 16 + kind as u8,
    }
}

fn decode(code: u8) -> PanicStage {
    PanicStage::At(match code {
        1 => EventStage::Parse,
        2 => EventStage::Setup,
        3 => EventStage::Preflight,
        4 => EventStage::Finalize,
        16.. => match StageKind::ALL.get(usize::from(code - 16)) {
            Some(kind) => EventStage::Stage(*kind),
            None => return PanicStage::Unknown,
        },
        _ => return PanicStage::Unknown,
    })
}

/// Install the hook, writing into `spool` and starting the helper for consent
/// `generation`. Called once, by `managed::keep_for_process`.
pub fn install(spool: PathBuf, generation: String) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if !REPORTED.swap(true, Ordering::SeqCst) {
            report(&spool, &generation);
        }
        previous(info);
    }));
}

/// Capture, sanitize and publish one event, then start the helper. The payload is
/// never read.
fn report(spool: &Path, generation: &str) {
    // Refuse at the cap before the costly capture; `publish` checks again.
    if matches!(ready_count(spool), Ok(n) if n >= MAX_READY) {
        return;
    }
    let backtrace = std::backtrace::Backtrace::force_capture().to_string();
    let frames = sanitize(&backtrace, env!("CARGO_CRATE_NAME"));
    drop(backtrace);
    let Some(event_id) = EventId::random() else {
        return;
    };
    let event = build(
        event_id,
        super::now_unix_millis(),
        super::cpu_count(),
        active(),
        frames,
    );
    let Some(bytes) = ready_bytes(&event) else {
        return;
    };
    if publish(spool, &event_id.hex(), &bytes).is_ok()
        && matches!(net::endpoint(), Ok(Endpoint::Url(_) | Endpoint::File(_)))
    {
        let _ = managed::spawn_helper(generation);
    }
}

/// Write `.panic-<id>.tmp` (create-new, never through a symlink), sync it, then
/// link it to `panic-ready-<id>.json`, which fails rather than replace an existing
/// file, and unlink the temp. Any failure removes the temp. A spool already holding
/// [`MAX_READY`] ready files is refused.
fn publish(spool: &Path, id: &str, bytes: &[u8]) -> io::Result<()> {
    durable::ensure_private_dir(spool)?;
    if ready_count(spool)? >= MAX_READY {
        return Err(io::Error::other("the spool holds too many panic events"));
    }
    let tmp = spool.join(format!(".panic-{id}.tmp"));
    let result = (|| {
        let mut file = durable::create_new(&tmp)?;
        file.write_all(bytes)?;
        let _ = file.sync_all();
        drop(file);
        fs::hard_link(&tmp, spool.join(format!("panic-ready-{id}.json")))
    })();
    let _ = fs::remove_file(&tmp);
    let _ = durable::sync_dir(spool);
    result
}

/// The spool's panic-ready files, counted up to [`MAX_READY`].
fn ready_count(spool: &Path) -> io::Result<usize> {
    let mut n = 0;
    for dirent in fs::read_dir(spool)? {
        if n >= MAX_READY {
            break;
        }
        if dirent?.file_name().to_str().and_then(spool::classify) == Some(Kind::PanicReady) {
            n += 1;
        }
    }
    Ok(n)
}

/// A panic on demand for the subprocess tests, in debug builds only:
/// `NC_TEST_PANIC=<stage>` panics on entering that stage. With
/// `NC_TEST_PANIC_GATE=<path>` it first creates `<path>.waiting`, then waits (up to
/// a minute) for `<path>` to exist. The message is hostile on purpose: none of it
/// may reach an event.
#[cfg(debug_assertions)]
pub fn test_panic(stage: StageKind) {
    let Some(want) = std::env::var_os("NC_TEST_PANIC") else {
        return;
    };
    if serde_json::to_value(stage)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        != want.to_str().map(str::to_owned)
    {
        return;
    }
    if let Some(gate) = std::env::var_os("NC_TEST_PANIC_GATE") {
        let mut waiting = gate.clone();
        waiting.push(".waiting");
        let _ = fs::write(waiting, b"");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !Path::new(&gate).exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    panic!("NC_TEST_PANIC: /Users/alice/Pictures/roll 12/frame.tif:12:7 at 0x102dc1078\nnext line");
}

#[cfg(not(debug_assertions))]
pub fn test_panic(_: StageKind) {}

#[cfg(test)]
mod tests;

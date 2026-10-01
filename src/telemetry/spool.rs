//! The upload queue on disk: the selected JSONL file that runs append to, and its
//! private sibling spool `.<name>.nc-telemetry-spool` holding everything else
//! (`docs/telemetry-strategy.md`, "Queue drain and crash safety").
//!
//! Lifecycle of a record: appended to the queue file under `queue.lock`; rotated
//! whole into `raw-ready-<id>.jsonl`; projected into immutable request-sized
//! `batch-<id>-<n>.json` files (each one exact request body) plus a
//! `quarantine-<id>.jsonl` for lines that cannot be uploaded; a batch is deleted
//! once a response accounts for every event in it. Every step publishes before it
//! deletes its input, so a crash at any point leaves the record in some file and a
//! later drain resends it (the server deduplicates by `event_id`).
//!
//! A line from another local schema version (a record written before
//! `SCHEMA_VERSION`, or by an older build) is dropped and only counted.

use std::ffi::OsString;
use std::fs::{self, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::consent::Consent;
use super::durable::{self, Held, Mode, Wait};
use super::upload::{UPLOAD_SCHEMA_VERSION, to_upload_event};
use super::{SCHEMA_VERSION, TelemetryEvent};

/// The selected queue file plus everything in its spool.
pub const CAP_BYTES: u64 = 25 << 20;
/// Records older than this are dropped locally.
pub const MAX_AGE_MS: u64 = 30 * 86_400_000;
/// The Worker's limits on one request.
pub const MAX_EVENTS: usize = 100;
pub const MAX_BODY_BYTES: usize = 262_144;

const SPOOL_SUFFIX: &str = ".nc-telemetry-spool";
const QUEUE_LOCK: &str = "queue.lock";
const DRAIN_LOCK: &str = "drain.lock";
const STATUS: &str = "status.json";
/// A quarantined line is kept up to this many bytes.
const QUARANTINE_LINE_LIMIT: usize = 16 * 1024;

/// `<parent>/.<name>.nc-telemetry-spool` for the queue file `<parent>/<name>`.
pub fn spool_for(queue: &Path) -> Option<PathBuf> {
    let name = queue.file_name()?;
    let mut spool = OsString::from(".");
    spool.push(name);
    spool.push(SPOOL_SUFFIX);
    Some(queue.with_file_name(spool))
}

/// A spool entry this module recognizes; any other name is left alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Raw,
    Batch,
    Quarantine,
    /// Written by the panic hook (`telemetry/panic-hook`); kept, counted and purged.
    PanicReady,
    PanicTemp,
    Status,
    Lock,
    /// An unfinished publication of ours; its source still exists.
    Temp,
}

impl Kind {
    /// Whether the entry holds telemetry records.
    pub fn holds_data(self) -> bool {
        matches!(
            self,
            Kind::Raw | Kind::Batch | Kind::Quarantine | Kind::PanicReady | Kind::PanicTemp
        )
    }
}

fn id_like(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase() || b == b'-' || b == b'.')
}

pub fn classify(name: &str) -> Option<Kind> {
    let between = |prefix: &str, suffix: &str| {
        name.strip_prefix(prefix)
            .and_then(|r| r.strip_suffix(suffix))
            .is_some_and(id_like)
    };
    if name == QUEUE_LOCK || name == DRAIN_LOCK {
        Some(Kind::Lock)
    } else if name == STATUS {
        Some(Kind::Status)
    } else if between("raw-ready-", ".jsonl") {
        Some(Kind::Raw)
    } else if between("batch-", ".json") {
        Some(Kind::Batch)
    } else if between("quarantine-", ".jsonl") {
        Some(Kind::Quarantine)
    } else if between("panic-ready-", ".json") {
        Some(Kind::PanicReady)
    } else if between(".panic-", ".tmp") {
        Some(Kind::PanicTemp)
    } else if [".raw-", ".batch-", ".quarantine-", ".status"]
        .iter()
        .any(|p| between(p, ".tmp"))
    {
        Some(Kind::Temp)
    } else {
        None
    }
}

#[derive(Debug)]
pub struct Entry {
    pub name: String,
    pub kind: Kind,
    pub len: u64,
    pub modified: SystemTime,
}

/// Local counters and the last upload outcome, reported by `hanten telemetry status`.
/// Never uploaded.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Status {
    pub accepted: u64,
    pub duplicate: u64,
    pub rejected: u64,
    pub quarantined: u64,
    pub dropped_other_schema: u64,
    pub expired: u64,
    pub dropped_over_cap: u64,
    pub discarded_temps: u64,
    pub last_success_ms: Option<u64>,
    pub last_error: Option<LastError>,
    pub consecutive_failures: u32,
    pub next_attempt_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LastError {
    pub at_ms: u64,
    /// A fixed label (`network`, `timeout`, `http_503`, …), never response text.
    pub kind: String,
}

/// What is queued, for `status`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Usage {
    pub queue_bytes: u64,
    pub raw_files: u64,
    pub batches: u64,
    pub batch_events: u64,
    pub quarantine_records: u64,
    pub panic_ready: u64,
    pub total_bytes: u64,
}

/// One queue: the selected file and its spool.
#[derive(Clone, Debug)]
pub struct Queue {
    pub file: PathBuf,
    pub spool: PathBuf,
}

impl Queue {
    pub fn of(consent: &Consent) -> Self {
        Self {
            file: consent.queue.clone(),
            spool: consent.spool.clone(),
        }
    }

    /// The queue at `file` (absolute).
    pub fn at(file: PathBuf) -> Option<Self> {
        let spool = spool_for(&file)?;
        Some(Self { file, spool })
    }

    fn parent(&self) -> &Path {
        self.file.parent().unwrap_or(Path::new("."))
    }

    /// The spool directory, its locks and the queue file, created if missing.
    pub fn skeleton(&self) -> io::Result<()> {
        durable::ensure_private_dir(&self.spool)?;
        drop(self.lock(QUEUE_LOCK, Some(std::time::Duration::ZERO))?);
        drop(self.lock(DRAIN_LOCK, Some(std::time::Duration::ZERO))?);
        if durable::regular_or_missing(&self.file)?.is_none() {
            self.recreate()?;
        }
        Ok(())
    }

    fn lock(&self, name: &str, wait: Wait) -> io::Result<Option<Held>> {
        durable::ensure_private_dir(&self.spool)?;
        durable::lock(&self.spool.join(name), Mode::Exclusive, wait)
    }

    /// Held for every append and rotation.
    pub fn queue_lock(&self, wait: Wait) -> io::Result<Option<Held>> {
        self.lock(QUEUE_LOCK, wait)
    }

    /// Held by the one drainer.
    pub fn drain_lock(&self, wait: Wait) -> io::Result<Option<Held>> {
        self.lock(DRAIN_LOCK, wait)
    }

    /// Append one event line; `false` when the queue file is at the cap and the line
    /// was dropped. The caller holds the queue lock.
    pub fn append(&self, line: &str) -> io::Result<bool> {
        if durable::regular_or_missing(&self.file)?.is_some_and(|m| m.len() >= CAP_BYTES) {
            return Ok(false);
        }
        let mut buf = String::with_capacity(line.len() + 1);
        buf.push_str(line);
        buf.push('\n');
        use std::io::Write;
        durable::open_append(&self.file)?.write_all(buf.as_bytes())?;
        Ok(true)
    }

    fn recreate(&self) -> io::Result<()> {
        match durable::create_new(&self.file) {
            Ok(file) => file.sync_all()?,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                durable::regular_or_missing(&self.file)?;
            }
            Err(e) => return Err(e),
        }
        durable::sync_dir(self.parent())
    }

    /// Move a non-empty queue file into the spool and recreate it empty; recreate a
    /// missing one. The caller holds the queue lock.
    pub fn rotate(&self) -> io::Result<()> {
        match durable::regular_or_missing(&self.file)? {
            None => self.recreate(),
            Some(meta) if meta.len() == 0 => Ok(()),
            Some(_) => {
                durable::ensure_private_dir(&self.spool)?;
                fs::OpenOptions::new()
                    .append(true)
                    .open(&self.file)?
                    .sync_all()?;
                let name = format!("raw-ready-{}.jsonl", durable::random_hex()?);
                fs::rename(&self.file, self.spool.join(name))?;
                durable::sync_dir(self.parent())?;
                durable::sync_dir(&self.spool)?;
                self.recreate()
            }
        }
    }

    /// Every recognized spool entry, sorted by name. A recognized name that is not
    /// a safe regular file fails closed.
    pub fn entries(&self) -> io::Result<Vec<Entry>> {
        if !durable::dir_or_missing(&self.spool)? {
            return Ok(Vec::new());
        }
        let mut entries = Vec::new();
        for dirent in fs::read_dir(&self.spool)? {
            let dirent = dirent?;
            let Some(name) = dirent.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(kind) = classify(&name) else {
                continue;
            };
            let Some(meta) = durable::regular_or_missing(&self.spool.join(&name))? else {
                continue;
            };
            entries.push(entry(name, kind, &meta));
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    /// Remove unfinished publications; their sources still exist.
    pub fn reconcile(&self, status: &mut Status) -> io::Result<()> {
        for e in self.entries()? {
            if e.kind == Kind::Temp {
                durable::remove(&self.spool, &e.name)?;
                status.discarded_temps += 1;
            }
        }
        Ok(())
    }

    /// Project `raw` into batches and a quarantine file, then delete it. Names derive
    /// from the raw file's, so re-projecting after a crash rewrites nothing.
    pub fn project_raw(&self, raw: &str, now_ms: u64, status: &mut Status) -> io::Result<()> {
        let id = raw
            .strip_prefix("raw-ready-")
            .and_then(|r| r.strip_suffix(".jsonl"))
            .unwrap_or(raw);
        let bytes = match durable::read_capped(&self.spool.join(raw), CAP_BYTES + (1 << 20)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                // Larger than any queue the cap allows: not ours to parse.
                status.dropped_over_cap += 1;
                return durable::remove(&self.spool, raw);
            }
            Err(e) => return Err(e),
        };
        let projected = project(&bytes, now_ms);
        let (bodies, oversized) = chunk(&projected.events);
        let mut quarantine = projected.quarantine;
        quarantine.extend(oversized.into_iter().map(|e| record("oversized", &e)));
        if !quarantine.is_empty() {
            self.publish_once(&format!("quarantine-{id}.jsonl"), &lines(&quarantine))?;
        }
        for (i, body) in bodies.iter().enumerate() {
            self.publish_once(&format!("batch-{id}-{i:04}.json"), body.as_bytes())?;
        }
        durable::remove(&self.spool, raw)?;
        status.quarantined += quarantine.len() as u64;
        status.dropped_other_schema += projected.other_schema;
        status.expired += projected.expired;
        Ok(())
    }

    fn publish_once(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        if durable::regular_or_missing(&self.spool.join(name))?.is_some() {
            return Ok(());
        }
        self.publish(name, bytes)
    }

    fn publish(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let tmp = format!(".{name}.{}.tmp", durable::random_hex()?);
        durable::publish(&self.spool, &tmp, name, bytes)
    }

    /// Quarantine `records` (already JSON objects) in a new file.
    pub fn quarantine(&self, records: &[String]) -> io::Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let name = format!("quarantine-{}.jsonl", durable::random_hex()?);
        self.publish(&name, &lines(records))
    }

    pub fn read_batch(&self, name: &str) -> io::Result<String> {
        let bytes = durable::read_capped(&self.spool.join(name), MAX_BODY_BYTES as u64)?;
        String::from_utf8(bytes).map_err(|_| io::Error::other("a batch is not UTF-8"))
    }

    pub fn remove(&self, name: &str) -> io::Result<()> {
        durable::remove(&self.spool, name)
    }

    /// Drop batches and quarantine past [`MAX_AGE_MS`], then the oldest quarantine
    /// and batches until the queue fits in [`CAP_BYTES`].
    pub fn enforce_limits(&self, now: SystemTime, status: &mut Status) -> io::Result<()> {
        let age_ms = |e: &Entry| {
            now.duration_since(e.modified)
                .map_or(0, |d| d.as_millis() as u64)
        };
        let mut kept = Vec::new();
        for e in self.entries()? {
            let droppable = matches!(e.kind, Kind::Batch | Kind::Quarantine);
            if droppable && age_ms(&e) > MAX_AGE_MS {
                status.expired += self.records_in(&e);
                durable::remove(&self.spool, &e.name)?;
            } else {
                kept.push(e);
            }
        }
        let queue_bytes = durable::regular_or_missing(&self.file)?.map_or(0, |m| m.len());
        let mut total: u64 = queue_bytes + kept.iter().map(|e| e.len).sum::<u64>();
        // Quarantine goes first, then the oldest batches.
        let mut victims: Vec<&Entry> = kept
            .iter()
            .filter(|e| matches!(e.kind, Kind::Batch | Kind::Quarantine))
            .collect();
        victims.sort_by_key(|e| (e.kind == Kind::Batch, e.modified));
        for e in victims {
            if total <= CAP_BYTES {
                break;
            }
            status.dropped_over_cap += self.records_in(e);
            durable::remove(&self.spool, &e.name)?;
            total -= e.len;
        }
        Ok(())
    }

    /// Records in a batch (its events) or a quarantine file (its lines); 0 if unreadable.
    fn records_in(&self, e: &Entry) -> u64 {
        let Ok(bytes) = durable::read_capped(&self.spool.join(&e.name), CAP_BYTES) else {
            return 0;
        };
        match e.kind {
            Kind::Batch => batch_ids(&bytes).map_or(0, |ids| ids.len() as u64),
            _ => bytes
                .split(|b| *b == b'\n')
                .filter(|l| !l.is_empty())
                .count() as u64,
        }
    }

    pub fn usage(&self) -> io::Result<Usage> {
        let queue_bytes = durable::regular_or_missing(&self.file)?.map_or(0, |m| m.len());
        let mut usage = Usage {
            queue_bytes,
            total_bytes: queue_bytes,
            ..Usage::default()
        };
        for e in self.entries()? {
            if e.kind.holds_data() {
                usage.total_bytes += e.len;
            }
            match e.kind {
                Kind::Raw => usage.raw_files += 1,
                Kind::Batch => {
                    usage.batches += 1;
                    usage.batch_events += self.records_in(&e);
                }
                Kind::Quarantine => usage.quarantine_records += self.records_in(&e),
                Kind::PanicReady => usage.panic_ready += 1,
                _ => {}
            }
        }
        Ok(usage)
    }

    /// The names of whatever still holds telemetry: a non-empty queue file and every
    /// data entry. Empty means a retarget may leave this queue. Run [`reconcile`]
    /// first, so a temp of ours does not count.
    ///
    /// [`reconcile`]: Queue::reconcile
    pub fn holdings(&self) -> io::Result<Vec<String>> {
        let mut held = Vec::new();
        if durable::regular_or_missing(&self.file)?.is_some_and(|m| m.len() > 0) {
            held.push(self.file.display().to_string());
        }
        for e in self.entries()? {
            if e.kind.holds_data() {
                held.push(e.name);
            }
        }
        Ok(held)
    }

    /// Remove every recognized record, temp and the status, then replace the queue
    /// file with an empty one. The spool and its lock files stay. The caller holds
    /// every lock purge needs.
    pub fn purge(&self) -> io::Result<()> {
        for e in self.entries()? {
            if e.kind != Kind::Lock {
                durable::remove(&self.spool, &e.name)?;
            }
        }
        let name = self
            .file
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| io::Error::other("the queue path has no UTF-8 file name"))?;
        let tmp = format!(".{name}.nc-telemetry-empty.{}.tmp", durable::random_hex()?);
        durable::publish(self.parent(), &tmp, name, b"")
    }

    pub fn read_status(&self) -> Status {
        durable::read_capped(&self.spool.join(STATUS), 64 * 1024)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn write_status(&self, status: &Status) -> io::Result<()> {
        let bytes = serde_json::to_vec(status).map_err(io::Error::other)?;
        self.publish(STATUS, &bytes)
    }

    pub fn remove_status(&self) -> io::Result<()> {
        durable::remove(&self.spool, STATUS)
    }

    /// The request bodies a drain would send now, in order, without writing
    /// anything: pending batches as stored, then the projection of each raw file and
    /// of the queue file's complete lines.
    pub fn preview(&self, now_ms: u64) -> io::Result<Vec<String>> {
        let entries = self.entries()?;
        let mut bodies = Vec::new();
        for e in entries.iter().filter(|e| e.kind == Kind::Batch) {
            bodies.push(self.read_batch(&e.name)?);
        }
        let mut sources = Vec::new();
        for e in entries.iter().filter(|e| e.kind == Kind::Raw) {
            sources.push(durable::read_capped(
                &self.spool.join(&e.name),
                CAP_BYTES + (1 << 20),
            )?);
        }
        if durable::regular_or_missing(&self.file)?.is_some() {
            let mut live = durable::read_capped(&self.file, CAP_BYTES + (1 << 20))?;
            // A line still being written is not a record yet.
            let complete = live.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
            live.truncate(complete);
            sources.push(live);
        }
        for bytes in sources {
            bodies.extend(chunk(&project(&bytes, now_ms).events).0);
        }
        Ok(bodies)
    }
}

fn entry(name: String, kind: Kind, meta: &Metadata) -> Entry {
    Entry {
        name,
        kind,
        len: meta.len(),
        modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
    }
}

fn lines(records: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for r in records {
        out.extend_from_slice(r.as_bytes());
        out.push(b'\n');
    }
    out
}

/// A quarantine record: why, and what (an event object or the raw line as text).
pub fn record(reason: &str, payload: &str) -> String {
    let value = serde_json::from_str::<serde_json::Value>(payload).unwrap_or_else(|_| {
        let mut end = payload.len().min(QUARANTINE_LINE_LIMIT);
        while !payload.is_char_boundary(end) {
            end -= 1;
        }
        serde_json::Value::String(payload[..end].to_owned())
    });
    serde_json::json!({ "reason": reason, "record": value }).to_string()
}

/// What a queue file's lines come to.
#[derive(Debug, Default)]
pub struct Projected {
    /// Upload events, serialized, in line order.
    pub events: Vec<String>,
    /// Quarantine records.
    pub quarantine: Vec<String>,
    /// Lines of another local schema version: dropped.
    pub other_schema: u64,
    /// Lines older than [`MAX_AGE_MS`]: dropped.
    pub expired: u64,
}

/// Project every non-blank line. Pure apart from `now_ms`.
pub fn project(bytes: &[u8], now_ms: u64) -> Projected {
    let mut out = Projected::default();
    for line in bytes.split(|b| *b == b'\n') {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let text = String::from_utf8_lossy(line);
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(line) else {
            out.quarantine.push(record("malformed", &text));
            continue;
        };
        if value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            != Some(u64::from(SCHEMA_VERSION))
        {
            out.other_schema += 1;
            continue;
        }
        let event: TelemetryEvent = match serde_json::from_value(value) {
            Ok(event) => event,
            Err(_) => {
                out.quarantine.push(record("malformed", &text));
                continue;
            }
        };
        if now_ms.saturating_sub(event.timestamp_ms) > MAX_AGE_MS {
            out.expired += 1;
            continue;
        }
        match to_upload_event(&event).map(|u| serde_json::to_string(&u)) {
            Ok(Ok(json)) => out.events.push(json),
            Ok(Err(_)) => out.quarantine.push(record("malformed", &text)),
            Err(why) => out
                .quarantine
                .push(record(&format!("not_uploadable:{why:?}"), &text)),
        }
    }
    out
}

const BODY_PREFIX: &str = "{\"upload_schema_version\":";
const EVENTS_KEY: &str = ",\"events\":[";
const BODY_SUFFIX: &str = "]}";

/// Group serialized events, in order, into request bodies of at most
/// [`MAX_EVENTS`] events and [`MAX_BODY_BYTES`] bytes. An event too large for any
/// request comes back on its own.
pub fn chunk(events: &[String]) -> (Vec<String>, Vec<String>) {
    let head = format!("{BODY_PREFIX}{UPLOAD_SCHEMA_VERSION}{EVENTS_KEY}");
    let empty = head.len() + BODY_SUFFIX.len();
    let mut bodies = Vec::new();
    let mut oversized = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut size = empty;
    let flush = |current: &mut Vec<&str>, bodies: &mut Vec<String>| {
        if !current.is_empty() {
            bodies.push(format!("{head}{}{BODY_SUFFIX}", current.join(",")));
            current.clear();
        }
    };
    for event in events {
        if empty + event.len() > MAX_BODY_BYTES {
            oversized.push(event.clone());
            continue;
        }
        let added = event.len() + usize::from(!current.is_empty());
        if current.len() == MAX_EVENTS || size + added > MAX_BODY_BYTES {
            flush(&mut current, &mut bodies);
            size = empty;
        }
        size += event.len() + usize::from(!current.is_empty());
        current.push(event);
    }
    flush(&mut current, &mut bodies);
    (bodies, oversized)
}

/// The `event_id`s of a request body, in order; `None` if it is not one.
pub fn batch_ids(body: &[u8]) -> Option<Vec<String>> {
    #[derive(Deserialize)]
    struct Body {
        upload_schema_version: u32,
        events: Vec<Id>,
    }
    #[derive(Deserialize)]
    struct Id {
        event_id: String,
    }
    let body: Body = serde_json::from_slice(body).ok()?;
    (body.upload_schema_version == UPLOAD_SCHEMA_VERSION && !body.events.is_empty())
        .then(|| body.events.into_iter().map(|e| e.event_id).collect())
}

#[cfg(test)]
mod tests;

//! Persistent upload consent: one versioned record in the platform config
//! directory naming the selected queue (the active JSONL), its private spool, and
//! a random generation, plus the cross-process locks beside it
//! (`docs/telemetry-strategy.md`, "Consent and user controls").
//!
//! The record is read fresh every time and fails closed: missing means no
//! consent, and a torn, unknown-version, symlinked, foreign-owned or inconsistent
//! record is an error that every caller treats as "not active". Inactive consent
//! keeps its generation and paths so `purge` and re-enable target them exactly.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::durable::{self, Held, Mode, Wait};
use super::spool;

/// The record's own format. An unknown one fails closed.
pub const CONSENT_FORMAT: u32 = 1;

/// The upload field manifest (`contracts/telemetry/upload-v1/README.md`) this build
/// asks consent for. Bump it when the manifest grows; consent given to an older
/// version then reads as inactive until the user enables again. A manifest that
/// sends strictly less may keep it.
pub const MANIFEST_VERSION: u32 = 1;

const RECORD: &str = "telemetry-consent.json";
const GATE: &str = "telemetry-consent.gate";
const REQUEST_LEASE: &str = "telemetry-request.lease";
const COLLECTION_LEASE: &str = "telemetry-collection.lease";
/// A consent record is a few hundred bytes.
const RECORD_LIMIT: u64 = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Active,
    Inactive,
}

/// A validated consent record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Consent {
    pub state: State,
    pub manifest_version: u32,
    /// 32 lowercase hex characters, fresh at every activation.
    pub generation: String,
    /// The selected queue: absolute, its parent canonical.
    pub queue: PathBuf,
    /// `spool::spool_for(queue)`.
    pub spool: PathBuf,
}

impl Consent {
    /// Fresh active consent for `queue` (already normalized).
    pub fn activate(queue: PathBuf) -> io::Result<Self> {
        let spool = spool::spool_for(&queue)
            .ok_or_else(|| io::Error::other("the queue path has no file name"))?;
        Ok(Self {
            state: State::Active,
            manifest_version: MANIFEST_VERSION,
            generation: durable::random_hex()?,
            queue,
            spool,
        })
    }

    /// Active, and for the manifest this build uploads.
    pub fn is_active(&self) -> bool {
        self.state == State::Active && self.manifest_version == MANIFEST_VERSION
    }

    /// The same generation and paths: what a captured snapshot must still match.
    pub fn same_target(&self, other: &Consent) -> bool {
        self.generation == other.generation
            && self.queue == other.queue
            && self.spool == other.spool
    }

    pub fn deactivated(&self) -> Self {
        Self {
            state: State::Inactive,
            ..self.clone()
        }
    }
}

/// Why a consent record could not be used.
#[derive(Debug)]
pub struct ConsentError {
    message: String,
    /// The record itself is at fault, not the queue or spool it names.
    pub in_record: bool,
}

impl fmt::Display for ConsentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    consent_format: u32,
    manifest_version: u32,
    state: State,
    generation: String,
    queue: String,
    spool: String,
}

/// Where the consent record and its locks live.
#[derive(Clone, Debug)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// `$XDG_CONFIG_HOME/nc`, else `%APPDATA%\nc` (Windows), else `$HOME/.config/nc`.
    /// None in unit tests, which must never see the developer's own consent.
    pub fn locate() -> Option<Self> {
        if cfg!(test) {
            return None;
        }
        let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
        let appdata = if cfg!(windows) { env("APPDATA") } else { None };
        resolve_config_dir(env("XDG_CONFIG_HOME"), appdata, env("HOME")).map(Self::at)
    }

    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn record_path(&self) -> PathBuf {
        self.dir.join(RECORD)
    }

    /// Whether anything is at the record's path — the cheap check that keeps a
    /// user who never enabled from paying for (or creating) any lock file.
    pub fn exists(&self) -> bool {
        fs::symlink_metadata(self.record_path()).is_ok()
    }

    /// The validated record; `Ok(None)` when there is none.
    pub fn read(&self) -> Result<Option<Consent>, ConsentError> {
        let path = self.record_path();
        let fail = |what: String| ConsentError {
            message: format!("consent record {}: {what}", path.display()),
            in_record: true,
        };
        let unsafe_target = |e: io::Error| ConsentError {
            message: format!("the consented queue is unusable: {e}"),
            in_record: false,
        };
        match durable::regular_or_missing(&path) {
            Ok(None) => return Ok(None),
            Ok(Some(_)) => {}
            Err(e) => return Err(fail(e.to_string())),
        }
        let bytes = durable::read_capped(&path, RECORD_LIMIT).map_err(|e| fail(e.to_string()))?;
        let record: Record =
            serde_json::from_slice(&bytes).map_err(|e| fail(format!("unreadable ({e})")))?;
        if record.consent_format != CONSENT_FORMAT {
            return Err(fail(format!(
                "unknown format {} (this build reads {CONSENT_FORMAT})",
                record.consent_format
            )));
        }
        if record.manifest_version > MANIFEST_VERSION {
            return Err(fail(format!(
                "written for upload manifest {} by a newer build",
                record.manifest_version
            )));
        }
        if super::EventId::parse(&record.generation).is_none() {
            return Err(fail("bad generation".into()));
        }
        let queue = PathBuf::from(&record.queue);
        let spool = PathBuf::from(&record.spool);
        if !queue.is_absolute() || spool::spool_for(&queue).as_ref() != Some(&spool) {
            return Err(fail("the queue and spool paths do not match".into()));
        }
        durable::regular_or_missing(&queue).map_err(unsafe_target)?;
        durable::dir_or_missing(&spool).map_err(unsafe_target)?;
        Ok(Some(Consent {
            state: record.state,
            manifest_version: record.manifest_version,
            generation: record.generation,
            queue,
            spool,
        }))
    }

    /// Publish `consent` atomically and durably.
    pub fn publish(&self, consent: &Consent) -> io::Result<()> {
        let utf8 = |p: &Path| {
            p.to_str()
                .map(str::to_owned)
                .ok_or_else(|| io::Error::other(format!("{} is not UTF-8", p.display())))
        };
        let record = Record {
            consent_format: CONSENT_FORMAT,
            manifest_version: consent.manifest_version,
            state: consent.state,
            generation: consent.generation.clone(),
            queue: utf8(&consent.queue)?,
            spool: utf8(&consent.spool)?,
        };
        let mut bytes = serde_json::to_vec_pretty(&record).map_err(io::Error::other)?;
        bytes.push(b'\n');
        self.ensure_dir()?;
        // A crashed publish's temp; every caller holds the gate, so none is live.
        let stale = format!(".{RECORD}.");
        for e in fs::read_dir(&self.dir)?.flatten() {
            let name = e.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(&stale) && name.ends_with(".tmp") {
                let _ = fs::remove_file(e.path());
            }
        }
        let tmp = format!(".{RECORD}.{}.tmp", durable::random_hex()?);
        durable::publish(&self.dir, &tmp, RECORD, &bytes)
    }

    fn ensure_dir(&self) -> io::Result<()> {
        fs::create_dir_all(&self.dir)
    }

    fn lock(&self, name: &str, mode: Mode, wait: Wait) -> io::Result<Option<Held>> {
        self.ensure_dir()?;
        durable::lock(&self.dir.join(name), mode, wait)
    }

    /// The short gate that serializes taking a lease with validating consent.
    pub fn gate(&self, wait: Wait) -> io::Result<Option<Held>> {
        self.lock(GATE, Mode::Exclusive, wait)
    }

    /// Shared by every network request; exclusive for `disable` and `enable`.
    pub fn request_lease(&self, mode: Mode, wait: Wait) -> io::Result<Option<Held>> {
        self.lock(REQUEST_LEASE, mode, wait)
    }

    /// Shared by every consented `convert` for its whole run; exclusive for
    /// `purge`, re-enable and retarget, which wait out those runs.
    pub fn collection_lease(&self, mode: Mode, wait: Wait) -> io::Result<Option<Held>> {
        self.lock(COLLECTION_LEASE, mode, wait)
    }
}

/// Pure config-dir precedence, highest first; `None` when every source is absent.
fn resolve_config_dir(
    xdg_config_home: Option<std::ffi::OsString>,
    appdata: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    if let Some(x) = xdg_config_home {
        return Some(PathBuf::from(x).join("nc"));
    }
    if let Some(a) = appdata {
        return Some(PathBuf::from(a).join("nc"));
    }
    Some(PathBuf::from(home?).join(".config").join("nc"))
}

/// A queue path as consent stores it: absolute, with a canonical parent (which must
/// exist) and the file name kept, so a symlinked file is seen as one.
pub fn normalize_queue(path: &Path) -> io::Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let (Some(parent), Some(name)) = (absolute.parent(), absolute.file_name()) else {
        return Err(io::Error::other(format!(
            "{} does not name a file",
            path.display()
        )));
    };
    Ok(fs::canonicalize(parent)?.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nc-consent-{tag}-{}-{}",
            std::process::id(),
            durable::random_hex().unwrap()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn config_dir_precedence() {
        use std::ffi::OsString;
        let os = |s: &str| Some(OsString::from(s));
        assert_eq!(
            resolve_config_dir(os("/xdg"), os("C:/app"), os("/home")),
            Some(PathBuf::from("/xdg/nc"))
        );
        assert_eq!(
            resolve_config_dir(None, os("C:/app"), os("/home")),
            Some(PathBuf::from("C:/app/nc"))
        );
        assert_eq!(
            resolve_config_dir(None, None, os("/home")),
            Some(PathBuf::from("/home/.config/nc"))
        );
        assert_eq!(resolve_config_dir(None, None, None), None);
    }

    #[test]
    fn a_published_record_reads_back() {
        let dir = scratch("roundtrip");
        let store = Store::at(dir.join("cfg"));
        assert!(store.read().unwrap().is_none());
        assert!(!store.exists());
        let queue = normalize_queue(&dir.join("t.jsonl")).unwrap();
        let consent = Consent::activate(queue).unwrap();
        store.publish(&consent).unwrap();
        let back = store.read().unwrap().unwrap();
        assert_eq!(back, consent);
        assert!(back.is_active());
        store.publish(&consent.deactivated()).unwrap();
        let back = store.read().unwrap().unwrap();
        assert!(!back.is_active());
        assert!(back.same_target(&consent));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bad_record_fails_closed() {
        let dir = scratch("bad");
        let store = Store::at(dir.join("cfg"));
        let queue = normalize_queue(&dir.join("t.jsonl")).unwrap();
        let good = Consent::activate(queue).unwrap();
        store.publish(&good).unwrap();
        let text = fs::read_to_string(store.record_path()).unwrap();
        let cases = [
            text[..text.len() / 2].to_string(),
            text.replace("\"consent_format\": 1", "\"consent_format\": 2"),
            text.replace("\"manifest_version\": 1", "\"manifest_version\": 9"),
            text.replace(".t.jsonl.nc-telemetry-spool", ".other.nc-telemetry-spool"),
            text.replace(&good.generation, "zz"),
            text.replace("\"state\"", "\"extra\": 1, \"state\""),
        ];
        for case in cases {
            fs::write(store.record_path(), &case).unwrap();
            assert!(store.read().is_err(), "accepted:\n{case}");
        }
        #[cfg(unix)]
        {
            fs::write(dir.join("real.json"), &text).unwrap();
            fs::remove_file(store.record_path()).unwrap();
            std::os::unix::fs::symlink(dir.join("real.json"), store.record_path()).unwrap();
            assert!(store.read().is_err(), "a symlinked record");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn an_unsafe_queue_is_not_the_records_fault() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("queue");
        let store = Store::at(dir.join("cfg"));
        let queue = normalize_queue(&dir.join("t.jsonl")).unwrap();
        store
            .publish(&Consent::activate(queue.clone()).unwrap())
            .unwrap();
        fs::write(&queue, b"").unwrap();
        fs::set_permissions(&queue, fs::Permissions::from_mode(0o666)).unwrap();
        let err = store.read().unwrap_err();
        assert!(!err.in_record, "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bad_record_is_the_records_fault() {
        let dir = scratch("record");
        let store = Store::at(dir.join("cfg"));
        fs::create_dir_all(dir.join("cfg")).unwrap();
        fs::write(store.record_path(), b"{").unwrap();
        assert!(store.read().unwrap_err().in_record);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_older_manifest_reads_as_inactive() {
        let consent = Consent {
            manifest_version: MANIFEST_VERSION - 1,
            ..Consent::activate(PathBuf::from("/q/t.jsonl")).unwrap()
        };
        assert!(!consent.is_active());
    }
}

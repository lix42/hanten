//! `hanten telemetry …`: enable, disable, purge, status, preview, flush, and the
//! hidden `upload-once` the detached helper runs.
//!
//! Lock order, never reversed (`docs/telemetry-strategy.md`, "Purge
//! synchronization"):
//!
//! - disable: gate → exclusive request lease;
//! - first enable: gate → exclusive request lease;
//! - inactive same-queue enable: exclusive collection lease → drain lock → gate →
//!   exclusive request lease, all released before the one replacement helper starts;
//! - inactive retarget A→B and purge: exclusive collection lease → A's drain lock →
//!   gate → A's queue lock.
//!
//! Conversions take shared collection lease → gate (briefly) → queue lock; a
//! drainer takes drain lock → queue lock (released) → gate → shared request lease.
//! The panic hook takes none: it writes into the spool inside its process's shared
//! collection lease (`telemetry::panic`).

use std::io::{self, BufRead, IsTerminal};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;

use super::consent::{self, Consent, State, Store};
use super::drain::{self, Trigger};
use super::durable::{self, Held, Mode};
use super::managed::{disabled_by_env, spawn_helper};
use super::net::{self, Endpoint};
use super::spool::{PanicWriters, Queue};
use crate::stdio;
use crate::types::{NcError, Result};

/// A line on stderr (`crate::stdio`, which survives a closed pipe).
macro_rules! say {
    ($($arg:tt)*) => {
        stdio::stderr_line(format_args!($($arg)*))
    };
}

fn other(e: impl std::fmt::Display) -> NcError {
    NcError::Other(format!("telemetry: {e}"))
}

fn store() -> Result<Store> {
    Store::locate().ok_or_else(|| {
        NcError::Usage(
            "telemetry: no config directory for the consent record (set XDG_CONFIG_HOME or HOME)"
                .into(),
        )
    })
}

fn read(store: &Store) -> Result<Option<Consent>> {
    store.read().map_err(|e| {
        let remedy = if e.in_record {
            "remove the record to start over"
        } else {
            "make it a regular file and directory of yours, not writable by every user"
        };
        other(format!("{e}. Telemetry stays off until then: {remedy}"))
    })
}

/// Take a blocking lock, saying what it waits for when it is not free at once.
fn wait_for(
    take: impl Fn(Option<Duration>) -> io::Result<Option<Held>>,
    what: &str,
) -> Result<Held> {
    if let Some(held) = take(Some(Duration::from_millis(50))).map_err(other)? {
        return Ok(held);
    }
    say!("telemetry: waiting for {what}…");
    take(None)
        .map_err(other)?
        .ok_or_else(|| other("a blocking lock returned nothing"))
}

fn confirm(yes: bool, question: &str) -> Result<()> {
    if yes {
        return Ok(());
    }
    if !io::stdin().is_terminal() {
        return Err(NcError::Usage(format!(
            "telemetry: {question} needs confirmation: pass --yes when not on a terminal"
        )));
    }
    say!("{question} [y/N]");
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer).map_err(other)?;
    if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        Ok(())
    } else {
        Err(NcError::Usage(
            "telemetry: not confirmed; nothing changed".into(),
        ))
    }
}

/// The queue `enable` would select: `--queue`, else `NC_TELEMETRY_LOG` or the default
/// log path. Its parent directory is created so the path can be normalized.
fn selected_queue(queue: Option<&Path>) -> Result<PathBuf> {
    let requested = match queue {
        Some(p) => p.to_path_buf(),
        None => super::default_log_path().ok_or_else(|| {
            NcError::Usage(
                "telemetry: no data directory for the queue: pass --queue or set NC_TELEMETRY_LOG"
                    .into(),
            )
        })?,
    };
    if let Some(parent) = requested.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(other)?;
    }
    let path = consent::normalize_queue(&requested)
        .map_err(|e| NcError::Usage(format!("telemetry: queue {}: {e}", requested.display())))?;
    if path.to_str().is_none() {
        return Err(NcError::Usage(format!(
            "telemetry: queue {} must be a UTF-8 path",
            path.display()
        )));
    }
    durable::regular_or_missing(&path)
        .map_err(|e| NcError::Usage(format!("telemetry: queue {e}")))?;
    Ok(path)
}

const MANIFEST: &str = "\
What is uploaded, per `convert` run (the field manifest is
contracts/telemetry/upload-v1/README.md):
  - a random ID per event (never reused, never linked to you or this machine)
  - the day (not the time), Hanten version, OS, CPU architecture, CPU-count bucket
  - where the run ended and how: success or failure, exit code, error category
  - coarse quality signals: warning-count and clipped-fraction buckets, NaN yes/no
  - per-stage timings in whole milliseconds
  - image format, megapixels to 0.1, input-size bucket, bit depth, IR present
  - output encoding, film-base source kind (region/explicit), IR exported
  - if a run panics: the stage, and up to 32 Hanten function names
Never uploaded: file or folder names, pixels, recipe or parameter values,
exact sizes, dimensions or timestamps, error text, any user/machine/install ID.";

/// `YYYY-MM-DD` (UTC) of a Unix-epoch millisecond time.
fn day(ms: u64) -> String {
    // Howard Hinnant's civil-from-days.
    let z = (ms / 86_400_000) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn notice(queue: &Queue, endpoint: &Endpoint) {
    let usage = queue.usage().unwrap_or_default();
    let (records, span) = queue.summary();
    say!("{MANIFEST}");
    say!("");
    if *endpoint == Endpoint::Url(net::DEFAULT_ENDPOINT.into()) {
        say!("Backend: {endpoint}, Hanten's ingestion service on Cloudflare, which");
        say!("keeps events 180 days.");
    } else {
        say!("Backend: {endpoint} (not Hanten's default service).");
    }
    say!("Disabling or purging later cannot delete events already uploaded: they");
    say!("carry no identity to find them by, and expire after 180 days.");
    say!("");
    say!("Queue: {}", queue.file.display());
    say!("Spool: {}", queue.spool.display());
    match span {
        Some((first, last)) => say!(
            "Already queued: {records} records, {} to {} ({} bytes).",
            day(first),
            day(last),
            usage.total_bytes
        ),
        None => say!(
            "Already queued: {records} records ({} bytes).",
            usage.total_bytes
        ),
    }
    say!("The queue is capped at 25 MiB and records expire after 30 days.");
    say!("");
    say!("Once enabled, every `hanten convert` records one event to this queue and a");
    say!("short-lived background process uploads it. Uploading empties the queue file:");
    say!("it stops being a local history (`--telemetry` to another NC_TELEMETRY_LOG");
    say!("keeps one). NC_TELEMETRY=0 turns collection off for one process;");
    say!("`hanten telemetry disable` turns it off.");
}

/// `hanten telemetry enable [--queue PATH] [--yes]`.
pub fn enable(queue: Option<&Path>, yes: bool) -> Result<()> {
    let endpoint = net::endpoint().map_err(NcError::Usage)?;
    if endpoint == Endpoint::None {
        return Err(NcError::Usage(
            "telemetry: this build uploads nowhere (its endpoint is `none`); nothing to enable"
                .into(),
        ));
    }
    let store = store()?;
    let path = selected_queue(queue)?;
    let target = Queue::at(path.clone()).ok_or_else(|| other("the queue has no file name"))?;
    let current = read(&store)?;
    match &current {
        Some(c) if c.is_active() && c.queue == path => {
            say!(
                "telemetry: upload is already enabled for {}",
                path.display()
            );
            return Ok(());
        }
        // Active for this build or for an older one (a stale manifest).
        Some(c) if c.state == State::Active && c.queue != path => {
            return Err(NcError::Usage(format!(
                "telemetry: upload is enabled for {}; run `hanten telemetry disable` before \
                 selecting another queue",
                c.queue.display()
            )));
        }
        _ => {}
    }
    notice(&target, &endpoint);
    confirm(yes, "Enable telemetry upload?")?;
    let fresh = Consent::activate(path).map_err(other)?;
    match current {
        None => first_enable(&store, &target, &fresh)?,
        Some(old) if old.queue == fresh.queue => reenable(&store, &old, &target, &fresh)?,
        Some(old) => retarget(&store, &old, &target, &fresh)?,
    }
    if let Err(e) = spawn_helper(&fresh.generation) {
        say!("hanten: warning: telemetry: could not start the upload helper: {e}");
    }
    say!("telemetry: upload enabled for {}", fresh.queue.display());
    Ok(())
}

fn changed() -> NcError {
    other("consent changed while enabling; nothing was published, run the command again")
}

fn first_enable(store: &Store, target: &Queue, fresh: &Consent) -> Result<()> {
    target.skeleton().map_err(other)?;
    let _gate = wait_for(|w| store.gate(w), "the consent gate")?;
    let _requests = wait_for(
        |w| store.request_lease(Mode::Exclusive, w),
        "an upload in flight",
    )?;
    if read(store)?.is_some() {
        return Err(changed());
    }
    store.publish(fresh).map_err(other)
}

/// Inactive same-queue enable: wait out the old generation's conversions and its
/// helper before publishing a fresh generation for the same queue.
fn reenable(store: &Store, old: &Consent, target: &Queue, fresh: &Consent) -> Result<()> {
    let _collection = wait_for(
        |w| store.collection_lease(Mode::Exclusive, w),
        "running conversions to finish",
    )?;
    let _drain = wait_for(|w| target.drain_lock(w), "a running upload helper")?;
    let _gate = wait_for(|w| store.gate(w), "the consent gate")?;
    let _requests = wait_for(
        |w| store.request_lease(Mode::Exclusive, w),
        "an upload in flight",
    )?;
    if read(store)?.is_none_or(|c| c.is_active() || !c.same_target(old)) {
        return Err(changed());
    }
    target.skeleton().map_err(other)?;
    store.publish(fresh).map_err(other)
}

/// Inactive A→B: allowed only once A holds nothing, so no record is stranded.
fn retarget(store: &Store, old: &Consent, target: &Queue, fresh: &Consent) -> Result<()> {
    let from = Queue::of(old);
    let _collection = wait_for(
        |w| store.collection_lease(Mode::Exclusive, w),
        "running conversions to finish",
    )?;
    let _drain = wait_for(|w| from.drain_lock(w), "a running upload helper")?;
    let _gate = wait_for(|w| store.gate(w), "the consent gate")?;
    if read(store)?.is_none_or(|c| c.state == State::Active || !c.same_target(old)) {
        return Err(changed());
    }
    let _queue = wait_for(|w| from.queue_lock(w), "the queue lock")?;
    let mut status = from.read_status();
    from.reconcile(&mut status, PanicWriters::Excluded)
        .map_err(other)?;
    let held = from.holdings().map_err(other)?;
    if !held.is_empty() {
        return Err(NcError::Usage(format!(
            "telemetry: the previous queue {} still holds telemetry ({}). To keep it, run \
             `hanten telemetry enable --queue {}` and let it drain (or `hanten telemetry \
             flush`), then disable; to drop it, run `hanten telemetry purge`. Then select \
             the new queue again",
            from.file.display(),
            held.join(", "),
            from.file.display()
        )));
    }
    target.skeleton().map_err(other)?;
    from.remove_status().map_err(other)?;
    store.publish(fresh).map_err(other)
}

/// `hanten telemetry disable`.
pub fn disable() -> Result<()> {
    let store = store()?;
    match read(&store)? {
        None => {
            say!("telemetry: upload was never enabled");
            return Ok(());
        }
        Some(c) if c.state == State::Inactive => {
            say!("telemetry: upload is already disabled");
            return Ok(());
        }
        Some(_) => {}
    }
    let _gate = wait_for(|w| store.gate(w), "the consent gate")?;
    let _requests = wait_for(
        |w| store.request_lease(Mode::Exclusive, w),
        "an upload in flight (at most 10 s)",
    )?;
    if let Some(c) = read(&store)?.filter(|c| c.state == State::Active) {
        store.publish(&c.deactivated()).map_err(other)?;
    }
    say!(
        "telemetry: upload disabled. Queued records are kept; `hanten telemetry purge` \
         deletes them"
    );
    Ok(())
}

/// `hanten telemetry purge [--yes]`: inactive consent only.
pub fn purge(yes: bool) -> Result<()> {
    let store = store()?;
    let old = match read(&store)? {
        None => {
            return Err(NcError::Usage(
                "telemetry: nothing to purge: upload was never enabled".into(),
            ));
        }
        Some(c) if c.state == State::Active => {
            return Err(NcError::Usage(
                "telemetry: run `hanten telemetry disable` before purging".into(),
            ));
        }
        Some(c) => c,
    };
    let queue = Queue::of(&old);
    confirm(
        yes,
        &format!(
            "Delete every queued telemetry record in {} and its spool?",
            queue.file.display()
        ),
    )?;
    let _collection = wait_for(
        |w| store.collection_lease(Mode::Exclusive, w),
        "running conversions to finish",
    )?;
    let _drain = wait_for(|w| queue.drain_lock(w), "a running upload helper")?;
    let _gate = wait_for(|w| store.gate(w), "the consent gate")?;
    if read(&store)?.is_none_or(|c| c.state == State::Active || !c.same_target(&old)) {
        return Err(other("consent changed while purging; nothing was deleted"));
    }
    let _queue = wait_for(|w| queue.queue_lock(w), "the queue lock")?;
    queue.purge().map_err(other)?;
    say!("telemetry: purged {}", queue.file.display());
    Ok(())
}

/// `hanten telemetry status`: JSON on stdout.
pub fn status() -> Result<()> {
    let store = store()?;
    let endpoint = net::endpoint();
    let mut out = json!({
        "endpoint": match &endpoint { Ok(e) => e.to_string(), Err(e) => e.clone() },
        "disabled_by_env": disabled_by_env(),
        "consent_record": store.record_path(),
    });
    match store.read() {
        Err(e) => out["consent"] = json!({ "state": "unreadable", "error": e.to_string() }),
        Ok(None) => out["consent"] = json!({ "state": "never_enabled" }),
        Ok(Some(c)) => {
            let state = if c.is_active() {
                "active"
            } else if c.state == State::Active {
                "needs_reconsent"
            } else {
                "inactive"
            };
            out["consent"] = json!({
                "state": state,
                "manifest_version": c.manifest_version,
                "queue": c.queue,
                "spool": c.spool,
            });
            let queue = Queue::of(&c);
            out["queue"] = match queue.usage() {
                Ok(u) => serde_json::to_value(u).unwrap_or_default(),
                Err(e) => json!({ "error": e.to_string() }),
            };
            out["upload"] = serde_json::to_value(queue.read_status()).unwrap_or_default();
        }
    }
    print_json(&out)
}

fn print_json(value: &serde_json::Value) -> Result<()> {
    let text = serde_json::to_string_pretty(value).map_err(other)?;
    stdio::stdout_line(&text).map(drop).map_err(other)
}

/// `hanten telemetry preview`: the request bodies a drain would send now, one per
/// line, exactly as sent. Reads the consented queue, else the default log.
pub fn preview() -> Result<()> {
    let store = store()?;
    let queue = match read(&store)? {
        Some(c) => Queue::of(&c),
        None => {
            let path = super::default_log_path()
                .ok_or_else(|| NcError::Usage("telemetry: no queue to preview".into()))?;
            let path = consent::normalize_queue(&path).unwrap_or(path);
            Queue::at(path).ok_or_else(|| other("the queue has no file name"))?
        }
    };
    let bodies = queue.preview(super::now_unix_millis()).map_err(other)?;
    for body in bodies {
        if stdio::stdout_line(&body).map_err(other)? == stdio::Delivery::ReaderGone {
            break;
        }
    }
    Ok(())
}

/// `hanten telemetry flush`: drain now, in the foreground; JSON report on stdout.
pub fn flush() -> Result<()> {
    let store = store()?;
    let consent = read(&store)?.filter(Consent::is_active).ok_or_else(|| {
        NcError::Usage("telemetry: upload is not enabled (`hanten telemetry enable`)".into())
    })?;
    if disabled_by_env() {
        return Err(NcError::Usage(
            "telemetry: NC_TELEMETRY=0 forbids uploading from this process".into(),
        ));
    }
    let endpoint = net::endpoint().map_err(NcError::Usage)?;
    if endpoint == Endpoint::None {
        return Err(NcError::Usage(
            "telemetry: this build uploads nowhere (its endpoint is `none`)".into(),
        ));
    }
    let report = drain::drain(&store, &consent, &endpoint, Trigger::Flush);
    print_json(&serde_json::to_value(&report).map_err(other)?)?;
    match (report.error, report.stopped) {
        (Some(e), _) => Err(other(format!("upload failed: {e}"))),
        (None, Some(why)) => Err(other(format!("upload stopped: {why}"))),
        (None, None) => Ok(()),
    }
}

/// Hidden `upload-once --generation G`: the detached helper. Silent, always exit 0.
pub fn upload_once(generation: &str) -> Result<()> {
    if disabled_by_env() {
        return Ok(());
    }
    let (Some(store), Ok(endpoint)) = (Store::locate(), net::endpoint()) else {
        return Ok(());
    };
    if endpoint == Endpoint::None {
        return Ok(());
    }
    if let Ok(Some(consent)) = store.read()
        && consent.is_active()
        && consent.generation == generation
    {
        drain::drain(&store, &consent, &endpoint, Trigger::Background);
    }
    Ok(())
}

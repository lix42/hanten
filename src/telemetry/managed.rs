//! Automatic collection under persistent consent, for one `convert` process.
//!
//! [`begin`] takes the shared collection lease and, inside the consent gate,
//! captures an immutable snapshot of active consent; [`keep_for_process`] holds both
//! until the process ends and installs the panic hook (`telemetry::panic`), so
//! `purge` and retarget wait it out while `disable` does not. The
//! event goes to the snapshot's queue, and the helper is launched only if the
//! same consent is still active then (the panic hook launches it unchecked; its
//! drain checks before every request). Every wait is bounded and every failure is
//! silent: a conversion never waits long on, or fails for, telemetry.

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use super::consent::{self, Consent, Store};
use super::durable::{Held, Mode};
use super::net::{self, Endpoint};
use super::spool::Queue;

const LEASE_WAIT: Duration = Duration::from_secs(1);
const GATE_WAIT: Duration = Duration::from_secs(1);
const QUEUE_WAIT: Duration = Duration::from_secs(2);

/// `NC_TELEMETRY=0`: no automatic collection, helper or upload in this process.
/// Explicit `--telemetry` / `--telemetry-file` still write locally.
pub fn disabled_by_env() -> bool {
    std::env::var_os("NC_TELEMETRY").is_some_and(|v| v == "0")
}

/// Active consent captured at invocation start, with the shared collection lease.
pub struct Snapshot {
    store: Store,
    consent: Consent,
    _lease: Held,
}

/// The snapshot for this invocation, or `None` when nothing should be collected.
pub fn begin() -> Option<Snapshot> {
    if disabled_by_env() {
        return None;
    }
    let store = Store::locate()?;
    // A user who never enabled pays for no lock and creates no file.
    if !store.exists() {
        return None;
    }
    let lease = store
        .collection_lease(Mode::Shared, Some(LEASE_WAIT))
        .ok()??;
    let consent = {
        let _gate = store.gate(Some(GATE_WAIT)).ok()??;
        store.read().ok()??
    };
    consent.is_active().then_some(Snapshot {
        store,
        consent,
        _lease: lease,
    })
}

/// Keep `snapshot`, with its lease, until the process ends, and report this
/// process's panics into its spool. A second call in one process keeps the first.
pub fn keep_for_process(snapshot: Snapshot) -> &'static Snapshot {
    static KEPT: OnceLock<Snapshot> = OnceLock::new();
    let mut fresh = false;
    let kept = KEPT.get_or_init(|| {
        fresh = true;
        snapshot
    });
    if fresh {
        super::panic::install(kept.consent.spool.clone(), kept.consent.generation.clone());
    }
    kept
}

impl Snapshot {
    pub fn queue(&self) -> &Path {
        &self.consent.queue
    }

    /// Append the event line to the consented queue.
    pub fn append(&self, line: &str) -> io::Result<()> {
        append_locked(&Queue::of(&self.consent), line)
    }

    /// Launch the detached upload helper if the captured consent is still the
    /// active one and this build has an endpoint. Silent on any failure.
    pub fn launch_helper(&self) {
        if !matches!(net::endpoint(), Ok(Endpoint::Url(_) | Endpoint::File(_))) {
            return;
        }
        let still = {
            let Ok(Some(_gate)) = self.store.gate(Some(GATE_WAIT)) else {
                return;
            };
            self.store.read().ok().flatten()
        };
        if still.is_some_and(|c| c.is_active() && c.same_target(&self.consent)) {
            let _ = spawn_helper(&self.consent.generation);
        }
    }
}

fn append_locked(queue: &Queue, line: &str) -> io::Result<()> {
    let Some(_lock) = queue.queue_lock(Some(QUEUE_WAIT))? else {
        return Err(io::Error::other("the telemetry queue stayed locked"));
    };
    if queue.append(line)? {
        Ok(())
    } else {
        Err(io::Error::other("the telemetry queue is full"))
    }
}

/// The consent (active or not) whose queue is `path`, with its store.
fn selecting(path: &Path) -> Option<(Consent, Store)> {
    Store::locate()
        .filter(Store::exists)
        .and_then(|store| Some((store.read().ok()??, store)))
        .filter(|(c, _)| consent::normalize_queue(path).is_ok_and(|p| p == c.queue))
}

/// Whether consent, active or not, selected `path` as its upload queue.
pub fn is_selected_queue(path: &Path) -> bool {
    selecting(path).is_some()
}

/// Append an explicit `--telemetry` line to `path`. When `path` is the queue that
/// consent (active or not) selected, the append synchronizes like a managed one —
/// shared collection lease, then the path re-checked under the gate, then the
/// queue lock — so `purge` and retarget never race it; under `NC_TELEMETRY=0` it is
/// refused, since a line there would be uploaded later. Elsewhere it is a plain
/// append.
pub fn append_explicit(path: &Path, line: &str) -> io::Result<()> {
    let Some((consent, store)) = selecting(path) else {
        return super::append_jsonl(path, line);
    };
    if disabled_by_env() {
        return Err(io::Error::other(
            "it is the upload queue and NC_TELEMETRY=0 forbids uploading this run; \
             set NC_TELEMETRY_LOG to another file to keep a local log",
        ));
    }
    let busy = || io::Error::other("the telemetry queue stayed locked");
    let _lease = store
        .collection_lease(Mode::Shared, Some(LEASE_WAIT))?
        .ok_or_else(busy)?;
    let still_selected = {
        let _gate = store.gate(Some(GATE_WAIT))?.ok_or_else(busy)?;
        store
            .read()
            .ok()
            .flatten()
            .is_some_and(|c| c.queue == consent.queue)
    };
    if !still_selected {
        return super::append_jsonl(path, line);
    }
    append_locked(&Queue::of(&consent), line)
}

/// Start `hanten telemetry upload-once` detached, with no standard streams.
/// `NC_TELEMETRY_HELPER=0` starts none (diagnostic: uploads then wait for `flush`).
pub fn spawn_helper(generation: &str) -> io::Result<()> {
    if disabled_by_env() || std::env::var_os("NC_TELEMETRY_HELPER").is_some_and(|v| v == "0") {
        return Ok(());
    }
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.args(["telemetry", "upload-once", "--generation", generation])
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Out of the terminal's process group, so its Ctrl-C does not reach it.
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn().map(drop)
}

/// Record a `convert` that clap refused, under persistent consent only: a usage
/// failure in the `parse` stage. No argument text is read.
pub fn record_parse_failure(started: Instant) {
    let Some(snapshot) = begin() else {
        return;
    };
    let Some(event_id) = super::EventId::random() else {
        return;
    };
    let event = super::build_event(super::EventInputs {
        outcome: super::Outcome::Failure {
            stage: super::EventStage::Parse,
            kind: super::ErrorKind::Usage,
            exit_code: 2,
        },
        event_id,
        timestamp_ms: super::now_unix_millis(),
        cpu_count: super::cpu_count(),
        timings: super::TimingInfo {
            total: started.elapsed().as_secs_f64() * 1000.0,
            ..super::TimingInfo::default()
        },
        image: None,
        conversion: None,
        loss: None,
        warnings: 0,
    });
    let Ok(line) = serde_json::to_string(&event) else {
        return;
    };
    if snapshot.append(&line).is_ok() {
        snapshot.launch_helper();
    }
}

//! One drain of the consented queue: rotate, reconcile, project, enforce the local
//! limits, then send each batch under a shared request lease whose consent check
//! runs inside the gate, so `disable` either waits for the request or wins before
//! it starts (`docs/telemetry-strategy.md`, "Consent and user controls").
//!
//! The background helper (`upload-once`) and `flush` share this; only `flush`
//! waits for a busy drainer and ignores the retry back-off.

use std::collections::HashMap;
use std::io;
use std::time::{Duration, SystemTime};

use serde::Serialize;

use super::consent::{Consent, Store};
use super::durable;
use super::durable::Mode;
use super::managed::disabled_by_env;
use super::net::{self, Acknowledgement, Endpoint, Sent};
use super::now_unix_millis;
use super::spool::{self, Kind, LastError, PanicWriters, Queue, Status};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// The detached helper: gives up if another drain runs, honours the back-off.
    Background,
    /// `hanten telemetry flush`.
    Flush,
}

/// What a drain did, printed by `flush`.
#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub requests: u64,
    pub accepted: u64,
    pub duplicate: u64,
    pub rejected: u64,
    pub quarantined: u64,
    pub pending_batches: u64,
    /// Why the drain stopped early, when it did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped: Option<String>,
    /// The upload failure that stopped it (a fixed label).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Retry back-off after `failures` consecutive failures: 1 min doubling to 6 h.
fn backoff(failures: u32) -> Duration {
    let minutes = 1u64 << failures.saturating_sub(1).min(9);
    Duration::from_secs((minutes * 60).min(6 * 3600))
}

/// Drain the queue `captured` names. I/O failures end the drain and are reported;
/// none is ever raised to a conversion.
///
/// A background helper that finds the drain lock busy exits, so every holder looks
/// at the queue file and the panic-ready files again *after* releasing it and drains
/// once more if either refilled: an event written while the lock was held is then
/// taken by whoever released last. A pass consumes every ready file it saw or fails,
/// and a failure ends the loop.
pub fn drain(store: &Store, captured: &Consent, endpoint: &Endpoint, trigger: Trigger) -> Report {
    let mut report = Report::default();
    let queue = Queue::of(captured);
    let mut wait = match trigger {
        Trigger::Background => Some(Duration::ZERO),
        Trigger::Flush => None,
    };
    let mut first = true;
    loop {
        let held = match queue.drain_lock(wait) {
            Ok(Some(held)) => held,
            // A drainer re-checks after its own release; a consent change starts a
            // helper of its own.
            Ok(None) if !first => return report,
            Ok(None) => {
                report.stopped = Some("another upload is draining this queue".into());
                return report;
            }
            Err(e) => {
                report.error = Some(format!("io: {e}"));
                return report;
            }
        };
        locked_pass(store, captured, endpoint, trigger, &queue, &mut report);
        hold_for_diagnosis();
        drop(held);
        let refilled = durable::regular_or_missing(&queue.file)
            .ok()
            .flatten()
            .is_some_and(|m| m.len() > 0)
            || queue
                .entries()
                .is_ok_and(|e| e.iter().any(|e| e.kind == Kind::PanicReady));
        if report.error.is_some() || report.stopped.is_some() || !refilled {
            return report;
        }
        first = false;
        wait = Some(Duration::ZERO);
    }
}

/// One pass under the drain lock, its status written once.
fn locked_pass(
    store: &Store,
    captured: &Consent,
    endpoint: &Endpoint,
    trigger: Trigger,
    queue: &Queue,
    report: &mut Report,
) {
    let mut status = queue.read_status();
    if let Err(e) = pass(
        store,
        captured,
        endpoint,
        trigger,
        queue,
        &mut status,
        report,
    ) {
        report.error = Some(format!("io: {e}"));
        status.last_error = Some(LastError {
            at_ms: now_unix_millis(),
            kind: "io".into(),
        });
    }
    let _ = queue.write_status(&status);
}

/// Diagnostic `NC_TELEMETRY_DRAIN_HOLD_MS`: keep the drain lock that long after a
/// pass, so a test can append while it is held.
fn hold_for_diagnosis() {
    if let Some(ms) = std::env::var("NC_TELEMETRY_DRAIN_HOLD_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        std::thread::sleep(Duration::from_millis(ms));
    }
}

/// Rotate, project, enforce the limits, then send every batch.
fn pass(
    store: &Store,
    captured: &Consent,
    endpoint: &Endpoint,
    trigger: Trigger,
    queue: &Queue,
    status: &mut Status,
    report: &mut Report,
) -> io::Result<()> {
    let now = SystemTime::now();
    let now_ms = now_unix_millis();
    {
        let Some(_queue) = queue.queue_lock(Some(Duration::from_secs(10)))? else {
            return Err(io::Error::other("the queue lock stayed busy"));
        };
        queue.rotate()?;
    }
    queue.reconcile(status, PanicWriters::MayRun(now))?;
    for e in queue.entries()? {
        if e.kind == Kind::Raw {
            queue.project_raw(&e.name, now_ms, status)?;
        }
    }
    queue.project_panics(now_ms, status)?;
    queue.enforce_limits(now, status)?;

    let batches: Vec<String> = queue
        .entries()?
        .into_iter()
        .filter(|e| e.kind == Kind::Batch)
        .map(|e| e.name)
        .collect();
    report.pending_batches = batches.len() as u64;
    if trigger == Trigger::Background && status.next_attempt_ms.is_some_and(|t| t > now_ms) {
        report.stopped = Some("backing off after a failed upload".into());
        return Ok(());
    }
    for name in batches {
        // A read error keeps the batch for the next drain; only an unparsable body
        // is quarantined, and then whole.
        let body = queue.read_batch(&name)?;
        let Some(ids) = spool::batch_ids(body.as_bytes()) else {
            queue.quarantine(&[spool::record("corrupt_batch", &body)])?;
            queue.remove(&name)?;
            report.pending_batches -= 1;
            continue;
        };
        let sent = {
            let lease = {
                let _gate = store.gate(None)?;
                let lease = store.request_lease(Mode::Shared, None)?;
                let current = store.read().ok().flatten();
                let allowed = current.is_some_and(|c| c.is_active() && c.same_target(captured))
                    && !disabled_by_env();
                if !allowed {
                    report.stopped = Some("upload consent changed".into());
                    break;
                }
                lease
            };
            report.requests += 1;
            let sent = net::send(endpoint, &body);
            drop(lease);
            sent
        };
        let now_ms = now_unix_millis();
        match sent {
            Sent::Acknowledged(ack) if accounts_for(&ack, &ids) => {
                let rejected = rejected_records(&ack, &body);
                queue.quarantine(&rejected)?;
                queue.remove(&name)?;
                report.accepted += ack.accepted.len() as u64;
                report.duplicate += ack.duplicate.len() as u64;
                report.rejected += ack.rejected.len() as u64;
                status.accepted += ack.accepted.len() as u64;
                status.duplicate += ack.duplicate.len() as u64;
                status.rejected += ack.rejected.len() as u64;
                report.quarantined += rejected.len() as u64;
                status.quarantined += rejected.len() as u64;
                status.last_success_ms = Some(now_ms);
                status.consecutive_failures = 0;
                status.next_attempt_ms = None;
            }
            Sent::Acknowledged(_) => {
                fail(status, report, now_ms, "bad_response", None);
                break;
            }
            Sent::Malformed => {
                let records = event_records("http_400", &body);
                queue.quarantine(&records)?;
                queue.remove(&name)?;
                report.quarantined += records.len() as u64;
                status.quarantined += records.len() as u64;
            }
            Sent::Retry {
                kind,
                retry_after_s,
            } => {
                fail(status, report, now_ms, &kind, retry_after_s);
                break;
            }
        }
        report.pending_batches -= 1;
    }
    Ok(())
}

fn fail(
    status: &mut Status,
    report: &mut Report,
    now_ms: u64,
    kind: &str,
    retry_after_s: Option<u64>,
) {
    status.consecutive_failures = status.consecutive_failures.saturating_add(1);
    let wait = backoff(status.consecutive_failures)
        .max(Duration::from_secs(retry_after_s.unwrap_or(0).min(86_400)));
    status.next_attempt_ms = Some(now_ms + wait.as_millis() as u64);
    status.last_error = Some(LastError {
        at_ms: now_ms,
        kind: kind.into(),
    });
    report.error = Some(kind.into());
}

/// Whether `ack` names every ID of the batch exactly once, and nothing else.
pub fn accounts_for(ack: &Acknowledgement, ids: &[String]) -> bool {
    if ack.upload_schema_version != super::upload::UPLOAD_SCHEMA_VERSION {
        return false;
    }
    let mut answered: Vec<&str> = ack
        .accepted
        .iter()
        .chain(&ack.duplicate)
        .map(String::as_str)
        .chain(ack.rejected.iter().map(|r| r.event_id.as_str()))
        .collect();
    let mut sent: Vec<&str> = ids.iter().map(String::as_str).collect();
    answered.sort_unstable();
    sent.sort_unstable();
    answered == sent
}

/// The batch's events by ID.
fn events_by_id(body: &str) -> HashMap<String, serde_json::Value> {
    let events = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("events").and_then(|e| e.as_array()).cloned())
        .unwrap_or_default();
    events
        .into_iter()
        .filter_map(|e| Some((e.get("event_id")?.as_str()?.to_owned(), e)))
        .collect()
}

fn rejected_records(ack: &Acknowledgement, body: &str) -> Vec<String> {
    let events = events_by_id(body);
    ack.rejected
        .iter()
        .map(|r| {
            serde_json::json!({
                "reason": "rejected",
                "code": r.code,
                "record": events.get(&r.event_id).cloned().unwrap_or_default(),
            })
            .to_string()
        })
        .collect()
}

fn event_records(reason: &str, body: &str) -> Vec<String> {
    let events = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("events").and_then(|e| e.as_array()).cloned())
        .unwrap_or_default();
    events
        .into_iter()
        .map(|e| serde_json::json!({ "reason": reason, "record": e }).to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::net::Rejection;

    fn ack(accepted: &[&str], duplicate: &[&str], rejected: &[&str]) -> Acknowledgement {
        Acknowledgement {
            upload_schema_version: 1,
            accepted: accepted.iter().map(|s| s.to_string()).collect(),
            duplicate: duplicate.iter().map(|s| s.to_string()).collect(),
            rejected: rejected
                .iter()
                .map(|s| Rejection {
                    event_id: s.to_string(),
                    code: "invalid_field".into(),
                })
                .collect(),
        }
    }

    #[test]
    fn a_response_must_account_for_every_id_exactly_once() {
        let ids: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        assert!(accounts_for(&ack(&["a"], &["b"], &["c"]), &ids));
        assert!(accounts_for(&ack(&["c", "b", "a"], &[], &[]), &ids));
        assert!(!accounts_for(&ack(&["a", "b"], &[], &[]), &ids), "missing");
        assert!(
            !accounts_for(&ack(&["a", "b", "c", "d"], &[], &[]), &ids),
            "extra"
        );
        assert!(
            !accounts_for(&ack(&["a", "b"], &["b"], &["c"]), &ids),
            "twice"
        );
        let mut wrong = ack(&["a", "b", "c"], &[], &[]);
        wrong.upload_schema_version = 2;
        assert!(!accounts_for(&wrong, &ids), "version");
    }

    #[test]
    fn backoff_doubles_to_a_ceiling() {
        assert_eq!(backoff(1), Duration::from_secs(60));
        assert_eq!(backoff(2), Duration::from_secs(120));
        assert_eq!(backoff(10), Duration::from_secs(6 * 3600));
        assert_eq!(backoff(u32::MAX), Duration::from_secs(6 * 3600));
    }
}

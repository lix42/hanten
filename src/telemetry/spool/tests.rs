use super::*;
use crate::telemetry::{
    ErrorKind, EventId, EventInputs, EventStage, Outcome, TimingInfo, build_event, durable,
};

const DAY_MS: u64 = 86_400_000;
const NOW_MS: u64 = 20_000 * DAY_MS;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nc-spool-{tag}-{}-{}",
        std::process::id(),
        durable::random_hex().unwrap()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn queue_in(dir: &Path) -> Queue {
    Queue::at(dir.join("telemetry.jsonl")).unwrap()
}

/// A current-schema usage failure written at `timestamp_ms`.
fn event_line(timestamp_ms: u64) -> String {
    let event = build_event(EventInputs {
        outcome: Outcome::Failure {
            stage: EventStage::Setup,
            kind: ErrorKind::Usage,
            exit_code: 2,
        },
        event_id: EventId::random().unwrap(),
        timestamp_ms,
        cpu_count: Some(8),
        timings: TimingInfo {
            total: 3.0,
            ..TimingInfo::default()
        },
        image: None,
        conversion: None,
        loss: None,
        warnings: 0,
    });
    serde_json::to_string(&event).unwrap()
}

fn kinds(queue: &Queue) -> Vec<Kind> {
    queue
        .entries()
        .unwrap()
        .into_iter()
        .map(|e| e.kind)
        .collect()
}

#[test]
fn spools_are_hidden_siblings_that_never_collide() {
    assert_eq!(
        spool_for(Path::new("/data/custom.jsonl")),
        Some(PathBuf::from("/data/.custom.jsonl.nc-telemetry-spool"))
    );
    assert_ne!(
        spool_for(Path::new("/data/a.jsonl")),
        spool_for(Path::new("/data/b.jsonl"))
    );
}

#[test]
fn names_classify() {
    let id = "0123456789abcdef0123456789abcdef";
    assert_eq!(classify(&format!("raw-ready-{id}.jsonl")), Some(Kind::Raw));
    assert_eq!(
        classify(&format!("batch-{id}-0003.json")),
        Some(Kind::Batch)
    );
    assert_eq!(
        classify(&format!("quarantine-{id}.jsonl")),
        Some(Kind::Quarantine)
    );
    assert_eq!(
        classify(&format!("panic-ready-{id}.json")),
        Some(Kind::PanicReady)
    );
    assert_eq!(classify(&format!(".panic-{id}.tmp")), Some(Kind::PanicTemp));
    assert_eq!(
        classify(&format!(".batch-{id}-0000.json.{id}.tmp")),
        Some(Kind::Temp)
    );
    assert_eq!(
        classify(&format!(".status.json.{id}.tmp")),
        Some(Kind::Temp)
    );
    assert_eq!(classify("queue.lock"), Some(Kind::Lock));
    assert_eq!(classify("status.json"), Some(Kind::Status));
    for other in [
        "notes.txt",
        "batch-.json",
        "batch-../x.json",
        "raw-ready-A.jsonl",
    ] {
        assert_eq!(classify(other), None, "{other}");
    }
}

#[test]
fn projection_drops_other_schemas_and_expired_lines_and_quarantines_bad_ones() {
    let mut bytes = Vec::new();
    for line in [
        event_line(NOW_MS),
        r#"{"schema_version":9,"timestamp_ms":1}"#.to_string(),
        r#"{"no_version":true}"#.to_string(),
        "{\"schema_version\":11, torn".to_string(),
        r#"{"schema_version":11,"event_id":"x"}"#.to_string(),
        event_line(NOW_MS - 31 * DAY_MS),
        String::new(),
        event_line(NOW_MS - DAY_MS),
    ] {
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
    }
    let p = project(&bytes, NOW_MS);
    assert_eq!(p.events.len(), 2);
    assert_eq!(p.other_schema, 2);
    assert_eq!(p.expired, 1);
    assert_eq!(p.quarantine.len(), 2);
    for q in &p.quarantine {
        let v: serde_json::Value = serde_json::from_str(q).unwrap();
        assert_eq!(v["reason"], "malformed");
    }
}

#[test]
fn chunks_respect_both_request_limits() {
    let small: Vec<String> = (0..250).map(|i| format!("{{\"n\":{i}}}")).collect();
    let (bodies, oversized) = chunk(&small);
    assert!(oversized.is_empty());
    assert_eq!(bodies.len(), 3);
    let counts: Vec<usize> = bodies
        .iter()
        .map(|b| {
            let v: serde_json::Value = serde_json::from_str(b).unwrap();
            assert_eq!(v["upload_schema_version"], 1);
            v["events"].as_array().unwrap().len()
        })
        .collect();
    assert_eq!(counts, [100, 100, 50]);

    let big = format!("{{\"pad\":\"{}\"}}", "x".repeat(100_000));
    let (bodies, oversized) = chunk(&[big.clone(), big.clone(), big.clone()]);
    assert!(oversized.is_empty());
    assert_eq!(
        bodies.len(),
        2,
        "two 100 kB events fit one body, three do not"
    );
    assert!(bodies.iter().all(|b| b.len() <= MAX_BODY_BYTES));

    let huge = format!("{{\"pad\":\"{}\"}}", "x".repeat(MAX_BODY_BYTES));
    let (bodies, oversized) = chunk(&[huge]);
    assert!(bodies.is_empty());
    assert_eq!(oversized.len(), 1);
}

#[test]
fn rotation_and_projection_move_every_record_into_batches() {
    let dir = scratch("rotate");
    let queue = queue_in(&dir);
    queue.skeleton().unwrap();
    let lines: Vec<String> = (0..3).map(|_| event_line(NOW_MS)).collect();
    for l in &lines {
        assert!(queue.append(l).unwrap());
    }
    queue.append("not json").unwrap();
    queue.rotate().unwrap();
    assert_eq!(
        fs::metadata(&queue.file).unwrap().len(),
        0,
        "recreated empty"
    );
    assert_eq!(kinds(&queue), [Kind::Lock, Kind::Lock, Kind::Raw]);

    let mut status = Status::default();
    let raw = queue
        .entries()
        .unwrap()
        .into_iter()
        .find(|e| e.kind == Kind::Raw)
        .unwrap();
    queue.project_raw(&raw.name, NOW_MS, &mut status).unwrap();
    assert_eq!(status.quarantined, 1);
    let entries = queue.entries().unwrap();
    let batch = entries.iter().find(|e| e.kind == Kind::Batch).unwrap();
    assert!(entries.iter().any(|e| e.kind == Kind::Quarantine));
    assert!(!entries.iter().any(|e| e.kind == Kind::Raw));
    let ids = batch_ids(queue.read_batch(&batch.name).unwrap().as_bytes()).unwrap();
    let expected: Vec<String> = lines
        .iter()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            v["event_id"].as_str().unwrap().to_owned()
        })
        .collect();
    assert_eq!(ids, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn reprojecting_after_a_crash_rewrites_nothing() {
    let dir = scratch("reproject");
    let queue = queue_in(&dir);
    queue.skeleton().unwrap();
    queue.append(&event_line(NOW_MS)).unwrap();
    queue.rotate().unwrap();
    let raw = queue
        .entries()
        .unwrap()
        .into_iter()
        .find(|e| e.kind == Kind::Raw)
        .unwrap()
        .name;
    let kept = fs::read(queue.spool.join(&raw)).unwrap();
    let mut status = Status::default();
    queue.project_raw(&raw, NOW_MS, &mut status).unwrap();
    let batch = queue
        .entries()
        .unwrap()
        .into_iter()
        .find(|e| e.kind == Kind::Batch)
        .unwrap()
        .name;
    let first = fs::read(queue.spool.join(&batch)).unwrap();
    // The crash: the raw file is back, as if its deletion never happened.
    fs::write(queue.spool.join(&raw), kept).unwrap();
    queue.project_raw(&raw, NOW_MS, &mut status).unwrap();
    let batches: Vec<_> = queue
        .entries()
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == Kind::Batch)
        .collect();
    assert_eq!(batches.len(), 1);
    assert_eq!(fs::read(queue.spool.join(&batch)).unwrap(), first);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn reconcile_discards_our_temps_and_leaves_other_names() {
    let dir = scratch("reconcile");
    let queue = queue_in(&dir);
    queue.skeleton().unwrap();
    fs::write(queue.spool.join(".batch-x-0000.json.abc.tmp"), b"{").unwrap();
    fs::write(queue.spool.join(".panic-abc.tmp"), b"{").unwrap();
    fs::write(queue.spool.join("notes.txt"), b"mine").unwrap();
    let mut status = Status::default();
    queue.reconcile(&mut status).unwrap();
    assert_eq!(status.discarded_temps, 1);
    assert!(!queue.spool.join(".batch-x-0000.json.abc.tmp").exists());
    assert!(
        queue.spool.join(".panic-abc.tmp").exists(),
        "the panic hook's"
    );
    assert!(queue.spool.join("notes.txt").exists());
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_symlinked_managed_entry_fails_closed() {
    let dir = scratch("symlink");
    let queue = queue_in(&dir);
    queue.skeleton().unwrap();
    fs::write(dir.join("elsewhere"), b"x").unwrap();
    std::os::unix::fs::symlink(dir.join("elsewhere"), queue.spool.join("batch-a-0000.json"))
        .unwrap();
    assert!(queue.entries().is_err());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_cap_drops_quarantine_before_batches() {
    let dir = scratch("cap");
    let queue = queue_in(&dir);
    queue.skeleton().unwrap();
    let pad = "x".repeat(9 << 20);
    fs::write(queue.spool.join("quarantine-a.jsonl"), format!("{pad}\n")).unwrap();
    let body = |n: u32| {
        format!(
            "{{\"upload_schema_version\":1,\"events\":[{{\"event_id\":\"{n}\",\"p\":\"{pad}\"}}]}}"
        )
    };
    fs::write(queue.spool.join("batch-a-0000.json"), body(0)).unwrap();
    fs::write(queue.spool.join("batch-b-0000.json"), body(1)).unwrap();
    let mut status = Status::default();
    queue
        .enforce_limits(SystemTime::now(), &mut status)
        .unwrap();
    // 27 MiB over a 25 MiB cap: the quarantine file alone goes.
    assert_eq!(status.dropped_over_cap, 1);
    assert_eq!(
        kinds(&queue).iter().filter(|k| **k == Kind::Batch).count(),
        2
    );
    assert!(!kinds(&queue).contains(&Kind::Quarantine));
    fs::write(queue.spool.join("batch-c-0000.json"), body(2)).unwrap();
    queue
        .enforce_limits(SystemTime::now(), &mut status)
        .unwrap();
    assert_eq!(status.dropped_over_cap, 2, "then the oldest batch");
    assert_eq!(
        kinds(&queue).iter().filter(|k| **k == Kind::Batch).count(),
        2
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn old_batches_expire() {
    let dir = scratch("expire");
    let queue = queue_in(&dir);
    queue.skeleton().unwrap();
    fs::write(
        queue.spool.join("batch-a-0000.json"),
        r#"{"upload_schema_version":1,"events":[{"event_id":"a"},{"event_id":"b"}]}"#,
    )
    .unwrap();
    let mut status = Status::default();
    let later = SystemTime::now() + std::time::Duration::from_millis(MAX_AGE_MS + DAY_MS);
    queue.enforce_limits(later, &mut status).unwrap();
    assert_eq!(status.expired, 2);
    assert!(!kinds(&queue).contains(&Kind::Batch));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn preview_is_what_projection_writes() {
    let dir = scratch("preview");
    let queue = queue_in(&dir);
    queue.skeleton().unwrap();
    for _ in 0..3 {
        queue.append(&event_line(NOW_MS)).unwrap();
    }
    let before = queue.preview(NOW_MS).unwrap();
    queue.rotate().unwrap();
    let raw = queue
        .entries()
        .unwrap()
        .into_iter()
        .find(|e| e.kind == Kind::Raw)
        .unwrap()
        .name;
    queue
        .project_raw(&raw, NOW_MS, &mut Status::default())
        .unwrap();
    assert_eq!(queue.preview(NOW_MS).unwrap(), before);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_full_queue_file_drops_appends() {
    let dir = scratch("full");
    let queue = queue_in(&dir);
    queue.skeleton().unwrap();
    let file = fs::OpenOptions::new()
        .write(true)
        .open(&queue.file)
        .unwrap();
    file.set_len(CAP_BYTES).unwrap();
    assert!(!queue.append("{}").unwrap());
    assert_eq!(fs::metadata(&queue.file).unwrap().len(), CAP_BYTES);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn purge_keeps_the_spool_and_its_lock_files() {
    let dir = scratch("purge");
    let queue = queue_in(&dir);
    queue.skeleton().unwrap();
    queue.append(&event_line(NOW_MS)).unwrap();
    queue.rotate().unwrap();
    queue.append(&event_line(NOW_MS)).unwrap();
    fs::write(queue.spool.join("panic-ready-a.json"), b"{}").unwrap();
    fs::write(queue.spool.join("notes.txt"), b"mine").unwrap();
    queue.write_status(&Status::default()).unwrap();
    #[cfg(unix)]
    let inode = |name: &str| {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(queue.spool.join(name)).unwrap().ino()
    };
    #[cfg(unix)]
    let locks = (inode("queue.lock"), inode("drain.lock"));
    assert!(!queue.holdings().unwrap().is_empty());
    queue.purge().unwrap();
    assert!(queue.holdings().unwrap().is_empty());
    assert_eq!(kinds(&queue), [Kind::Lock, Kind::Lock]);
    assert_eq!(fs::metadata(&queue.file).unwrap().len(), 0);
    assert!(queue.spool.join("notes.txt").exists());
    #[cfg(unix)]
    assert_eq!(locks, (inode("queue.lock"), inode("drain.lock")));
    let _ = fs::remove_dir_all(&dir);
}

//! Panic reporting (`telemetry/panic-hook`) end to end. The binary panics on demand
//! through `NC_TEST_PANIC`, which only debug builds honour, hence the module's
//! `cfg(debug_assertions)`.

use super::*;

/// A conversion of the fixture that panics on entering `stage`, uploading to
/// `endpoint`.
fn panicking_to(home: &Home, endpoint: &str, stage: &str, name: &str) -> Command {
    let input = fixture("hdr-48bit.tif");
    let out = home.path(name);
    let mut cmd = home.command(
        endpoint,
        &[
            "convert",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.8,0.5,0.3",
            "--report",
            "none",
        ],
    );
    cmd.env("NC_TEST_PANIC", stage);
    cmd
}

/// The same with no endpoint and no helper, so the event stays in the spool.
fn panicking(home: &Home, stage: &str, name: &str) -> Command {
    let mut cmd = panicking_to(home, "none", stage, name);
    cmd.env("NC_TELEMETRY_HELPER", "0");
    cmd
}

/// Run `cmd`, which must end as a panic does: exit 101 and the default hook's
/// message on stderr.
fn panics(cmd: &mut Command) -> Output {
    let out = cmd.output().unwrap();
    assert_eq!(code(&out), 101, "{}", stderr(&out));
    assert!(
        stderr(&out).contains("panicked at") && stderr(&out).contains("NC_TEST_PANIC:"),
        "the default hook still runs: {}",
        stderr(&out)
    );
    out
}

/// Stderr without the thread ID the default hook prints (`thread 'main' (123)`).
fn without_thread_id(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("' (") {
        out.push_str(&rest[..=i]);
        let after = &rest[i + 3..];
        match after.find(')') {
            Some(j) if after[..j].bytes().all(|b| b.is_ascii_digit()) => rest = &after[j + 1..],
            _ => {
                out.push_str(" (");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The parsed panic-ready files, each checked to be one compact line.
fn ready_events(home: &Home) -> Vec<serde_json::Value> {
    spool_names(home)
        .into_iter()
        .filter(|n| n.starts_with("panic-ready-"))
        .map(|n| {
            let text = fs::read_to_string(home.spool().join(&n)).unwrap();
            assert!(text.len() <= 16 * 1024);
            assert_eq!(text.matches('\n').count(), 1, "{n}: one line");
            assert!(text.ends_with('\n'));
            let event: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(
                n,
                format!("panic-ready-{}.json", event["event_id"].as_str().unwrap())
            );
            event
        })
        .collect()
}

fn panic_temps(home: &Home) -> Vec<String> {
    spool_names(home)
        .into_iter()
        .filter(|n| n.starts_with(".panic-"))
        .collect()
}

/// The contract's frame grammar, restated independently of `telemetry::panic`.
fn is_contract_frame(f: &str) -> bool {
    let mut segments = f.split("::");
    f.is_ascii()
        && (2..=192).contains(&f.len())
        && segments.next() == Some("nc")
        && f.split("::").count() <= 16
        && segments.all(|s| {
            s.bytes()
                .next()
                .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
}

#[test]
fn without_consent_a_panic_installs_no_hook_and_writes_nothing() {
    let home = Home::new("panic-noconsent");
    let plain = panics(&mut panicking(&home, "look", "a.tiff"));
    // Explicit local telemetry is not consent: still no hook, no spool.
    let log = home.path("events.jsonl");
    panics(panicking(&home, "look", "b.tiff").args([
        "--telemetry",
        "--telemetry-file",
        log.to_str().unwrap(),
    ]));
    assert!(!home.path("cfg").exists(), "no consent record or lock");
    assert!(!home.spool().exists(), "no panic spool");
    assert!(!log.exists());

    // With consent, the user sees exactly the same panic.
    enable_collect_only(&home);
    let consented = panics(&mut panicking(&home, "look", "c.tiff"));
    assert_eq!(
        without_thread_id(&stderr(&consented)),
        without_thread_id(&stderr(&plain))
    );
    assert_eq!(ready_events(&home).len(), 1);

    // `NC_TELEMETRY=0` opts this run out.
    panics(panicking(&home, "look", "d.tiff").env("NC_TELEMETRY", "0"));
    assert_eq!(ready_events(&home).len(), 1);
}

#[test]
fn under_consent_a_panic_writes_one_sanitized_event_that_uploads() {
    let home = Home::new("panic-consent");
    enable_collect_only(&home);
    panics(&mut panicking(&home, "look", "a.tiff"));
    let events = ready_events(&home);
    assert_eq!(events.len(), 1);
    assert!(panic_temps(&home).is_empty());
    let event = &events[0];
    assert_eq!(event["schema_version"], 11);
    assert_eq!(event["event"], "panic");
    assert_eq!(event["command"], "convert");
    assert_eq!(event["stage"], "look");
    let frames: Vec<&str> = event["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    assert!(frames.len() <= 32);
    assert!(frames.iter().all(|f| is_contract_frame(f)), "{frames:?}");
    assert!(
        frames.contains(&"nc::cli::run_convert"),
        "the panic's nc callers: {frames:?}"
    );
    assert!(
        !frames.iter().any(|f| f.starts_with("nc::telemetry::panic")),
        "not the hook's own: {frames:?}"
    );
    let text = event.to_string();
    for leak in [
        "alice",
        "frame.tif",
        "0x",
        "NC_TEST_PANIC",
        "/",
        ".rs",
        "a.tiff",
    ] {
        assert!(!text.contains(leak), "{leak} in {text}");
    }

    // A panic in another stage names it.
    panics(&mut panicking(&home, "decode", "b.tiff"));
    let mut stages: Vec<String> = ready_events(&home)
        .iter()
        .map(|e| e["stage"].as_str().unwrap().to_owned())
        .collect();
    stages.sort();
    assert_eq!(stages, ["decode", "look"]);
    assert_eq!(home.status()["queue"]["panic_ready"], 2);

    let server = Endpoint::accepting();
    let out = home.run(&server.url, &["telemetry", "flush"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(server.requests(), 1);
    assert_eq!(event_ids(&server.bodies()[0]).len(), 2);
    assert!(ready_events(&home).is_empty());
    assert_eq!(batches(&home), 0);
}

#[test]
fn a_panic_while_the_queue_lock_is_held_does_not_wait_for_it() {
    let home = Home::new("panic-queue-lock");
    enable_collect_only(&home);
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(home.spool().join("queue.lock"))
        .unwrap();
    lock.lock().unwrap();
    let started = Instant::now();
    panics(&mut panicking(&home, "look", "a.tiff"));
    assert!(started.elapsed() < Duration::from_secs(20));
    assert_eq!(ready_events(&home).len(), 1);
    drop(lock);
}

#[cfg(unix)]
#[test]
fn an_unwritable_spool_changes_nothing_the_user_sees() {
    use std::os::unix::fs::PermissionsExt;
    let home = Home::new("panic-unwritable");
    let plain = panics(&mut panicking(&home, "look", "a.tiff"));
    enable_collect_only(&home);
    fs::set_permissions(home.spool(), fs::Permissions::from_mode(0o500)).unwrap();
    let out = panics(&mut panicking(&home, "look", "b.tiff"));
    fs::set_permissions(home.spool(), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        without_thread_id(&stderr(&out)),
        without_thread_id(&stderr(&plain))
    );
    assert!(ready_events(&home).is_empty());
    assert!(panic_temps(&home).is_empty());
}

/// A panicking conversion held at its panic until `gate` exists.
fn held_at_panic(home: &Home, gate: &Path, name: &str) -> Child {
    let child = panicking(home, "look", name)
        .env("NC_TEST_PANIC_GATE", gate)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut waiting = gate.as_os_str().to_owned();
    waiting.push(".waiting");
    let waiting = PathBuf::from(waiting);
    wait_until("the conversion to reach its panic", || waiting.exists());
    child
}

#[test]
fn disable_does_not_wait_for_a_running_conversion_whose_panic_stays_local() {
    let home = Home::new("panic-disable");
    let server = Endpoint::accepting();
    enable_collect_only(&home);
    let gate = home.path("gate");
    let child = held_at_panic(&home, &gate, "a.tiff");
    let started = Instant::now();
    let out = home.run("none", &["telemetry", "disable"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(started.elapsed() < Duration::from_secs(5), "disable waited");
    fs::write(&gate, b"").unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(code(&out), 101, "{}", stderr(&out));
    assert_eq!(ready_events(&home).len(), 1, "published after disable");

    let _ = home.run(&server.url, &["telemetry", "flush"]);
    thread::sleep(Duration::from_millis(300));
    assert_eq!(server.requests(), 0, "nothing sent while inactive");
    assert!(
        !ready_events(&home).is_empty() || batches(&home) == 1,
        "the event is kept"
    );
}

#[test]
fn purge_waits_for_a_conversion_that_may_still_panic() {
    let home = Home::new("panic-purge");
    enable_collect_only(&home);
    let gate = home.path("gate");
    let child = held_at_panic(&home, &gate, "a.tiff");
    assert_eq!(code(&home.run("none", &["telemetry", "disable"])), 0);
    let mut purge = home
        .command("none", &["telemetry", "purge", "--yes"])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(700));
    assert!(purge.try_wait().unwrap().is_none(), "purge did not wait");
    fs::write(&gate, b"").unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(code(&out), 101, "{}", stderr(&out));
    let out = purge.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        ready_events(&home).is_empty(),
        "purge ran after the publish"
    );
    assert!(panic_temps(&home).is_empty());
}

#[test]
fn simultaneous_panics_in_many_processes_publish_distinct_files() {
    let home = Home::new("panic-many");
    enable_collect_only(&home);
    let gates: Vec<PathBuf> = (0..6).map(|i| home.path(&format!("gate-{i}"))).collect();
    let children: Vec<Child> = gates
        .iter()
        .enumerate()
        .map(|(i, gate)| held_at_panic(&home, gate, &format!("{i}.tiff")))
        .collect();
    // All at their panic; release them together.
    for gate in &gates {
        fs::write(gate, b"").unwrap();
    }
    for child in children {
        let out = child.wait_with_output().unwrap();
        assert_eq!(code(&out), 101, "{}", stderr(&out));
    }
    let events = ready_events(&home);
    assert_eq!(events.len(), 6);
    let ids: BTreeSet<&str> = events
        .iter()
        .map(|e| e["event_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 6);
    assert!(panic_temps(&home).is_empty());
}

#[test]
fn a_crashed_panic_write_is_discarded_never_uploaded() {
    let home = Home::new("panic-crashed");
    let server = Endpoint::accepting();
    enable_collect_only(&home);
    let stale = home
        .spool()
        .join(".panic-0123456789abcdef0123456789abcdef.tmp");
    fs::write(&stale, br#"{"schema_version":11,"event":"pa"#).unwrap();
    let file = File::options().write(true).open(&stale).unwrap();
    file.set_modified(std::time::SystemTime::now() - Duration::from_secs(3600))
        .unwrap();
    drop(file);
    let live = home
        .spool()
        .join(".panic-fedcba9876543210fedcba9876543210.tmp");
    fs::write(&live, b"{").unwrap();
    let out = home.run(&server.url, &["telemetry", "flush"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(server.requests(), 0);
    assert!(!stale.exists(), "a crashed write is removed");
    assert!(live.exists(), "a fresh one may be a live hook's");
    assert_eq!(home.status()["upload"]["discarded_temps"], 1);
}

#[test]
fn a_panic_starts_the_upload_helper() {
    let home = Home::new("panic-helper");
    let server = Endpoint::accepting();
    enable_collect_only(&home);
    panics(&mut panicking_to(&home, &server.url, "look", "a.tiff"));
    wait_until("the panic's upload", || server.requests() == 1);
    let body: serde_json::Value = serde_json::from_str(&server.bodies()[0]).unwrap();
    let events = body["events"].as_array().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event_name"], "panic");
    wait_until("an empty spool", || batches(&home) == 0);
    assert!(
        !spool_names(&home)
            .iter()
            .any(|n| n.starts_with("panic-ready-")),
        "{:?}",
        spool_names(&home)
    );
}

#[test]
fn a_panic_published_while_a_helper_drains_is_uploaded() {
    let home = Home::new("panic-handoff");
    let server = Endpoint::accepting();
    // The enable helper finds the queue empty, then holds the drain lock.
    let out = home
        .command(&server.url, &["telemetry", "enable", "--yes"])
        .env("NC_TELEMETRY_DRAIN_HOLD_MS", "3000")
        .output()
        .unwrap();
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    thread::sleep(Duration::from_millis(500));
    // Published inside the hold, by a run with no helper of its own.
    panics(&mut panicking(&home, "decode", "a.tiff"));
    wait_until("the late panic's upload", || server.requests() == 1);
    assert!(server.bodies()[0].contains(r#""event_name":"panic""#));
}

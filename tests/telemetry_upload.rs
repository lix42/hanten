//! End-to-end tests of opt-in telemetry upload (`telemetry/upload`): the binary
//! against a fake `/v1/events` endpoint on loopback, with consent, data and queue
//! directories in a private temp dir so the machine's own consent is never read.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const NC: &str = env!("CARGO_BIN_EXE_hanten");

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A private home for one test: consent, data dir and outputs.
struct Home(PathBuf);

impl Home {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("nc-upload-{}-{tag}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn queue(&self) -> PathBuf {
        self.path("data/nc/telemetry.jsonl")
    }

    fn spool(&self) -> PathBuf {
        self.path("data/nc/.telemetry.jsonl.nc-telemetry-spool")
    }

    fn config(&self) -> PathBuf {
        self.path("cfg/nc")
    }

    fn command(&self, endpoint: &str, args: &[&str]) -> Command {
        let mut cmd = Command::new(NC);
        cmd.args(args)
            .env_remove("NC_TELEMETRY")
            .env_remove("NC_TELEMETRY_LOG")
            .env("XDG_CONFIG_HOME", self.path("cfg"))
            .env("XDG_DATA_HOME", self.path("data"))
            .env("NC_TELEMETRY_ENDPOINT", endpoint)
            .stdin(Stdio::null());
        cmd
    }

    fn run(&self, endpoint: &str, args: &[&str]) -> Output {
        self.command(endpoint, args).output().unwrap()
    }

    /// A successful conversion to `name`.
    fn convert(&self, endpoint: &str, name: &str, extra: &[&str]) -> Output {
        let out = self.path(name);
        let input = fixture("hdr-48bit.tif");
        let mut args = vec![
            "convert",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.8,0.5,0.3",
            "--report",
            "none",
        ];
        args.extend_from_slice(extra);
        let output = self.run(endpoint, &args);
        assert_eq!(code(&output), 0, "{}", stderr(&output));
        output
    }

    fn consent(&self) -> serde_json::Value {
        serde_json::from_slice(&fs::read(self.config().join("telemetry-consent.json")).unwrap())
            .unwrap()
    }

    fn status(&self) -> serde_json::Value {
        let out = self.run("none", &["telemetry", "status"]);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    /// Hold `name` (a lock beside the consent record) as another process would.
    fn hold(&self, name: &str, exclusive: bool) -> File {
        fs::create_dir_all(self.config()).unwrap();
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.config().join(name))
            .unwrap();
        if exclusive {
            file.lock().unwrap();
        } else {
            file.lock_shared().unwrap();
        }
        file
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn code(out: &Output) -> i32 {
    out.status.code().expect("terminated by a signal")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(25));
    }
}

/// How the fake endpoint answers the n-th request (0-based) with this body.
type Responder = dyn Fn(usize, &str) -> Reply + Send + Sync;

struct Reply {
    status: u16,
    body: String,
    delay: Duration,
}

/// A fake ingestion endpoint on loopback recording every request body.
struct Endpoint {
    url: String,
    bodies: Arc<Mutex<Vec<String>>>,
}

impl Endpoint {
    fn start(responder: impl Fn(usize, &str) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/events", listener.local_addr().unwrap());
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let responder: Arc<Responder> = Arc::new(responder);
        let seen = Arc::clone(&bodies);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let (seen, responder) = (Arc::clone(&seen), Arc::clone(&responder));
                thread::spawn(move || serve(stream, &seen, &*responder));
            }
        });
        Self { url, bodies }
    }

    /// Accept everything.
    fn accepting() -> Self {
        Self::start(|_, body| Reply {
            status: 200,
            body: acknowledge(body, &[], &[]),
            delay: Duration::ZERO,
        })
    }

    fn bodies(&self) -> Vec<String> {
        self.bodies.lock().unwrap().clone()
    }

    fn requests(&self) -> usize {
        self.bodies.lock().unwrap().len()
    }
}

fn serve(stream: TcpStream, seen: &Mutex<Vec<String>>, responder: &Responder) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut length = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap();
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let body = String::from_utf8(body).unwrap();
    let n = {
        let mut seen = seen.lock().unwrap();
        seen.push(body.clone());
        seen.len() - 1
    };
    let reply = responder(n, &body);
    thread::sleep(reply.delay);
    let mut stream = stream;
    let _ = write!(
        stream,
        "HTTP/1.1 {} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\
         connection: close\r\n\r\n{}",
        reply.status,
        reply.body.len(),
        reply.body
    );
}

fn event_ids(body: &str) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    v["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["event_id"].as_str().unwrap().to_owned())
        .collect()
}

/// An acknowledgement accepting every ID except those listed as duplicate or
/// rejected.
fn acknowledge(body: &str, duplicate: &[&str], rejected: &[&str]) -> String {
    let ids = event_ids(body);
    let accepted: Vec<&String> = ids
        .iter()
        .filter(|i| !duplicate.contains(&i.as_str()) && !rejected.contains(&i.as_str()))
        .collect();
    let rejected: Vec<serde_json::Value> = rejected
        .iter()
        .map(|i| serde_json::json!({"event_id": i, "code": "invalid_field"}))
        .collect();
    serde_json::json!({
        "upload_schema_version": 1,
        "accepted": accepted,
        "duplicate": duplicate,
        "rejected": rejected,
    })
    .to_string()
}

fn enable(home: &Home, endpoint: &str) {
    let out = home.run(endpoint, &["telemetry", "enable", "--yes"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
}

/// Enable without starting a helper, so only `flush` uploads.
fn enable_collect_only(home: &Home) {
    let out = home
        .command("file:/dev/null", &["telemetry", "enable", "--yes"])
        .env("NC_TELEMETRY_HELPER", "0")
        .output()
        .unwrap();
    assert_eq!(code(&out), 0, "{}", stderr(&out));
}

/// Every spool entry name.
fn spool_names(home: &Home) -> BTreeSet<String> {
    fs::read_dir(home.spool())
        .map(|d| {
            d.map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect()
        })
        .unwrap_or_default()
}

fn batches(home: &Home) -> usize {
    spool_names(home)
        .iter()
        .filter(|n| n.starts_with("batch-"))
        .count()
}

/// The event IDs of the queue file's lines.
fn queued_ids(home: &Home) -> Vec<String> {
    fs::read_to_string(home.queue())
        .unwrap_or_default()
        .lines()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            v["event_id"].as_str().unwrap().to_owned()
        })
        .collect()
}

#[test]
fn enable_uploads_queued_current_events_and_drops_older_schemas() {
    let home = Home::new("enable");
    let server = Endpoint::accepting();
    // Collected locally before consent: one current event and one older record.
    home.convert("none", "pre.tiff", &["--telemetry"]);
    let before = queued_ids(&home);
    assert_eq!(before.len(), 1);
    let mut queue = fs::OpenOptions::new()
        .append(true)
        .open(home.queue())
        .unwrap();
    writeln!(queue, r#"{{"schema_version":9,"timestamp_ms":1}}"#).unwrap();
    drop(queue);

    enable(&home, &server.url);
    wait_until("the enable helper's upload", || server.requests() == 1);
    assert_eq!(event_ids(&server.bodies()[0]), before);
    wait_until("the helper to finish", || {
        home.status()["upload"]["accepted"] == 1
    });
    assert_eq!(home.status()["upload"]["dropped_other_schema"], 1);

    // From now on every convert is collected and uploaded by its own helper.
    home.convert(&server.url, "a.tiff", &[]);
    wait_until("the convert's upload", || server.requests() == 2);
    let event =
        &serde_json::from_str::<serde_json::Value>(&server.bodies()[1]).unwrap()["events"][0];
    assert_eq!(event["outcome"]["status"], "success");
    for forbidden in [
        "params_hash",
        "width",
        "timestamp_ms",
        "target",
        "a.tiff",
        "hdr-48bit",
    ] {
        assert!(
            !server.bodies()[1].contains(forbidden),
            "{forbidden} uploaded"
        );
    }
    wait_until("an empty queue", || batches(&home) == 0);
}

#[test]
fn without_consent_nothing_is_collected_and_no_file_is_created() {
    let home = Home::new("noconsent");
    let server = Endpoint::accepting();
    home.convert(&server.url, "a.tiff", &[]);
    let refused = home.run(&server.url, &["convert", "--bogus"]);
    assert_eq!(code(&refused), 2);
    thread::sleep(Duration::from_millis(300));
    assert_eq!(server.requests(), 0);
    assert!(!home.path("cfg").exists(), "no consent dir or lock file");
    assert!(!home.path("data").exists(), "no queue");
}

#[test]
fn nc_telemetry_zero_overrides_consent() {
    let home = Home::new("envoff");
    let server = Endpoint::accepting();
    enable_collect_only(&home);
    let mut cmd = home.command(&server.url, &[]);
    let out = cmd
        .args([
            "convert",
            fixture("hdr-48bit.tif").to_str().unwrap(),
            "-o",
            home.path("a.tiff").to_str().unwrap(),
            "--film-base",
            "0.8,0.5,0.3",
            "--report",
            "none",
        ])
        .env("NC_TELEMETRY", "0")
        .output()
        .unwrap();
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    thread::sleep(Duration::from_millis(300));
    assert_eq!(server.requests(), 0);
    assert!(queued_ids(&home).is_empty());
}

#[test]
fn a_refused_convert_is_a_parse_failure_event() {
    let home = Home::new("parse");
    let server = Endpoint::accepting();
    enable(&home, &server.url);
    let out = home.run(
        &server.url,
        &["convert", "--no-such-flag", "/secret/path.tif"],
    );
    assert_eq!(code(&out), 2);
    assert!(
        stderr(&out).contains("--no-such-flag"),
        "clap's own message"
    );
    wait_until("the parse failure's upload", || server.requests() == 1);
    let body = &server.bodies()[0];
    let event = &serde_json::from_str::<serde_json::Value>(body).unwrap()["events"][0];
    assert_eq!(event["stage"], "parse");
    assert_eq!(event["outcome"]["error_kind"], "usage");
    assert_eq!(event["outcome"]["exit_code"], 2);
    assert!(!body.contains("secret") && !body.contains("no-such-flag"));
    // Another subcommand's refusal is not recorded.
    let out = home.run(&server.url, &["inspect", "--no-such-flag"]);
    assert_eq!(code(&out), 2);
    thread::sleep(Duration::from_millis(500));
    assert_eq!(server.requests(), 1);
}

#[test]
fn an_event_queued_while_another_helper_drains_is_uploaded() {
    let home = Home::new("handoff");
    let server = Endpoint::accepting();
    // The enable helper finds the queue empty, then holds the drain lock, so the
    // conversion's helper finds it busy and exits.
    let out = home
        .command(&server.url, &["telemetry", "enable", "--yes"])
        .env("NC_TELEMETRY_DRAIN_HOLD_MS", "1500")
        .output()
        .unwrap();
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    thread::sleep(Duration::from_millis(500));
    // A refused `convert` queues its event fast, well inside the hold.
    let out = home.run(&server.url, &["convert", "--bogus"]);
    assert_eq!(code(&out), 2);
    wait_until("the late event's upload", || server.requests() == 1);
}

#[test]
fn retryable_failures_keep_the_batch_and_retry_with_the_same_ids() {
    let home = Home::new("retry");
    let server = Endpoint::start(|n, body| match n {
        0 => Reply {
            status: 503,
            body: r#"{"error":"unavailable"}"#.into(),
            delay: Duration::ZERO,
        },
        // A reply missing an ID is not an acknowledgement.
        1 => Reply {
            status: 200,
            body: r#"{"upload_schema_version":1,"accepted":[],"duplicate":[],"rejected":[]}"#
                .into(),
            delay: Duration::ZERO,
        },
        2 => Reply {
            status: 429,
            body: "{}".into(),
            delay: Duration::ZERO,
        },
        _ => Reply {
            status: 200,
            body: acknowledge(body, &[], &[]),
            delay: Duration::ZERO,
        },
    });
    // Collect without uploading, then drain by hand.
    enable_collect_only(&home);
    home.convert("none", "a.tiff", &[]);
    home.convert("none", "b.tiff", &[]);
    let queued = queued_ids(&home);
    assert_eq!(queued.len(), 2);

    for (attempt, kind) in [(0, "http_503"), (1, "bad_response"), (2, "http_429")] {
        let out = home.run(&server.url, &["telemetry", "flush"]);
        assert_eq!(code(&out), 1, "attempt {attempt}: {}", stderr(&out));
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(report["error"], kind);
        assert_eq!(report["pending_batches"], 1);
        assert_eq!(batches(&home), 1);
        assert_eq!(home.status()["upload"]["last_error"]["kind"], kind);
    }
    let out = home.run(&server.url, &["telemetry", "flush"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(batches(&home), 0);
    let bodies = server.bodies();
    assert_eq!(bodies.len(), 4);
    assert!(
        bodies.iter().all(|b| b == &bodies[0]),
        "the same immutable batch"
    );
    assert_eq!(event_ids(&bodies[0]), queued);
    let status = home.status()["upload"].clone();
    assert_eq!(status["accepted"], 2);
    assert_eq!(status["consecutive_failures"], 0);
}

#[test]
fn rejected_events_and_malformed_requests_are_quarantined_not_retried() {
    let home = Home::new("reject");
    let rejected_id = Arc::new(Mutex::new(String::new()));
    let pick = Arc::clone(&rejected_id);
    let server = Endpoint::start(move |n, body| {
        if n == 0 {
            let ids = event_ids(body);
            *pick.lock().unwrap() = ids[0].clone();
            Reply {
                status: 200,
                body: acknowledge(body, &[&ids[1]], &[&ids[0]]),
                delay: Duration::ZERO,
            }
        } else {
            Reply {
                status: 400,
                body: r#"{"error":"invalid_envelope"}"#.into(),
                delay: Duration::ZERO,
            }
        }
    });
    enable_collect_only(&home);
    for name in ["a.tiff", "b.tiff", "c.tiff"] {
        home.convert("none", name, &[]);
    }
    let out = home.run(&server.url, &["telemetry", "flush"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        (
            report["accepted"].clone(),
            report["duplicate"].clone(),
            report["rejected"].clone()
        ),
        (1.into(), 1.into(), 1.into())
    );
    assert_eq!(batches(&home), 0);
    let quarantine: Vec<String> = spool_names(&home)
        .into_iter()
        .filter(|n| n.starts_with("quarantine-"))
        .collect();
    assert_eq!(quarantine.len(), 1);
    let record = fs::read_to_string(home.spool().join(&quarantine[0])).unwrap();
    assert!(record.contains(&*rejected_id.lock().unwrap()));
    assert!(record.contains("invalid_field"));

    home.convert("none", "d.tiff", &[]);
    let out = home.run(&server.url, &["telemetry", "flush"]);
    assert_eq!(code(&out), 0, "a 400 is final, not an error to retry");
    assert_eq!(batches(&home), 0);
    assert_eq!(home.status()["queue"]["quarantine_records"], 2);
    assert_eq!(server.requests(), 2);
}

#[test]
fn preview_prints_exactly_what_flush_sends() {
    let home = Home::new("preview");
    let server = Endpoint::accepting();
    enable_collect_only(&home);
    home.convert("none", "a.tiff", &[]);
    let refused = home.run("none", &["convert", "--bogus"]);
    assert_eq!(code(&refused), 2);
    let preview = home.run("none", &["telemetry", "preview"]);
    assert_eq!(code(&preview), 0, "{}", stderr(&preview));
    let previewed: Vec<String> = String::from_utf8(preview.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(previewed.len(), 1);
    assert_eq!(event_ids(&previewed[0]).len(), 2);
    let out = home.run(&server.url, &["telemetry", "flush"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(server.bodies(), previewed);
}

#[test]
fn a_hanging_endpoint_never_reaches_the_conversion() {
    let home = Home::new("hang");
    let server = Endpoint::start(|_, body| Reply {
        status: 200,
        body: acknowledge(body, &[], &[]),
        delay: Duration::from_secs(30),
    });
    enable(&home, &server.url);
    let reference = Home::new("hang-ref");
    reference.convert("none", "a.tiff", &[]);
    let started = Instant::now();
    let output = home.convert(&server.url, "a.tiff", &[]);
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(8),
        "the conversion waited on the endpoint: {elapsed:?}"
    );
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read(home.path("a.tiff")).unwrap(),
        fs::read(reference.path("a.tiff")).unwrap()
    );
    // The helper gives up within the request timeout and keeps the batch.
    wait_until("the helper's timeout", || {
        home.status()["upload"]["last_error"]["kind"] == "timeout"
    });
    assert_eq!(home.status()["queue"]["batches"], 1);
}

#[test]
fn enable_is_idempotent_and_active_retarget_is_refused() {
    let home = Home::new("idem");
    enable(&home, "file:/dev/null");
    let first = home.consent();
    enable(&home, "file:/dev/null");
    assert_eq!(home.consent(), first, "same generation");
    let other = home.path("other.jsonl");
    let out = home.run(
        "file:/dev/null",
        &[
            "telemetry",
            "enable",
            "--yes",
            "--queue",
            other.to_str().unwrap(),
        ],
    );
    assert_eq!(code(&out), 2);
    assert!(
        stderr(&out).contains("telemetry disable"),
        "{}",
        stderr(&out)
    );
    assert_eq!(home.consent(), first);
    // Without --yes and off a terminal, enable refuses and publishes nothing.
    let fresh = Home::new("idem-noyes");
    let out = fresh.run("file:/dev/null", &["telemetry", "enable"]);
    assert_eq!(code(&out), 2);
    assert!(!fresh.config().join("telemetry-consent.json").exists());
    // A build whose endpoint is `none` has nothing to enable.
    let out = fresh.run("none", &["telemetry", "enable", "--yes"]);
    assert_eq!(code(&out), 2);
}

#[test]
fn a_corrupt_consent_record_fails_closed() {
    let home = Home::new("corrupt");
    let server = Endpoint::accepting();
    enable(&home, &server.url);
    let record = home.config().join("telemetry-consent.json");
    let text = fs::read_to_string(&record).unwrap();
    fs::write(&record, &text[..text.len() / 2]).unwrap();
    home.convert(&server.url, "a.tiff", &[]);
    thread::sleep(Duration::from_millis(500));
    assert_eq!(server.requests(), 0);
    assert!(queued_ids(&home).is_empty());
    assert_eq!(home.status()["consent"]["state"], "unreadable");
    let out = home.run(&server.url, &["telemetry", "flush"]);
    assert_ne!(code(&out), 0);
}

#[test]
fn disable_waits_for_a_request_in_flight_but_not_for_a_conversion() {
    let home = Home::new("disable");
    enable_collect_only(&home);
    // A conversion holding its shared collection lease does not hold disable up.
    let conversion = home.hold("telemetry-collection.lease", false);
    let started = Instant::now();
    let out = home.run("none", &["telemetry", "disable"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(home.consent()["state"], "inactive");
    drop(conversion);

    enable_collect_only(&home);
    // A request in flight (a shared request lease) does.
    let request = home.hold("telemetry-request.lease", false);
    let mut disable = home
        .command("none", &["telemetry", "disable"])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(700));
    assert!(
        disable.try_wait().unwrap().is_none(),
        "disable returned early"
    );
    assert_eq!(home.consent()["state"], "active");
    drop(request);
    assert!(disable.wait().unwrap().success());
    assert_eq!(home.consent()["state"], "inactive");
}

/// Wait for `child`, failing if it returned before `released`.
fn finishes_only_after(mut child: Child, hold: File, what: &str) -> Output {
    thread::sleep(Duration::from_millis(700));
    assert!(child.try_wait().unwrap().is_none(), "{what} did not wait");
    drop(hold);
    child.wait_with_output().unwrap()
}

#[test]
fn purge_is_inactive_only_and_waits_out_running_conversions() {
    let home = Home::new("purge");
    enable_collect_only(&home);
    home.convert("none", "a.tiff", &[]);
    let out = home.run("none", &["telemetry", "purge", "--yes"]);
    assert_eq!(code(&out), 2, "purge while enabled");
    assert_eq!(queued_ids(&home).len(), 1);

    assert_eq!(code(&home.run("none", &["telemetry", "disable"])), 0);
    // A consented conversion still running after disable may append once more.
    let running = home.hold("telemetry-collection.lease", false);
    let purge = home
        .command("none", &["telemetry", "purge", "--yes"])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = finishes_only_after(purge, running, "purge");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(queued_ids(&home).is_empty());
    assert_eq!(fs::metadata(home.queue()).unwrap().len(), 0);
    assert!(spool_names(&home).contains("queue.lock"));
    assert_eq!(
        home.consent()["state"],
        "inactive",
        "purge keeps the consent target"
    );
}

#[test]
fn reenabling_the_same_queue_waits_for_old_conversions_and_helpers() {
    let home = Home::new("reenable");
    enable_collect_only(&home);
    let old = home.consent()["generation"].clone();
    assert_eq!(code(&home.run("none", &["telemetry", "disable"])), 0);
    let running = home.hold("telemetry-collection.lease", false);
    let enable = home
        .command("file:/dev/null", &["telemetry", "enable", "--yes"])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(400));
    assert_eq!(
        home.consent()["state"],
        "inactive",
        "published while a run held on"
    );
    let out = finishes_only_after(enable, running, "enable");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(home.consent()["state"], "active");
    assert_ne!(home.consent()["generation"], old, "a fresh generation");
}

#[test]
fn an_inactive_retarget_needs_an_empty_old_queue() {
    let home = Home::new("retarget");
    enable_collect_only(&home);
    home.convert("none", "a.tiff", &[]);
    assert_eq!(code(&home.run("none", &["telemetry", "disable"])), 0);
    let first = home.consent();
    let other = home.path("other.jsonl");
    let retarget = [
        "telemetry",
        "enable",
        "--yes",
        "--queue",
        other.to_str().unwrap(),
    ];
    let out = home.run("file:/dev/null", &retarget);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("telemetry purge"), "{}", stderr(&out));
    assert_eq!(home.consent(), first, "nothing published");

    assert_eq!(code(&home.run("none", &["telemetry", "purge", "--yes"])), 0);
    let out = home.run("file:/dev/null", &retarget);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let consent = home.consent();
    assert_eq!(consent["state"], "active");
    // Stored with its parent canonical (macOS's temp dir is under a symlink).
    let stored = fs::canonicalize(&other).unwrap();
    assert_eq!(consent["queue"], stored.to_str().unwrap());
    assert_ne!(consent["generation"], first["generation"]);
    // The new queue collects; the old one is left alone.
    home.convert("none", "b.tiff", &[]);
    assert_eq!(fs::read_to_string(&other).unwrap().lines().count(), 1);
    assert!(queued_ids(&home).is_empty());
}

#[test]
fn an_explicit_log_on_the_selected_queue_is_written_once() {
    let home = Home::new("explicit");
    enable_collect_only(&home);
    home.convert("none", "a.tiff", &["--telemetry"]);
    assert_eq!(
        queued_ids(&home).len(),
        1,
        "managed and explicit share one line"
    );
    // An explicit log elsewhere stays local and is never drained.
    let mut cmd = home.command("none", &[]);
    let custom = home.path("custom.jsonl");
    let out = cmd
        .args([
            "convert",
            fixture("hdr-48bit.tif").to_str().unwrap(),
            "-o",
            home.path("b.tiff").to_str().unwrap(),
            "--film-base",
            "0.8,0.5,0.3",
            "--report",
            "none",
            "--telemetry",
        ])
        .env("NC_TELEMETRY_LOG", &custom)
        .output()
        .unwrap();
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(fs::read_to_string(&custom).unwrap().lines().count(), 1);
    assert_eq!(queued_ids(&home).len(), 2);
    let sent = home.path("sent.jsonl");
    let out = home.run(&format!("file:{}", sent.display()), &["telemetry", "flush"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(event_ids(&fs::read_to_string(&sent).unwrap()).len(), 2);
    assert_eq!(fs::read_to_string(&custom).unwrap().lines().count(), 1);
}

/// The convert arguments `Home::convert` uses, for a test that sets its own env.
fn convert_args(home: &Home, name: &str) -> Vec<String> {
    [
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        home.path(name).to_str().unwrap(),
        "--film-base",
        "0.8,0.5,0.3",
        "--report",
        "none",
    ]
    .map(String::from)
    .to_vec()
}

#[test]
fn nc_telemetry_zero_keeps_an_explicit_event_out_of_the_upload_queue() {
    let home = Home::new("envoff-explicit");
    enable_collect_only(&home);
    let out = home
        .command("none", &[])
        .args(convert_args(&home, "a.tiff"))
        .arg("--telemetry")
        .env("NC_TELEMETRY", "0")
        .output()
        .unwrap();
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stderr(&out).contains("NC_TELEMETRY=0"), "{}", stderr(&out));
    assert!(queued_ids(&home).is_empty());
}

#[test]
fn a_telemetry_file_never_overwrites_the_upload_queue() {
    let home = Home::new("oneoff");
    enable_collect_only(&home);
    home.convert("none", "a.tiff", &[]);
    let queued = queued_ids(&home);
    let out = home.convert(
        "none",
        "b.tiff",
        &["--telemetry-file", home.queue().to_str().unwrap()],
    );
    assert!(
        stderr(&out).contains("is the upload queue"),
        "{}",
        stderr(&out)
    );
    assert_eq!(queued_ids(&home), queued);
}

#[cfg(unix)]
#[test]
fn a_batch_that_cannot_be_read_is_kept() {
    use std::os::unix::fs::PermissionsExt;
    let home = Home::new("unreadable");
    let server = Endpoint::start(|n, body| Reply {
        status: if n == 0 { 503 } else { 200 },
        body: acknowledge(body, &[], &[]),
        delay: Duration::ZERO,
    });
    enable_collect_only(&home);
    home.convert("none", "a.tiff", &[]);
    assert_eq!(code(&home.run(&server.url, &["telemetry", "flush"])), 1);
    let batch = spool_names(&home)
        .into_iter()
        .find(|n| n.starts_with("batch-"))
        .unwrap();
    let path = home.spool().join(&batch);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
    let out = home.run(&server.url, &["telemetry", "flush"]);
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    assert!(
        path.exists(),
        "an unreadable batch is kept, not quarantined"
    );
    assert_eq!(server.requests(), 1);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(code(&home.run(&server.url, &["telemetry", "flush"])), 0);
    assert!(!path.exists());
}

#[test]
fn a_pending_batch_blocks_an_inactive_retarget() {
    let home = Home::new("retarget-batch");
    let server = Endpoint::start(|_, _| Reply {
        status: 503,
        body: "{}".into(),
        delay: Duration::ZERO,
    });
    enable_collect_only(&home);
    home.convert("none", "a.tiff", &[]);
    assert_eq!(code(&home.run(&server.url, &["telemetry", "flush"])), 1);
    assert_eq!(fs::metadata(home.queue()).unwrap().len(), 0, "rotated");
    assert_eq!(batches(&home), 1);
    assert_eq!(code(&home.run("none", &["telemetry", "disable"])), 0);
    let other = home.path("other.jsonl");
    let out = home.run(
        "file:/dev/null",
        &[
            "telemetry",
            "enable",
            "--yes",
            "--queue",
            other.to_str().unwrap(),
        ],
    );
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("batch-"), "{}", stderr(&out));
    assert_eq!(home.consent()["state"], "inactive");
}

#[test]
fn reenabling_waits_for_the_old_helper_too() {
    let home = Home::new("reenable-drain");
    enable_collect_only(&home);
    assert_eq!(code(&home.run("none", &["telemetry", "disable"])), 0);
    // The old helper, still draining.
    let drain = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(home.spool().join("drain.lock"))
        .unwrap();
    drain.lock().unwrap();
    let enable = home
        .command("file:/dev/null", &["telemetry", "enable", "--yes"])
        .env("NC_TELEMETRY_HELPER", "0")
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = finishes_only_after(enable, drain, "enable");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(home.consent()["state"], "active");
}

use super::*;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/contracts/telemetry/upload-v1/local/panic-ready.json"
);

fn fixture_event() -> PanicEvent {
    let mut event = build(
        EventId::parse("5f0c3a9e81d24b7c9e0a6d3f2b1c8e47").unwrap(),
        1_781_000_000_000,
        Some(11),
        PanicStage::At(EventStage::Stage(StageKind::Look)),
        vec![
            "nc::pipeline::look::apply".into(),
            "nc::cli::convert_frame".into(),
            "nc::main".into(),
        ],
    );
    // The fixture pins a release and a target; the build's own are compile-time.
    event.nc_version = "0.1.0".into();
    event.target = "aarch64-apple-darwin".into();
    event
}

#[test]
fn the_written_bytes_are_the_shared_fixture() {
    let fixture = std::fs::read(FIXTURE).unwrap();
    assert_eq!(
        String::from_utf8(ready_bytes(&fixture_event()).unwrap()).unwrap(),
        String::from_utf8(fixture.clone()).unwrap()
    );
    let read: PanicEvent = serde_json::from_slice(&fixture).unwrap();
    assert_eq!(read, fixture_event());
}

#[test]
fn the_stage_is_a_stage_a_phase_or_unknown() {
    for (stage, wire) in [
        (
            PanicStage::At(EventStage::Stage(StageKind::FitGamut)),
            "fit_gamut",
        ),
        (PanicStage::At(EventStage::Setup), "setup"),
        (PanicStage::At(EventStage::Finalize), "finalize"),
        (PanicStage::Unknown, "unknown"),
    ] {
        let json = serde_json::to_string(&stage).unwrap();
        assert_eq!(json, format!("\"{wire}\""));
        assert_eq!(serde_json::from_str::<PanicStage>(&json).unwrap(), stage);
    }
    assert!(serde_json::from_str::<PanicStage>("\"warp\"").is_err());
}

/// The shape of what `Backtrace::force_capture()` prints in a release build of this
/// binary (aarch64-apple-darwin): the hook's frames, std's, then the panic site
/// outward.
const RELEASE_BACKTRACE: &str = "   0: <std::backtrace::Backtrace>::create
   1: hanten::telemetry::panic::report
   2: hanten::telemetry::panic::install::{closure#0}
   3: std::panicking::panic_with_hook
   4: std::panicking::panic_handler::{closure#0}
   5: std::sys::backtrace::__rust_end_short_backtrace::<std::panicking::panic_handler::{closure#0}, !>
   6: __rustc::rust_begin_unwind
   7: core::panicking::panic_fmt
   8: hanten::pipeline::look::apply
   9: hanten::cli::convert_frame
  10: hanten::main
  11: std::sys::backtrace::__rust_begin_short_backtrace::<fn(), ()>
  12: std::rt::lang_start::<()>::{closure#0}
  13: std::rt::lang_start_internal
  14: _main
";

#[test]
fn a_release_backtrace_keeps_the_crate_s_frames_as_nc_paths() {
    assert_eq!(
        sanitize(RELEASE_BACKTRACE, "hanten"),
        [
            "nc::pipeline::look::apply",
            "nc::cli::convert_frame",
            "nc::main"
        ]
    );
}

#[test]
fn a_debug_backtrace_s_locations_are_dropped() {
    let text = "   7: hanten::pipeline::look::apply::{closure#0}
             at ./src/pipeline/look.rs:12:5
   8: <rayon::iter::map::MapFolder<C, F> as rayon::iter::plumbing::Folder<T>>::consume
             at /Users/someone/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rayon-1.12.0/src/iter/map.rs:193:22
   9: hanten::pipeline::look::apply
             at ./src/pipeline/look.rs:40:9
";
    assert_eq!(
        sanitize(text, "hanten"),
        ["nc::pipeline::look::apply", "nc::pipeline::look::apply"]
    );
}

#[test]
fn hostile_text_yields_only_normalized_nc_paths() {
    let text = "\
thread 'main' panicked at /Users/alice/src/hanten/src/pipeline/look.rs:7:21:
hanten::secret /Users/alice/Pictures/roll-12/frame.tif
   0:        0x102dc1078 - hanten[2358ed647de94a65]::pipeline::look::apply
   1: hanten::pipeline::look::apply::h0123456789abcdef
   2: hanten::pipeline::look::apply::{{closure}}
   3: hanten::fit::<f32>
   4: <hanten::pipeline::Chain as core::ops::Drop>::drop
   5: hanten::cli::run at /Users/alice/src/hanten/src/cli.rs:12:3
   6: hanten::cli::run:12
   7: hanten::ünicode
   8: hanten::0x102dc1078
   9: tiff::decoder::Decoder<R>::read
  10: hanten_helper::thing
  11: hanten
  12: hanten::{closure#0}
  13: hanten::a::{closure#x}
  14: hanten[zz]::a
x15: hanten::forged
  16: hanten::cli::run\r
  17: hanten::pipeline::look::apply::{closure#0}::{closure#1}
  18: hanten[2358ed647de94a65]::pipeline::look::apply
  19: <hanten::pipeline::Chain>::run
  20: hanten::a::<T, /Users/alice/x>
  21: <hanten::X<f32>>::m
  22: hanten::b::<<T as hanten::Y>::Z>
  23: hanten::c::<T>>
  24: <hanten::X>
  25: <hanten::X> ::m
";
    let frames = sanitize(text, "hanten");
    assert_eq!(
        frames,
        [
            "nc::pipeline::look::apply",
            "nc::pipeline::look::apply",
            "nc::fit",
            "nc::cli::run",
            "nc::pipeline::look::apply",
            "nc::pipeline::look::apply",
            "nc::pipeline::Chain::run",
            "nc::a",
            "nc::b",
        ],
        "a hash, a closure, generic arguments, a CRLF, a disambiguator or an \
         inherent impl's brackets are stripped; nothing else survives"
    );
    for f in &frames {
        assert!(is_frame(f));
        for leak in ["alice", "/", ".rs", "0x", "secret", "frame.tif", ":12"] {
            assert!(!f.contains(leak), "{f} leaks {leak}");
        }
    }
}

#[test]
fn frames_are_capped_in_count_and_length() {
    let deep: String = (0..40).map(|i| format!("  {i}: hanten::a{i}\n")).collect();
    let frames = sanitize(&deep, "hanten");
    assert_eq!(frames.len(), MAX_FRAMES);
    assert_eq!(frames[0], "nc::a0");

    let long = format!("nc::{}", "a".repeat(MAX_FRAME_BYTES - 4));
    assert!(is_frame(&long));
    assert!(!is_frame(&format!("{long}b")));
    let segments = format!("nc{}", "::a".repeat(MAX_SEGMENTS));
    assert!(is_frame(&segments));
    assert!(!is_frame(&format!("{segments}::a")));
    assert!(is_frame("nc"));
    for bad in [
        "", "n", "nc::", "nc::1a", "nc:: a", "ncx::a", "nc::a::", "nc::a-b",
    ] {
        assert!(!is_frame(bad), "{bad:?}");
    }
}

#[test]
fn build_drops_what_is_not_a_frame_and_caps_the_list() {
    let mut frames: Vec<String> = vec!["/Users/alice".into(), "nc::a".into()];
    frames.extend((0..40).map(|i| format!("nc::f{i}")));
    let event = build(EventId([0; 16]), 0, None, PanicStage::Unknown, frames);
    assert_eq!(event.frames.len(), MAX_FRAMES);
    assert_eq!(event.frames[0], "nc::a");
    assert_eq!(event.event, EventName::Panic);
    assert_eq!(event.schema_version, SCHEMA_VERSION);
}

#[test]
fn the_largest_event_fits_the_file_cap() {
    let longest = format!("nc::{}", "a".repeat(MAX_FRAME_BYTES - 4));
    let event = build(
        EventId([0xff; 16]),
        u64::MAX,
        Some(u32::MAX),
        PanicStage::At(EventStage::Stage(StageKind::SceneCorrection)),
        vec![longest; MAX_FRAMES],
    );
    let bytes = ready_bytes(&event).unwrap();
    assert!(bytes.len() <= MAX_BYTES);
    assert_eq!(bytes.last(), Some(&b'\n'));
    assert_eq!(bytes.iter().filter(|b| **b == b'\n').count(), 1);
}

#[test]
fn every_stage_and_phase_survives_the_active_stage_code() {
    let phases = [
        EventStage::Parse,
        EventStage::Setup,
        EventStage::Preflight,
        EventStage::Finalize,
    ];
    let stages = StageKind::ALL.into_iter().map(EventStage::Stage);
    for stage in phases.into_iter().chain(stages) {
        assert_eq!(decode(encode(stage)), PanicStage::At(stage), "{stage:?}");
    }
    for unknown in [0, 5, 15, 26, u8::MAX] {
        assert_eq!(decode(unknown), PanicStage::Unknown, "{unknown}");
    }
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nc-panic-{tag}-{}-{}",
        std::process::id(),
        durable::random_hex().unwrap()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn publishing_never_replaces_a_ready_file_and_leaves_no_temp() {
    let dir = scratch("publish");
    let spool = dir.join("spool");
    let id = "0123456789abcdef0123456789abcdef";
    publish(&spool, id, b"first\n").unwrap();
    let ready = format!("panic-ready-{id}.json");
    assert_eq!(names(&spool), std::slice::from_ref(&ready));
    let err = publish(&spool, id, b"second\n").unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(spool.join(&ready)).unwrap(), b"first\n");
    assert_eq!(names(&spool), [ready], "the losing temp is gone");
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn publishing_never_follows_a_symlink() {
    let dir = scratch("symlink");
    let elsewhere = dir.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    let spool = dir.join("spool");
    std::os::unix::fs::symlink(&elsewhere, &spool).unwrap();
    assert!(publish(&spool, "ab", b"x\n").is_err(), "a symlinked spool");
    assert!(names(&elsewhere).is_empty());

    let real = dir.join("real");
    fs::create_dir_all(&real).unwrap();
    std::os::unix::fs::symlink(elsewhere.join("t"), real.join(".panic-cd.tmp")).unwrap();
    assert!(publish(&real, "cd", b"x\n").is_err(), "a planted temp");
    assert!(names(&elsewhere).is_empty(), "nothing written through it");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn publishing_stops_once_the_spool_holds_the_cap() {
    let dir = scratch("cap");
    let spool = dir.join("spool");
    durable::ensure_private_dir(&spool).unwrap();
    // Only ready files count.
    fs::write(spool.join("batch-panic-ab-0000.json"), b"{}").unwrap();
    for i in 0..MAX_READY - 1 {
        publish(&spool, &format!("{i:032x}"), b"x\n").unwrap();
    }
    publish(&spool, &format!("{:032x}", MAX_READY - 1), b"x\n").expect("one below the cap");
    let full = names(&spool);
    assert_eq!(full.len(), MAX_READY + 1);
    assert!(
        publish(&spool, &"f".repeat(32), b"x\n").is_err(),
        "at the cap"
    );
    assert_eq!(names(&spool), full, "nothing written, no temp left");
    let _ = fs::remove_dir_all(&dir);
}

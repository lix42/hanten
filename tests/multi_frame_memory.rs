//! A multi-frame run peaks at about its largest frame (`io/multi-frame-memory-growth`):
//! frames a few pixels apart, each larger than the last — the order in which freed
//! buffers cannot be reused — through `measure-roll` and `roll`. Each run's own peak
//! RSS (`wait4`) is checked against its largest frame's `memory.estimated_peak_bytes`,
//! and against the largest frame run alone, which catches growth the estimate's
//! allowance would absorb on frames this small.

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tiff::encoder::{TiffEncoder, colortype};
use tiff::tags::Tag;

const NC: &str = env!("CARGO_BIN_EXE_hanten");

/// Each frame grows by a few pixels each way, as one roll's scans do. Large enough
/// that every full-frame plane goes through the allocator's direct path.
const FRAMES: u32 = 8;
const BASE: (u32, u32) = (2600, 1760);
const STEP: (u32, u32) = (12, 4);

/// How far the run may peak above its largest frame alone. Measured ~20 MB for both
/// commands (`measure-roll`'s pool keeps ~1.5 MB a frame); without the allocator
/// the growth is ~0.9 GB.
const GROWTH_SLACK_BYTES: u64 = 64 << 20;

/// An HDRi-shaped scan: uniform RGB16 plus a marker-verified Gray16 IR page.
fn write_hdri(path: &Path, w: u32, h: u32) {
    let rgb = [20000u16, 12000, 8000].repeat((w * h) as usize);
    let mut enc = TiffEncoder::new(std::fs::File::create(path).unwrap()).unwrap();
    enc.new_image::<colortype::RGB16>(w, h)
        .unwrap()
        .write_data(&rgb)
        .unwrap();
    let mut ir = enc.new_image::<colortype::Gray16>(w, h).unwrap();
    ir.encoder().write_tag(Tag::NewSubfileType, 4u32).unwrap();
    ir.write_data(&vec![41_000u16; (w * h) as usize]).unwrap();
}

/// Removes the scratch directory however the test ends.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One run: the largest per-frame estimate in its report, and its own peak RSS in
/// bytes. Reaped with `wait4`, so no other child's peak can leak into the reading.
struct Run {
    estimate: u64,
    peak: u64,
}

fn run(args: &[&OsStr], stderr_file: &Path) -> Run {
    #[expect(
        clippy::zombie_processes,
        reason = "reaped by `wait4` below, for its rusage"
    )]
    let mut child = Command::new(NC)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(std::fs::File::create(stderr_file).unwrap())
        .spawn()
        .unwrap();
    let mut stdout = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    let (mut status, pid) = (0, child.id() as libc::pid_t);
    // SAFETY: `rusage` is plain integers, so all-zero is valid; `wait4` reaps this
    // child, which `Child` has not waited for, writing through pointers to owned values.
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::wait4(pid, &mut status, 0, &mut ru) }, pid);
    assert!(
        libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0,
        "{args:?} exited {status}: {}",
        std::fs::read_to_string(stderr_file).unwrap()
    );
    let report: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    let estimate = report["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["memory"]["estimated_peak_bytes"].as_u64().unwrap())
        .max()
        .unwrap();
    // Bytes on Darwin, KiB on Linux (getrusage(2)).
    let maxrss = ru.ru_maxrss as u64;
    let peak = if cfg!(target_os = "macos") {
        maxrss
    } else {
        maxrss * 1024
    };
    Run { estimate, peak }
}

#[test]
fn a_roll_of_growing_frames_peaks_at_its_largest_frame() {
    let scratch =
        Scratch(std::env::temp_dir().join(format!("nc-multi-frame-memory-{}", std::process::id())));
    let dir = &scratch.0;
    std::fs::create_dir_all(dir).unwrap();
    let frames: Vec<PathBuf> = (0..FRAMES)
        .map(|i| {
            let p = dir.join(format!("{i:02}.tif"));
            write_hdri(&p, BASE.0 + i * STEP.0, BASE.1 + i * STEP.1);
            p
        })
        .collect();
    // Synthetic scans carry no SilverFast provenance, so the input is asserted.
    let recipe = dir.join("roll.json");
    std::fs::write(
        &recipe,
        r#"{ "recipe_version": 3,
             "input": { "transfer": "linear", "meaning": "scanner-device" },
             "calibration": { "film_base": { "explicit": [0.9, 0.55, 0.42] } } }"#,
    )
    .unwrap();
    let out_dir = dir.join("out");
    let stderr_file = dir.join("stderr.txt");
    let commands: [&[&OsStr]; 2] = [
        &["measure-roll".as_ref()],
        &["roll".as_ref(), "-o".as_ref(), out_dir.as_os_str()],
    ];
    for command in commands {
        let with = |inputs: &[PathBuf]| {
            let mut args = command.to_vec();
            args.extend(["--params".as_ref(), recipe.as_os_str(), "--quiet".as_ref()]);
            args.extend(inputs.iter().map(|p| p.as_os_str()));
            run(&args, &stderr_file)
        };
        let alone = with(&frames[frames.len() - 1..]);
        let all = with(&frames);
        let name = command[0].to_string_lossy();
        assert!(
            all.peak <= all.estimate,
            "{name} peaked at {} bytes over {FRAMES} frames; its largest frame was \
             estimated at {}",
            all.peak,
            all.estimate
        );
        assert!(
            all.peak <= alone.peak + GROWTH_SLACK_BYTES,
            "{name} peaked at {} bytes over {FRAMES} frames against {} for its largest \
             frame alone",
            all.peak,
            alone.peak
        );
    }
}

//! End-to-end pipeline tests — drive the compiled `nc` binary against the
//! committed real-scan fixtures (`tests/fixtures/`) and assert on exit codes,
//! the JSON report on stdout, and the files written. This exercises the full
//! decode → film-base → rendering chain → encode path that the unit tests (which
//! stop at module boundaries) can't.
//!
//! stdout must stay pure JSON (the agent contract), so every test parses it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use tiff::encoder::{TiffEncoder, colortype};

/// The binary under test, provided by Cargo for integration tests.
const NC: &str = env!("CARGO_BIN_EXE_hanten");

/// A committed fixture by file name.
///
/// `hdri-64bit.tif` carries an IR plane, so every conversion of it without
/// `--export-ir` warns that the plane is "preserved but not used", and a `--strict` run
/// then fails whatever else it tests. To prove a *specific* warning is strict-promotable,
/// use the IR-free `hdr-48bit.tif` (or pass `--export-ir` when the test needs the plane)
/// and add a no-override control run so the assertion is falsifiable.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Synthesize a uniform 16-bit RGB TIFF (the `RGB(16)` chunky layout the decoder
/// accepts) with every pixel set to `rgb`, at `path`. Encodes into memory then writes the whole buffer,
/// so the file can't be left truncated by a dropped writer.
fn write_uniform_rgb48(path: &Path, rgb: [u16; 3], w: u32, h: u32) {
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for _ in 0..(w * h) {
        data.extend_from_slice(&rgb);
    }
    let mut buf = Vec::new();
    {
        let mut enc = TiffEncoder::new(std::io::Cursor::new(&mut buf)).unwrap();
        enc.write_image::<colortype::RGB16>(w, h, &data).unwrap();
    }
    std::fs::write(path, &buf).unwrap();
}

/// Minimal synthetic SilverFast XMP packet (the real one is ~150 KB; only the
/// `Silverfast:` mode attributes matter). `attrs` is the attribute list on the
/// `rdf:Description` element.
fn silverfast_xmp(attrs: &str) -> String {
    format!(
        "<?xpacket begin=\"\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
         <rdf:Description rdf:about=\"\" xmlns:Silverfast=\"LSI/\" {attrs}/>\
         </rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
    )
}

/// Write an 8x8 RGB16 TIFF with optional SilverFast XMP (tag 700), an optional
/// `Software` tag, and an optional matching Gray16 IR page — the levers the
/// provenance gate keys on (and the two holes the adversarial review flagged:
/// Software-only and IR-only).
fn write_rgb16(path: &Path, xmp: Option<&str>, software: Option<&str>, with_ir: bool) {
    use tiff::tags::Tag;
    let (w, h) = (8u32, 8u32);
    let rgb = vec![20000u16; (w * h * 3) as usize];
    let mut enc = TiffEncoder::new(std::fs::File::create(path).unwrap()).unwrap();
    let mut image = enc.new_image::<colortype::RGB16>(w, h).unwrap();
    if let Some(s) = software {
        image.encoder().write_tag(Tag::Software, s).unwrap();
    }
    if let Some(x) = xmp {
        image
            .encoder()
            .write_tag(Tag::Unknown(700), x.as_bytes())
            .unwrap();
    }
    image.write_data(&rgb).unwrap();
    if with_ir {
        let ir = vec![0u16; (w * h) as usize];
        enc.write_image::<colortype::Gray16>(w, h, &ir).unwrap();
    }
}

/// Write an HDRi-shaped TIFF: an RGB16 image plus a **marker-verified** Gray16 IR
/// page (`NewSubfileType = 4`, the provenance the holder march requires),
/// filled with one uniform IR level.
fn write_hdri_with_uniform_ir(path: &Path, w: u32, h: u32, rgb: [u16; 3], ir: u16) {
    let mut pixels = Vec::with_capacity((w * h * 3) as usize);
    for _ in 0..(w * h) {
        pixels.extend_from_slice(&rgb);
    }
    write_hdri(path, w, h, &pixels, &vec![ir; (w * h) as usize]);
}

/// Write an HDRi-shaped TIFF from explicit RGB and IR buffers.
fn write_hdri(path: &Path, w: u32, h: u32, pixels: &[u16], plane: &[u16]) {
    use tiff::tags::Tag;
    assert_eq!(pixels.len(), (w * h * 3) as usize);
    assert_eq!(plane.len(), (w * h) as usize);
    let mut enc = TiffEncoder::new(std::fs::File::create(path).unwrap()).unwrap();
    let image = enc.new_image::<colortype::RGB16>(w, h).unwrap();
    image.write_data(pixels).unwrap();

    let mut ir_page = enc.new_image::<colortype::Gray16>(w, h).unwrap();
    ir_page
        .encoder()
        .write_tag(Tag::NewSubfileType, 4u32)
        .unwrap();
    ir_page.write_data(plane).unwrap();
}

/// An HDRi scan of an **unexposed frame** in its holder: a rippled field of
/// unexposed film (~0.53 / 0.26 / 0.16) inside a holder ring that is deeper on the
/// left (12 px) than on the right (4 px), top and bottom 6 px. `ir_film` sets the
/// IR transmission of the film itself, which is what the usability verdict
/// measures; the holder is IR-dark. `holder_rgb` is the holder's RGB, so a test can
/// paint it any value and show it contributes nothing.
fn write_hdri_unexposed(path: &Path, ir_film: u16, holder_rgb: [u16; 3]) {
    const W: u32 = 200;
    const H: u32 = 200;
    const FILM: [f32; 3] = [34734.0, 17040.0, 10486.0];
    const IR_HOLDER: u16 = 1300; // ~0.02, as real holders measure
    let mut rgb = Vec::with_capacity((W * H * 3) as usize);
    let mut ir = vec![ir_film; (W * H) as usize];
    for y in 0..H {
        for x in 0..W {
            let holder = !(6..H - 6).contains(&y) || !(12..W - 4).contains(&x);
            if holder {
                rgb.extend_from_slice(&holder_rgb);
                ir[(y * W + x) as usize] = IR_HOLDER;
            } else {
                // A ±4% grain-like ripple, so the median has something to choose.
                let f = 0.96 + 0.08 * ((x * 7 + y * 13) % 17) as f32 / 16.0;
                rgb.extend(FILM.map(|c| (c * f) as u16));
            }
        }
    }
    write_hdri(path, W, H, &rgb, &ir);
}

/// A unique temp directory that removes itself (and its contents) on drop, so a
/// failing test can't leak output TIFFs.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("nc-e2e-{}-{tag}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run `nc` with `args`; return (exit code, stdout, stderr).
///
/// Passes `args` through verbatim: a test states any destination flag itself. An
/// injected default would silently turn a test meant for the default path into a test
/// of another one.
fn run(args: &[&str]) -> (i32, String, String) {
    spawn(args, &[])
}

/// Identity roll gains and exposure and a stated contrast, for a `--strict` test whose
/// subject is not the roll: without them the `default` rendering warns that it fell back,
/// or that the roll has no exposure, and `--strict` fails on that instead.
const MEASURED: [&str; 6] = [
    "--roll-white-balance",
    "1,1,1",
    "--roll-exposure",
    "0",
    "--contrast",
    "1.1111112",
];

/// Like [`run`], but with extra environment variables set for the child (used to
/// point `NC_TELEMETRY_LOG` at a temp file so telemetry tests never touch the
/// real user data dir).
fn run_env(args: &[&str], envs: &[(&str, &str)]) -> (i32, String, String) {
    spawn(args, envs)
}

/// Spawn `nc` verbatim — the one place that runs the binary. `NC_TELEMETRY=0` keeps
/// the machine's own upload consent, if any, out of every test.
fn spawn(args: &[&str], envs: &[(&str, &str)]) -> (i32, String, String) {
    let mut cmd = Command::new(NC);
    cmd.args(args).env("NC_TELEMETRY", "0");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("failed to spawn nc binary");
    (
        out.status.code().expect("process terminated by signal"),
        String::from_utf8(out.stdout).expect("stdout is not UTF-8"),
        String::from_utf8(out.stderr).expect("stderr is not UTF-8"),
    )
}

/// Like [`run`], with `input` piped to the child's stdin.
fn run_stdin(args: &[&str], input: &str) -> (i32, String, String) {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(NC)
        .args(args)
        .env("NC_TELEMETRY", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn nc binary");
    // A child that refuses before reading closes the pipe; that is its answer.
    let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
    let out = child.wait_with_output().expect("failed to wait on nc");
    (
        out.status.code().expect("process terminated by signal"),
        String::from_utf8(out.stdout).expect("stdout is not UTF-8"),
        String::from_utf8(out.stderr).expect("stderr is not UTF-8"),
    )
}

/// Parse stdout as JSON, failing with the raw text if it isn't clean JSON.
fn json(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout)
        .unwrap_or_else(|e| panic!("stdout is not valid JSON ({e}):\n{stdout}"))
}

/// The sidecar path for an output (`out.tiff` → `out.tiff.json`).
fn sidecar_of(output: &Path) -> PathBuf {
    PathBuf::from(format!("{}.json", output.display()))
}

/// Writes the `{meta, params}` sidecar a pre-flip build left beside `output`, with
/// `params` as its recipe body. No build writes one any more; a run replacing the
/// image removes it as stale.
fn write_stale_sidecar(output: &Path, params: serde_json::Value) {
    let doc = serde_json::json!({
        "meta": {
            "nc_version": "0.1.0",
            "pipeline_version": 7,
            "target": "aarch64-apple-darwin",
            "params_hash": "0000000000000000"
        },
        "params": params
    });
    std::fs::write(
        sidecar_of(output),
        serde_json::to_string_pretty(&doc).unwrap(),
    )
    .unwrap();
}

/// The container a written file's *bytes* actually are, sniffed from its magic.
///
/// The destination's container decides the suffix an output path is **named** for; a
/// separate exhaustive match on its encoding decides which encoder writes it. Both are
/// exhaustive, so a new destination fails to compile in both — but nothing makes them
/// *agree*, and a destination named `.tiff` while dispatched to the JPEG encoder would
/// compile and ship a misnamed file. This is what pins the two together, at the only
/// level that matters: the bytes on disk.
fn sniff_container(path: &Path) -> &'static str {
    let bytes = std::fs::read(path).unwrap();
    assert!(bytes.len() > 12, "{} is too short to sniff", path.display());
    if bytes[0..2] == [0xff, 0xd8] {
        "jpeg"
    } else if (&bytes[0..2] == b"II" || &bytes[0..2] == b"MM")
        && matches!(
            u16::from_le_bytes([bytes[2], bytes[3]]),
            42 | 43 | 0x2a00 | 0x2b00
        )
    {
        "tiff"
    } else {
        panic!(
            "{}: unrecognised container magic {:?}",
            path.display(),
            &bytes[0..12]
        );
    }
}

/// A file that starts with the little-endian TIFF magic ("II", 42 or 43).
fn is_tiff(path: &Path) -> bool {
    let bytes = std::fs::read(path).unwrap();
    bytes.len() > 4
        && &bytes[0..2] == b"II"
        && matches!(u16::from_le_bytes([bytes[2], bytes[3]]), 42 | 43)
}

/// An HDR container whose signal never rises above the 203-nit reference white is
/// an HDR wrapper around an SDR picture: it costs bit depth and compatibility and
/// buys nothing, while the report still advertises `target_peak_nits: 1000`. Every
/// single-rendition HDR destination must say so, and must stop saying so as soon as the
/// frame actually uses the headroom.
#[test]
fn single_rendition_hdr_destinations_warn_when_the_signal_stays_below_reference_white() {
    const MARKER: &str = "HDR output carries an SDR-range signal";
    let tmp = TempDir::new("hdr-sdr-range");
    let warnings = |stdout: &str| -> Vec<String> {
        json(stdout)["warnings"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|w| w.as_str().unwrap().to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let input = fixture("hdr-48bit.tif");
    let convert = |destination: &[&str], out: &Path, extra: &[&str]| {
        let mut argv = vec![
            "convert",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "1,1,1",
        ];
        argv.extend_from_slice(&MEASURED);
        argv.extend_from_slice(destination);
        argv.extend_from_slice(extra);
        run(&argv)
    };

    for (preset, destination, ext) in [
        ("pq-tiff", &["--transfer", "pq"][..], "tif"),
        ("hlg-tiff", &["--transfer", "hlg"][..], "tif"),
        (
            "linear-tiff",
            &["--transfer", "linear", "--gamut", "bt2020"][..],
            "tif",
        ),
    ] {
        // Three stops down, this frame peaks under reference white — in a container
        // signalling HDR.
        let low = tmp.path(&format!("{preset}-low.{ext}"));
        let (code, stdout, err) = convert(destination, &low, &["--exposure=-5"]);
        assert_eq!(code, 0, "{err}");
        assert!(
            warnings(&stdout).iter().any(|w| w.contains(MARKER)),
            "{preset} must warn that its HDR signal is SDR-range: {:?}",
            warnings(&stdout)
        );

        // The falsifiable control: at the default exposure the same frame does reach
        // past reference white, so the warning must disappear. Without this the
        // assertion above would pass equally for a warning that always fires. The
        // `--strict` here is a second assertion — `hdr-48bit.tif` is the IR-free
        // fixture, so exit 0 proves the run raised *no* promotable warning at all.
        let high = tmp.path(&format!("{preset}-high.{ext}"));
        let (code, stdout, err) = convert(destination, &high, &["--strict"]);
        assert_eq!(code, 0, "{err}");
        assert!(
            !warnings(&stdout).iter().any(|w| w.contains(MARKER)),
            "{preset} must not warn when content exceeds reference white: {:?}",
            warnings(&stdout)
        );
    }

    // `--strict` promotes it. One destination is enough: promotion is the shared
    // `push_warning_buf` path, not anything per-destination.
    let strict = tmp.path("strict.tif");
    let (code, _stdout, err) = convert(
        &["--transfer", "pq"],
        &strict,
        &["--exposure=-5", "--strict"],
    );
    assert_eq!(
        code, 1,
        "--strict must promote the SDR-range warning: {err}"
    );
    assert!(err.contains(MARKER), "{err}");

    // The gain-map JPEG is dual-rendition — an SDR base image plus a gain map, so a
    // low-headroom render yields a flat gain map rather than a mislabelled HDR
    // container. Different artifact, different diagnosis; this warning stays off it.
    let gain_map = tmp.path("gain-map.jpg");
    let (code, stdout, err) = convert(&["--range", "hdr"], &gain_map, &["--exposure=-5"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        !warnings(&stdout).iter().any(|w| w.contains(MARKER)),
        "the gain-map JPEG must not carry the single-rendition HDR warning: {:?}",
        warnings(&stdout)
    );
}

#[test]
fn hdr_linear_tiff_writes_a_bit_exact_display_linear_bt2020_master() {
    use tiff::decoder::{Decoder, DecodingResult};
    use tiff::tags::Tag;

    let tmp = TempDir::new("hdr-linear-tiff");
    let first = tmp.path("first.tif");
    let second = tmp.path("second.TIFF");
    for output in [&first, &second] {
        // `hdr-48bit.tif` is the IR-free fixture, so `--strict` is a real assertion
        // here: the run must produce *no* promotable warning at all. On the HDRi
        // fixture every run trips the "IR preserved but not used" warning and this
        // would prove nothing.
        let (code, stdout, err) = run(&[
            &[
                "convert",
                fixture("hdr-48bit.tif").to_str().unwrap(),
                "-o",
                output.to_str().unwrap(),
                "--transfer",
                "linear",
                "--gamut",
                "bt2020",
                "--film-base",
                "1,1,1",
                "--strict",
            ][..],
            &MEASURED,
        ]
        .concat());
        assert_eq!(code, 0, "{err}");
        let report = json(&stdout);
        let axes = &report["chain"]["destination"]["display"];
        assert_eq!(axes["transfer"], "linear");
        assert_eq!(axes["gamut"], "bt2020");
        // The chain ran, which is what distinguishes this from the film master.
        assert_eq!(report["chain"]["stages"][1]["stage"], "look");

        let block = &report["hdr_linear_tiff"];
        assert_eq!(
            block["pixel_contract"],
            "rgb-f32-display-linear-bt2020-d65-relative-to-203-nit-reference-white"
        );
        assert_eq!(block["bits_per_sample"], 32);
        assert_eq!(block["sample_format"], 3, "3 == IEEE float");
        assert_eq!(block["bigtiff"], false);
        assert_eq!(block["reference_white_sample"], 1.0);
        assert_eq!(block["reference_white_nits"], 203.0);
        assert_eq!(block["target_peak_nits"], 1000.0);
        assert!(block["icc_bytes"].as_u64().unwrap() > 0);
        let headroom = block["linear_headroom"].as_f64().unwrap();
        assert!(
            (headroom - 1000.0 / 203.0).abs() < 1e-6,
            "headroom {headroom} is not 1000/203"
        );
        // Measured content light, not the mastering policy: a real frame must not
        // report the 1000/203 constants back.
        let cll = block["max_cll_nits"].as_u64().unwrap();
        let fall = block["max_fall_nits"].as_u64().unwrap();
        assert!(fall <= cll, "MaxFALL {fall} exceeds MaxCLL {cll}");
        assert!(cll <= 1000, "MaxCLL {cll} above the mastering peak");
        assert!(
            cll != 1000 || fall != 203,
            "content light looks like the policy constants, not a measurement"
        );
    }

    // Same build, same input ⇒ byte-identical (the ICC dateTime is zeroed).
    assert_eq!(
        std::fs::read(&first).unwrap(),
        std::fs::read(&second).unwrap(),
        "repeated hdr-linear-tiff encodes must be byte-identical"
    );

    // Independently decode the file and check the storage contract plus the linear
    // domain. A PQ-encoded frame would have no sample above 1.0 at all, so the
    // headroom assertion is what proves no transfer was applied.
    let bytes = std::fs::read(&first).unwrap();
    let mut decoder = Decoder::new(std::io::Cursor::new(&bytes)).unwrap();
    assert!(
        decoder.get_tag_u8_vec(Tag::IccProfile).is_ok(),
        "no embedded ICC profile"
    );
    let samples = match decoder.read_image().unwrap() {
        DecodingResult::F32(data) => data,
        other => panic!("expected 32-bit float samples, got {other:?}"),
    };
    assert!(samples.iter().all(|v| v.is_finite()));
    let max = samples.iter().copied().fold(f32::MIN, f32::max);
    assert!(
        max > 1.0,
        "no sample above the 203-nit reference white ({max}); either the fixture \
         has no highlights or a transfer/clamp was applied"
    );
    assert!(
        max <= 1000.0 / 203.0 + 1e-6,
        "sample {max} exceeds the 1000-nit peak"
    );
}

#[test]
fn coded_hdr_tiffs_store_exact_codes_and_signal_cicp_in_the_profile() {
    use tiff::decoder::{Decoder, DecodingResult};
    use tiff::tags::Tag;

    let tmp = TempDir::new("hdr-coded-tiff");
    for (preset, transfer_code, expect_hlg) in [("pq", 16u64, false), ("hlg", 18, true)] {
        let output = tmp.path(&format!("{preset}.tif"));
        let (code, stdout, err) = run(&[
            &[
                "convert",
                fixture("hdr-48bit.tif").to_str().unwrap(),
                "-o",
                output.to_str().unwrap(),
                "--transfer",
                preset,
                "--film-base",
                "1,1,1",
                // Exit 0 under `--strict` on the IR-free fixture means *no* promotable
                // warning — including the SDR-range one
                // (`single_rendition_hdr_destinations_warn_when_the_signal_stays_below_reference_white`),
                // which has nothing to do with PQ/HLG code storage.
                "--strict",
            ][..],
            &MEASURED,
        ]
        .concat());
        assert_eq!(code, 0, "{preset}: {err}");
        let report = json(&stdout);
        assert_eq!(
            report["chain"]["destination"]["display"]["transfer"],
            preset
        );

        let block = &report["hdr_coded_tiff"];
        assert_eq!(block["bits_per_sample"], 16);
        assert_eq!(block["sample_format"], 1, "1 == unsigned integer");
        assert_eq!(block["full_range"], true);
        assert_eq!(block["cicp"][0], 9, "BT.2020 primaries");
        assert_eq!(block["cicp"][1], transfer_code);
        // An RGB ICC profile requires MatrixCoefficients 0, not Y'CbCr's 9.
        assert_eq!(block["cicp"][2], 0);
        assert_eq!(block["reference_white_nits"], 203.0);
        assert_eq!(block["target_peak_nits"], 1000.0);
        // Rounding cannot cost more than half a code, and the report must say so
        // with a real measurement rather than a constant.
        let max = block["max_quantization_error_codes"].as_f64().unwrap();
        let rms = block["rms_quantization_error_codes"].as_f64().unwrap();
        assert!(max > 0.0 && max <= 0.5, "{preset}: max error {max}");
        assert!(rms > 0.0 && rms <= max, "{preset}: rms {rms} vs max {max}");
        // Truthful naming, in the artifact.
        let notes = block["interoperability"].as_str().unwrap();
        assert!(notes.contains("limited-interoperability"), "{notes}");
        assert!(
            notes.contains("not one of BT.2100's specified bit depths"),
            "{notes}"
        );
        // HLG carries its reference-display assumptions; PQ has none to carry.
        assert_eq!(block["hlg_system_gamma"].is_null(), !expect_hlg);
        // No clipping and no non-finite: the domain is verified before quantizing,
        // so `--strict` (exit 0 above) is a real assertion on the IR-free fixture.
        assert_eq!(report["loss"]["clipped_high"], 0);
        assert_eq!(report["loss"]["non_finite"], 0);

        // Independently decode: 16-bit unsigned samples plus an embedded profile
        // whose `cicp` tag a third-party reader can find.
        let bytes = std::fs::read(&output).unwrap();
        let mut decoder = Decoder::new(std::io::Cursor::new(&bytes)).unwrap();
        let icc = decoder
            .get_tag_u8_vec(Tag::IccProfile)
            .expect("no embedded ICC profile");
        // Walk the ICC tag table (count at byte 128, then 12-byte
        // signature/offset/size entries) to the `cicp` tag data, rather than
        // scanning for the bytes — the first `cicp` in the file is the *table
        // entry*, whose next four bytes are an offset, not the reserved zeros.
        let count = u32::from_be_bytes(icc[128..132].try_into().unwrap()) as usize;
        let mut cicp_at = None;
        for i in 0..count {
            let entry = 132 + i * 12;
            if &icc[entry..entry + 4] == b"cicp" {
                let offset = u32::from_be_bytes(icc[entry + 4..entry + 8].try_into().unwrap());
                let size = u32::from_be_bytes(icc[entry + 8..entry + 12].try_into().unwrap());
                assert_eq!(size, 12, "cicpType is a 12-byte structure");
                cicp_at = Some(offset as usize);
            }
        }
        let tag_at = cicp_at.expect("no cicp tag in the embedded profile");
        let tag = &icc[tag_at..tag_at + 12];
        assert_eq!(&tag[0..4], b"cicp", "cicpType signature");
        assert_eq!(&tag[4..8], &[0, 0, 0, 0], "reserved bytes must be zero");
        assert_eq!(tag[8], 9, "ColourPrimaries");
        assert_eq!(u64::from(tag[9]), transfer_code, "TransferCharacteristics");
        assert_eq!(tag[10], 0, "MatrixCoefficients must be 0 for RGB");
        assert_eq!(tag[11], 1, "VideoFullRangeFlag");

        let samples = match decoder.read_image().unwrap() {
            DecodingResult::U16(data) => data,
            other => panic!("{preset}: expected u16 samples, got {other:?}"),
        };
        // A real frame must use a wide part of the code range, and PQ/HLG place a
        // 203-nit white well below full scale — so a file pinned at 65535 would mean
        // the transfer was skipped.
        let max_code = samples.iter().copied().max().unwrap();
        assert!(
            max_code > 1000,
            "{preset}: max code {max_code} is implausibly low"
        );
    }

    // The two transfers must produce genuinely different files from one input.
    let pq = std::fs::read(tmp.path("pq.tif")).unwrap();
    let hlg = std::fs::read(tmp.path("hlg.tif")).unwrap();
    assert_ne!(pq, hlg, "PQ and HLG TIFFs must differ");
}

#[test]
fn hdr_linear_tiff_rejects_a_non_tiff_path_and_conflicting_flags() {
    let tmp = TempDir::new("hdr-linear-reject");
    let base = [
        "convert",
        "--transfer",
        "linear",
        "--gamut",
        "bt2020",
        "--film-base",
        "1,1,1",
    ];
    let input = fixture("hdr-48bit.tif");

    // Wrong suffix: exit 2 and the path is never rewritten.
    let jpg = tmp.path("out.jpg");
    let mut args = vec![
        base[0],
        input.to_str().unwrap(),
        "-o",
        jpg.to_str().unwrap(),
    ];
    args.extend_from_slice(&base[1..]);
    let (code, _, err) = run(&args);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains(".tif"), "{err}");
    assert!(!jpg.exists(), "a rejected run must write nothing");

    // The retired `--out-depth` is a migration error, never a synonym for this
    // destination's own f32.
    let out = tmp.path("out.tif");
    let mut args = vec![
        base[0],
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ];
    args.extend_from_slice(&base[1..]);
    args.push("--out-depth");
    args.push("f32");
    let (code, _, err) = run(&args);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("was removed"), "{err}");
    assert!(!out.exists(), "a rejected run must write nothing");
}

#[test]
fn avif_is_refused_as_removed_wherever_it_is_stated() {
    // A flag, an output suffix, a replayed recipe, a roll's shared recipe, a manifest
    // path and a per-frame override each name the removal and the TIFF, never "unknown
    // value" or a completed `out.avif.tiff`. A recipe's is worded by its key: no flag
    // can rescue a recipe refused before merge.
    let tmp = TempDir::new("avif-removed");
    let input = fixture("hdr-48bit.tif");
    let refused = |argv: &[&str], written: &Path| {
        let (code, _stdout, err) = run(argv);
        assert_eq!(code, 2, "{argv:?}: {err}");
        assert!(err.contains("no longer writes AVIF"), "{argv:?}: {err}");
        assert!(!err.contains("unknown"), "{argv:?}: {err}");
        assert!(!written.exists(), "{argv:?}: nothing may be written");
        err
    };
    let by_key = |err: &str| {
        assert!(
            err.contains(r#"`output.display.container` "avif" was removed"#),
            "{err}"
        );
        assert!(err.contains(r#"State `"container": "tiff"`"#), "{err}");
        assert!(!err.contains("--container avif"), "{err}");
        assert!(!err.contains("at line"), "{err}");
    };
    let tiff = tmp.path("pq.tiff");
    let flag = [
        "convert",
        input.to_str().unwrap(),
        "-o",
        tiff.to_str().unwrap(),
        "--transfer",
        "pq",
        "--container",
        "avif",
        "--film-base",
        "1,1,1",
    ];
    let err = refused(&flag, &tiff);
    assert!(err.contains("--container avif was removed"), "{err}");
    let avif = tmp.path("pq.avif");
    let suffix = [
        "convert",
        input.to_str().unwrap(),
        "-o",
        avif.to_str().unwrap(),
        "--transfer",
        "pq",
        "--film-base",
        "1,1,1",
    ];
    refused(&suffix, &avif);
    assert!(
        !tmp.path("pq.avif.tiff").exists(),
        "never completed as a stem"
    );
    // The suffix is a property of this invocation: it outranks the missing base.
    let err = refused(&suffix[..suffix.len() - 2], &avif);
    assert!(!err.contains("no film base selected"), "{err}");

    let recipe = tmp.path("avif.json");
    std::fs::write(
        &recipe,
        r#"{"recipe_version":3,"output":{"display":{"transfer":"pq","container":"avif"}},
            "calibration":{"film_base":{"explicit":[1,1,1]}}}"#,
    )
    .unwrap();
    let replayed = [
        "convert",
        input.to_str().unwrap(),
        "-o",
        tiff.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
        "--container",
        "tiff",
    ];
    by_key(&refused(&replayed, &tiff));
    let out_dir = tmp.path("roll-out");
    std::fs::create_dir_all(&out_dir).unwrap();
    let roll = [
        "roll",
        input.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ];
    by_key(&refused(&roll, &out_dir.join("hdr-48bit_positive.avif")));
    assert_eq!(
        std::fs::read_dir(&out_dir).unwrap().count(),
        0,
        "roll wrote nothing"
    );

    // A roll manifest's entry: its path, and its per-frame override, name the frame.
    let manifest = |entry: &str| {
        let path = tmp.path("frames.json");
        std::fs::write(
            &path,
            format!(
                r#"{{"frames": [{{"input": "{}", {entry}}}]}}"#,
                input.display()
            ),
        )
        .unwrap();
        path
    };
    let frame = format!("frame {}: ", input.display());
    for (entry, written) in [
        (r#""output": "x.avif""#, out_dir.join("x.avif")),
        (
            r#""params": {"output": {"display": {"container": "avif"}}}"#,
            out_dir.join("hdr-48bit_positive.tiff"),
        ),
    ] {
        let frames = manifest(entry);
        let err = refused(
            &[
                "roll",
                "--frames",
                frames.to_str().unwrap(),
                "--out-dir",
                out_dir.to_str().unwrap(),
                "--film-base",
                "1,1,1",
                "--transfer",
                "pq",
            ],
            &written,
        );
        assert!(err.contains(&frame), "{entry}: {err}");
        if entry.contains("params") {
            assert!(err.contains("per-frame `params` override"), "{err}");
            by_key(&err);
        } else {
            assert!(err.contains("the output path"), "{err}");
        }
        assert_eq!(
            std::fs::read_dir(&out_dir).unwrap().count(),
            0,
            "{entry}: roll wrote nothing"
        );
    }
}

/// Every `*.nctmp` staging file left in `dir` — the litter check that must come back
/// empty after any failure (`io/transactional-output-writes`).
fn staging_temps(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .expect("temp dir readable")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "nctmp"))
        .collect()
}

#[test]
fn a_failing_ir_export_leaves_no_primary_output() {
    // IR is staged before the primary, so its failure must abort the whole set. The
    // ordering trick that used to provide this (export IR first) only ever helped
    // because IR came first; now it holds because nothing is committed until all
    // three artifacts exist.
    let tmp = TempDir::new("ir-fails");
    let out = tmp.path("out.tiff");
    let ir = tmp.path("ir.tiff");
    std::fs::create_dir(&ir).expect("occupy the IR path");

    let (code, _stdout, err) = run(&[
        "convert",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--export-ir",
        ir.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    assert_ne!(code, 0, "a failing IR export must fail the run: {err}");
    assert!(
        !out.exists(),
        "no primary output for an aborted artifact set"
    );
    assert!(
        staging_temps(&tmp.0).is_empty(),
        "no staging temps survive: {:?}",
        staging_temps(&tmp.0)
    );
}

#[test]
fn an_interrupted_overwrite_leaves_the_previous_output_intact() {
    // The decided contract is atomic *replace*: `nc` keeps overwriting its own
    // output. What must never happen is a truncated new file where a valid old one
    // was — so a run that fails after the primary is encoded must leave the previous
    // bytes untouched, not a half-written TIFF.
    let tmp = TempDir::new("overwrite");
    let out = tmp.path("out.tiff");
    let ir = tmp.path("ir.tiff");
    let input = fixture("hdri-64bit.tif");
    let args = [
        "convert",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--export-ir",
        ir.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ];
    let (code, _o, _e) = run(&args);
    assert_eq!(code, 0, "first conversion should succeed");
    let original = std::fs::read(&out).expect("first output readable");

    // Now make the IR path unwritable so the second run fails after encoding.
    std::fs::remove_file(&ir).expect("remove the first IR export");
    std::fs::create_dir(&ir).expect("occupy the IR path");
    let (code, _o, err) = run(&args);
    assert_ne!(code, 0, "the second run must fail: {err}");
    assert_eq!(
        std::fs::read(&out).expect("previous output still readable"),
        original,
        "an interrupted overwrite must leave the OLD file intact, byte for byte"
    );
    assert!(staging_temps(&tmp.0).is_empty(), "no staging temps survive");
}

#[test]
fn a_successful_run_leaves_no_staging_temps() {
    // The success path's half of the litter check: every temp is consumed by its
    // rename, so a normal conversion leaves exactly the artifacts and nothing else.
    let tmp = TempDir::new("no-litter");
    let out = tmp.path("out.tiff");
    let ir = tmp.path("ir.tiff");
    let report = tmp.path("report.json");
    let (code, _stdout, err) = run(&[
        "convert",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--export-ir",
        ir.to_str().unwrap(),
        "--report-file",
        report.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    assert_eq!(code, 0, "conversion should succeed: {err}");
    for artifact in [&out, &ir, &report] {
        assert!(artifact.exists(), "missing artifact {}", artifact.display());
    }
    assert!(
        staging_temps(&tmp.0).is_empty(),
        "a successful run must leave no temps: {:?}",
        staging_temps(&tmp.0)
    );
}

#[test]
fn convert_writes_tiff_and_report() {
    let tmp = TempDir::new("convert-basic");
    let out = tmp.path("out.tiff");
    let (code, stdout, _err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        // An explicit base: the documented calibrate-once workflow.
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    assert_eq!(code, 0, "convert should succeed");
    assert!(is_tiff(&out), "output must be a valid TIFF");
    // No sidecar: the report's `recipe` is the record of the run.
    assert!(!sidecar_of(&out).exists());

    let report = json(&stdout);
    assert_eq!(report["command"], "convert");
    assert_eq!(report["recipe"]["recipe_version"], 3, "{stdout}");
    assert!(report["chain"].get("sidecar_written").is_none(), "{stdout}");
    assert_eq!(
        report["chain"]["destination"]["display"]["container"],
        "tiff"
    );
    // The pinned working-space mapping is stamped on every convert report
    // (design-spec §8).
    assert_eq!(report["working_mapping"], "nc-film-rgb-v1");
    assert_eq!(report["output"], out.to_str().unwrap());
    assert!(report["film_base"].is_object(), "film base reported");
    assert!(report["loss"].is_object(), "encode loss reported");
    assert!(report["elapsed_ms"].is_number());
}

#[test]
fn u16_clipping_is_reported_and_strict_promotes_it() {
    // Force guaranteed u16 clipping with a large positive `--print-exposure`
    // (2^12× gain blows every highlight past 1.0), so this test pins the
    // clip-reporting + `--strict` mechanism *independently* of the density
    // default's baseline exposure.
    // The HDR fixture carries no IR plane, so the only warning is the clipping —
    // proving clipping alone drives the strict failure.
    let tmp = TempDir::new("u16-clip");
    let base_args = |extra: &[&str], out: &Path| {
        let mut v = vec![
            "convert",
            "__IN__",
            "-o",
            "__OUT__",
            "--film-base",
            "0.9,0.55,0.42",
            "--exposure",
            "12",
        ];
        v.extend_from_slice(extra);
        v.into_iter()
            .map(|s| match s {
                "__IN__" => fixture("hdr-48bit.tif").to_str().unwrap().to_string(),
                "__OUT__" => out.to_str().unwrap().to_string(),
                other => other.to_string(),
            })
            .collect::<Vec<_>>()
    };

    // Non-strict: clipping is a warning, the run still succeeds.
    let out = tmp.path("out.tiff");
    let argv = base_args(&[], &out);
    let (code, stdout, _err) = run(&argv.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(code, 0, "non-strict clipping run should still succeed");
    let report = json(&stdout);
    assert!(
        report["loss"]["clipped_high"].as_u64().unwrap() > 0,
        "a +12-stop exposure must clip highlights: {report}"
    );
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("clipped")),
        "a clipping warning must be reported: {report}"
    );

    // Strict: the clipping warning becomes a non-zero exit (exactly 1, Other).
    let out2 = tmp.path("out2.tiff");
    let argv = base_args(&["--strict"], &out2);
    let (code, _stdout, err) = run(&argv.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(
        code, 1,
        "--strict must fail (exit 1) when a warning is present"
    );
    assert!(
        err.contains("strict"),
        "stderr should explain the strict failure: {err}"
    );
}

#[test]
fn inspect_reports_decode_facts() {
    let (code, stdout, _err) = run(&["inspect", fixture("hdri-64bit.tif").to_str().unwrap()]);
    assert_eq!(code, 0);
    let report = json(&stdout);
    assert_eq!(report["command"], "inspect");
    assert_eq!(report["decode"]["format"], "hdri");
    assert_eq!(report["decode"]["width"], 502);
    assert_eq!(report["decode"]["height"], 462);
    assert_eq!(report["decode"]["ir_present"], true);
    // No image is written by inspect.
    assert!(report["output"].is_null());
}

#[test]
fn estimate_from_region_reports_film_base() {
    let (code, stdout, _err) = run(&[
        "measure-base",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "--base-region",
        "0,0,60,60",
    ]);
    assert_eq!(code, 0, "region estimate should succeed:\n{stdout}");
    let report = json(&stdout);
    assert_eq!(report["command"], "measure-base");
    assert!(report["film_base"]["r"].is_number());
    assert!(report["film_base"]["g"].is_number());
    assert!(report["film_base"]["b"].is_number());
    // Structured source: {"region":[x,y,w,h]}, so the sampled rect is machine-readable.
    assert_eq!(
        report["film_base_source"]["region"],
        serde_json::json!([0, 0, 60, 60])
    );
}

#[test]
fn mixed_base_region_warns_and_strict_refuses_it() {
    // A rectangle mixing image content is a plausible-looking bad base; the
    // uniformity warning must ride the report (estimate), and --strict must
    // promote it to a failure (convert) — while the non-strict convert still
    // succeeds with the warning recorded.
    let (code, stdout, _err) = run(&[
        "measure-base",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "--base-region",
        "0,0,502,462",
    ]);
    assert_eq!(code, 0, "a mixed region is a warning, not an error");
    let report = json(&stdout);
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("not uniform")),
        "uniformity warning expected: {report}"
    );

    let tmp = TempDir::new("region-warn");
    let out = tmp.path("out.tiff");
    let (code, _stdout, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--base-region",
        "0,0,502,462",
        "--strict",
    ]);
    assert_eq!(
        code, 1,
        "--strict must refuse a non-uniform base region: {err}"
    );

    // `estimate --strict` refuses it too — the command that bakes the Dmin a
    // roll is calibrated on must not echo a plausible-looking-but-bad base.
    let (code, _stdout, err) = run(&[
        "measure-base",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "--base-region",
        "0,0,502,462",
        "--strict",
    ]);
    assert_eq!(
        code, 1,
        "estimate --strict must refuse a mixed region: {err}"
    );
}

#[test]
fn measure_base_out_writes_a_recipe_that_round_trips() {
    // The calibrate-once → reuse workflow (design-spec §8): `measure-base` reports the
    // measured base as a paste-ready `--film-base` flag and, with `--out`, writes it as
    // a recipe; feeding either back to `convert` must reproduce the exact same base
    // (and thus a byte-identical output).
    let tmp = TempDir::new("reuse");
    let fix = fixture("hdr-48bit.tif");
    // Focus: the reuse round-trip. (This real-photo fixture has no
    // region-uniform patch, so the inward-scan uniformity check warns on any
    // `--base-region` here — which the `--strict` case below relies on.)
    let region = ["--base-region", "0,0,60,60"];
    let recipe = tmp.path("base.json");
    let measure = |extra: &[&str]| {
        run(&[
            &["measure-base", fix.to_str().unwrap()][..],
            &region,
            &["--out", recipe.to_str().unwrap()],
            extra,
        ]
        .concat())
    };
    // `--strict` fails on the warning after the report lands, and writes no recipe.
    let (code, stdout, err) = measure(&["--strict"]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("no recipe written"), "{err}");
    assert!(json(&stdout)["film_base_flag"].is_string());
    assert!(!recipe.exists(), "a failed run writes no recipe");

    let (code, stdout, err) = measure(&[]);
    assert_eq!(code, 0, "measure-base should succeed: {err}");
    let report = json(&stdout);
    assert_eq!(report["command"], "measure-base");
    let base = report["film_base"].clone();
    assert!(
        report.get("calibration").is_none(),
        "the recipe is the file, not a report field: {report}"
    );

    // The flag string is `--film-base R,G,B` with the measured values.
    let flag = report["film_base_flag"].as_str().expect("flag emitted");
    let value = flag.strip_prefix("--film-base ").expect("flag prefix");
    // The file states the measurement and nothing else, so a later layer is pinned by
    // nothing the run did not measure.
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&recipe).unwrap()).unwrap();
    assert_eq!(
        written,
        serde_json::json!({
            "recipe_version": 3,
            "calibration": {"film_base": {"explicit": [base["r"], base["g"], base["b"]]}}
        })
    );

    // An existing file is refused before anything is decoded, unless `--force`.
    let before = std::fs::read(&recipe).unwrap();
    let (code, _, err) = measure(&[]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("exists") && err.contains("--force"), "{err}");
    std::fs::write(&recipe, "stale").unwrap();
    let (code, _, err) = measure(&["--force"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        std::fs::read(&recipe).unwrap(),
        before,
        "--force replaces it"
    );
    // Nor may it overwrite the scan.
    let (code, _, err) = run(&[
        "measure-base",
        fix.to_str().unwrap(),
        "--out",
        fix.to_str().unwrap(),
        "--force",
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("would overwrite the input scan"), "{err}");

    // Round-trip A: the flag value fed to `convert` reproduces the base.
    let out_flag = tmp.path("flag.tiff");
    let (code, stdout, err) = run(&[
        "convert",
        fix.to_str().unwrap(),
        "-o",
        out_flag.to_str().unwrap(),
        "--film-master",
        "--film-base",
        value,
    ]);
    assert_eq!(code, 0, "{err}");
    let convert_report = json(&stdout);
    assert_eq!(
        convert_report["film_base"], base,
        "--film-base from the flag string must reproduce the measured base"
    );

    // Round-trip B: the written recipe reproduces the base and a byte-identical output
    // (determinism across the two reuse forms).
    let out_recipe = tmp.path("recipe.tiff");
    let (code, stdout, err) = run(&[
        "convert",
        fix.to_str().unwrap(),
        "-o",
        out_recipe.to_str().unwrap(),
        // The recipe states only a film base, so the destination comes from the
        // flag, matching round-trip A.
        "--film-master",
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "the written recipe must load: {err}");
    assert_eq!(json(&stdout)["film_base"], base);
    assert_eq!(
        std::fs::read(&out_flag).unwrap(),
        std::fs::read(&out_recipe).unwrap(),
        "flag and recipe reuse must produce byte-identical outputs"
    );
}

#[test]
fn estimate_warns_that_a_picture_frame_is_not_unexposed_film() {
    // The median over a picture's effective area is a plausible, wrong base: the
    // spread is what says so, loudly enough for `--strict` to refuse it.
    let fix = fixture("hdr-48bit.tif");
    let (code, stdout, err) = run(&["measure-base", fix.to_str().unwrap()]);
    assert_eq!(code, 0, "a non-uniform area is a warning, not fatal: {err}");
    let report = json(&stdout);
    assert_eq!(report["film_base_source"], "effective_area", "{report}");
    assert!(
        report["warnings"].as_array().unwrap().iter().any(|w| w
            .as_str()
            .unwrap()
            .contains("does not look like unexposed film")),
        "{report}"
    );
    // A 48-bit scan has no IR plane, so the holder is never measured and the area
    // rests on the inset alone — said, not left for the user to infer.
    assert!(
        report["warnings"].as_array().unwrap().iter().any(|w| {
            let w = w.as_str().unwrap();
            w.contains("the film holder was not measured") && w.contains("no IR plane")
        }),
        "{report}"
    );
    let (code, stdout, err) = run(&["measure-base", fix.to_str().unwrap(), "--strict"]);
    assert_eq!(code, 1, "--strict must fail on it");
    let _ = json(&stdout); // the report still lands on stdout before the gate
    assert!(err.contains("strict"), "stderr should explain: {err}");
}

#[test]
fn a_conversion_reports_the_percentile_its_base_was_read_at() {
    // The stage returns the method; the report states it rather than leaving a
    // region base to be mistaken for an explicit one.
    let dir = TempDir::new("convert-percentile");
    let fix = fixture("hdr-48bit.tif");
    let convert = |source: &[&str], name: &str| {
        let out = dir.path(name);
        let (code, stdout, err) = run(&[
            &[
                "convert",
                fix.to_str().unwrap(),
                "-o",
                out.to_str().unwrap(),
            ][..],
            source,
        ]
        .concat());
        assert_eq!(code, 0, "{err}");
        json(&stdout)
    };
    let region = convert(&["--base-region", "0,0,60,60"], "region.tif");
    assert_eq!(region["film_base_percentile"], 0.97, "{region}");
    let explicit = convert(&["--film-base", "0.9,0.55,0.42"], "explicit.tif");
    assert!(explicit.get("film_base_percentile").is_none(), "{explicit}");
}

#[test]
fn estimate_refuses_a_degenerate_base_from_either_source() {
    // An all-black frame is not a usable Dmin anchor, whether the base is read over
    // the effective area or a stated region: both hit the birth guard, exit 1.
    let fix = fixture("black-48bit.tif");
    for extra in [&[][..], &["--base-region", "0,0,32,32"][..]] {
        let (code, _stdout, err) =
            run(&[&["measure-base", fix.to_str().unwrap()][..], extra].concat());
        assert_eq!(code, 1, "{extra:?}: {err}");
        assert!(err.contains("finite and positive"), "{err}");
    }
}

#[test]
fn export_ir_writes_plane_for_hdri_and_errors_for_hdr() {
    let tmp = TempDir::new("ir");
    // HDRi: the IR plane is written.
    let out = tmp.path("out.tiff");
    let ir = tmp.path("ir.tiff");
    let (code, stdout, _err) = run(&[
        "convert",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--export-ir",
        ir.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "HDRi export-ir should succeed:\n{stdout}");
    assert!(is_tiff(&ir), "IR plane TIFF must be written");
    assert_eq!(json(&stdout)["ir_exported"], ir.to_str().unwrap());

    // HDR: no IR plane, so --export-ir fails loudly with exit 4 (Unsupported),
    // before writing the main output.
    let out_hdr = tmp.path("out-hdr.tiff");
    let ir_hdr = tmp.path("ir-hdr.tiff");
    let (code, _stdout, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out_hdr.to_str().unwrap(),
        "--export-ir",
        ir_hdr.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    assert_eq!(code, 4, "export-ir on an HDR scan is Unsupported (exit 4)");
    assert!(
        !out_hdr.exists(),
        "no output should be written on the fast-fail path"
    );
    assert!(err.to_lowercase().contains("ir"));
}

#[test]
fn export_film_rgb_is_convert_only_and_guarded_as_a_write_target() {
    let tmp = TempDir::new("film-rgb");
    let out = tmp.path("out.tiff");
    let input = fixture("hdr-48bit.tif");
    // Colliding with the primary would overwrite one artifact with the other.
    let (code, _stdout, err) = run(&[
        "convert",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--export-film-rgb",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--export-film-rgb"), "{err}");
    assert!(!out.exists());

    // `roll` has no such flag: every frame would overwrite the one path.
    let (code, _stdout, err) = run(&[
        "roll",
        input.to_str().unwrap(),
        "-o",
        tmp.path("roll").to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--export-film-rgb",
        tmp.path("film.tiff").to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--export-film-rgb"), "{err}");
}

#[test]
fn bad_params_are_usage_errors() {
    let tmp = TempDir::new("usage");
    let out = tmp.path("out.tiff");
    // An impossible knob value (a zero linearization slope) is rejected at the CLI
    // boundary (exit 2).
    let (code, _stdout, _err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--density-gamma",
        "0",
    ]);
    assert_eq!(code, 2, "invalid params must exit 2");
    assert!(!out.exists(), "no output on a usage error");

    // The removed simple clip controls are migration errors (exit 2), and the
    // removed --algorithm selector says to drop the flag.
    let (code, _stdout, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--clip-low",
        "0.9",
    ]);
    assert_eq!(code, 2, "a removed flag must exit 2: {err}");
    let (code, _stdout, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--algorithm",
        "density",
    ]);
    assert_eq!(code, 2, "--algorithm must be a migration error: {err}");
    assert!(
        err.contains("recipe `reconstruction`") && err.contains("Drop the flag"),
        "the migration error names what replaced it: {err}"
    );
    assert!(!out.exists(), "no output on a usage error");
}

/// A recipe carrying **only** a calibration renders exactly what the same values render
/// as flags, and a recipe carrying **no** calibration renders with the base from a flag.
///
/// The two halves of the split, asserted as bytes: a roll calibration and a pipeline
/// profile are separable files, which is what `core/recipe-composition` layers.
#[test]
fn a_calibration_only_recipe_matches_the_same_values_given_as_flags() {
    let tmp = TempDir::new("calibration-split");
    let scan = fixture("hdr-48bit.tif");
    let common: [&str; 0] = [];

    // A calibration is "a recipe with nothing else".
    let calibration = write_file(
        &tmp.path("roll-cal.json"),
        r#"{"recipe_version":3,"calibration":{"film_base":{"explicit":[0.9,0.55,0.42]}}}"#,
    );
    let from_recipe = tmp.path("recipe.tif");
    let (code, _, err) = {
        let mut a = vec!["convert", scan.to_str().unwrap(), "-o"];
        a.push(from_recipe.to_str().unwrap());
        a.extend_from_slice(&common);
        a.extend_from_slice(&["--params", calibration.to_str().unwrap()]);
        run(&a)
    };
    assert_eq!(code, 0, "a calibration-only recipe must convert: {err}");

    let from_flags = tmp.path("flags.tif");
    let (code, _, err) = {
        let mut a = vec!["convert", scan.to_str().unwrap(), "-o"];
        a.push(from_flags.to_str().unwrap());
        a.extend_from_slice(&common);
        a.extend_from_slice(&["--film-base", "0.9,0.55,0.42"]);
        run(&a)
    };
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        std::fs::read(&from_recipe).unwrap(),
        std::fs::read(&from_flags).unwrap(),
        "a calibration file and the same values as flags must render identically"
    );

    // A profile is "a recipe with no `calibration` section" — it needs a base from
    // somewhere, which is exactly why `calibration.film_base` has no default.
    let profile = write_file(
        &tmp.path("look.json"),
        r#"{"recipe_version":3,
            "reconstruction":{"scale":[1.0,0.84,0.73],"linearization":1.8,
                              "anchor":{"mid-at-base-offset":0.62}},
            "look":{"contrast":1.2}}"#,
    );
    let (code, _, err) = run(&[
        "convert",
        scan.to_str().unwrap(),
        "-o",
        tmp.path("profile.tif").to_str().unwrap(),
        "--params",
        profile.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    assert_eq!(code, 0, "a profile plus a base flag must convert: {err}");

    // **…and under `--strict`.** A profile has no `calibration` section by definition,
    // and that absence must raise nothing a `--strict` run would promote. (The roll
    // measurement is the roll's, not the profile's, so it comes from flags here.)
    let (code, _, err) = run(&[
        &[
            "convert",
            scan.to_str().unwrap(),
            "-o",
            tmp.path("profile-strict.tif").to_str().unwrap(),
            "--params",
            profile.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
            "--strict",
        ][..],
        &MEASURED,
    ]
    .concat());
    assert_eq!(code, 0, "a profile must be --strict clean: {err}");

    // …and without the base it is refused, not guessed.
    let (code, _, err) = run(&[
        "convert",
        scan.to_str().unwrap(),
        "-o",
        tmp.path("unstated.tif").to_str().unwrap(),
        "--params",
        profile.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "an unstated base must still be refused: {err}");
    assert!(err.contains("no film base selected"), "{err}");
}

#[test]
fn convert_is_deterministic() {
    // The project's defining contract: same inputs + params ⇒ byte-identical
    // output. Convert the same fixture twice and compare the TIFFs — on the film
    // master (f32, no colour transform) and on the default SDR destination, whose
    // render runs the lcms2 transform across rayon bands, the parallel path a
    // nondeterminism would most likely hide in.
    let tmp = TempDir::new("determinism");
    for (preset, destination) in [("film-master", Some("--film-master")), ("sdr", None)] {
        let args = |out: &Path| {
            let mut v = vec![
                "convert".to_string(),
                fixture("hdri-64bit.tif").to_str().unwrap().to_string(),
                "-o".to_string(),
                out.to_str().unwrap().to_string(),
            ];
            v.extend(destination.map(str::to_string));
            v.extend([
                "--film-base".to_string(),
                "0.9,0.55,0.42".to_string(),
                "--report".to_string(),
                "none".to_string(),
            ]);
            v
        };
        let a = tmp.path(&format!("{preset}-a.tiff"));
        let b = tmp.path(&format!("{preset}-b.tiff"));
        let (ca, _, _) = run(&args(&a).iter().map(String::as_str).collect::<Vec<_>>());
        let (cb, _, _) = run(&args(&b).iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!((ca, cb), (0, 0), "{preset}");
        assert_eq!(
            std::fs::read(&a).unwrap(),
            std::fs::read(&b).unwrap(),
            "{preset}: output TIFF must be byte-identical across runs"
        );
    }
}

#[test]
fn unreadable_input_is_decode_error_exit_three() {
    let tmp = TempDir::new("decode");
    let bad = tmp.path("not-a.tiff");
    std::fs::write(&bad, b"this is not a TIFF file").unwrap();
    let (code, _stdout, _err) = run(&["inspect", bad.to_str().unwrap()]);
    assert_eq!(code, 3, "a non-TIFF input is a decode error (exit 3)");
}

#[test]
fn unwritable_output_is_write_error_exit_five() {
    // Output into a nonexistent directory: encode's File::create fails → exit 5.
    let tmp = TempDir::new("write");
    let out = tmp.path("no-such-dir/out.tiff");
    let (code, _stdout, _err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    assert_eq!(
        code, 5,
        "an unwritable output path is a write error (exit 5)"
    );
}

#[test]
fn verbose_keeps_stdout_clean_json_and_logs_to_stderr() {
    // -v adds progress lines; they must go to stderr only — stdout stays pure
    // JSON (the agent contract). --report-file redirects the report off stdout.
    let tmp = TempDir::new("verbose");
    let out = tmp.path("out.tiff");
    let (code, stdout, stderr) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "-v",
    ]);
    assert_eq!(code, 0);
    // stdout is still a single clean JSON object.
    let _ = json(&stdout);
    // The progress line landed on stderr, not stdout.
    assert!(
        stderr.contains("decoded"),
        "progress log should be on stderr: {stderr}"
    );
    // Check the actual stderr log marker (`hanten: decoded …`), not a bare "decoded"
    // substring — the JSON report legitimately carries a `transfer_decoded` field.
    assert!(
        !stdout.contains("hanten: decoded"),
        "stdout must not carry log lines"
    );
}

#[test]
fn report_file_writes_json_off_stdout() {
    let tmp = TempDir::new("report-file");
    let out = tmp.path("out.tiff");
    let report = tmp.path("report.json");
    let (code, stdout, _err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--report-file",
        report.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert!(
        stdout.trim().is_empty(),
        "--report-file must keep stdout empty"
    );
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    assert_eq!(written["command"], "convert");
}

// --- write-target collision guards (PR review: never clobber data, exit 0) ----

#[test]
fn convert_rejects_in_place_output() {
    let fix = fixture("hdr-48bit.tif");
    let before = std::fs::read(&fix).unwrap();
    let (code, _, err) = run(&[
        "convert",
        fix.to_str().unwrap(),
        "-o",
        fix.to_str().unwrap(),
        // A base must be stated since `film_base.source` has no default; the
        // rule under test is the in-place-output guard, not that one.
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    assert_eq!(code, 2, "in-place output must be a usage error: {err}");
    assert!(err.contains("overwrite the input"), "stderr: {err}");
    assert_eq!(
        std::fs::read(&fix).unwrap(),
        before,
        "input scan must be untouched"
    );
}

#[test]
fn convert_rejects_report_file_colliding_with_artifacts() {
    let dir = TempDir::new("collide");
    let out = dir.path("out.tiff");
    let fix = fixture("hdr-48bit.tif");
    // --report-file == the output TIFF.
    let (code, _, err) = run(&[
        "convert",
        fix.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        // A base must be stated (no default); without it all three of these
        // conversions exit 2 on the missing-base gate and never reach the
        // collision check they exist to pin.
        "--film-base",
        "0.9,0.6,0.5",
        "--report-file",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "report over output must be a usage error: {err}");
    // Exit 2 alone cannot say *which* rule fired — that is how this test came to
    // pass on the missing-base gate instead. Pin the reason.
    assert!(
        !err.contains("no film base selected"),
        "must reach the collision check, not the film-base gate: {err}"
    );
    // --report-file reaching the output through a `..` traversal (the target
    // doesn't exist yet, so canonicalizing the full path alone can't catch it).
    std::fs::create_dir_all(dir.path("sub")).unwrap();
    let dotted = dir.path("sub/../out.tiff");
    let (code, _, err) = run(&[
        "convert",
        fix.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        // A base must be stated (no default); without it all three of these
        // conversions exit 2 on the missing-base gate and never reach the
        // collision check they exist to pin.
        "--film-base",
        "0.9,0.6,0.5",
        "--report-file",
        dotted.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "dotted report over output must be rejected: {err}");
    assert!(
        !out.exists(),
        "no artifact may be written on a rejected run"
    );
}

#[test]
fn inspect_rejects_report_file_over_input() {
    let fix = fixture("hdri-64bit.tif");
    let before = std::fs::read(&fix).unwrap();
    let (code, _, err) = run(&[
        "inspect",
        fix.to_str().unwrap(),
        "--report-file",
        fix.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "report over input must be a usage error: {err}");
    assert_eq!(
        std::fs::read(&fix).unwrap(),
        before,
        "input scan must be untouched"
    );
}

#[test]
fn convert_rejects_unapplied_input_profile() {
    // `--input-profile` is reserved for the deferred scanner-profile-before-density
    // experiment — it must fail loudly (exit 4), not silently ignore the profile.
    let dir = TempDir::new("inprofile");
    let out = dir.path("out.tiff");
    let fix = fixture("hdr-48bit.tif");
    let (code, _, err) = run(&[
        "convert",
        fix.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--input-profile",
        "scanner.icc",
    ]);
    assert_eq!(code, 4, "unapplied input profile must exit 4: {err}");
    assert!(err.contains("not supported"), "stderr: {err}");
    assert!(!out.exists());
}

#[test]
fn convert_reports_resolved_input_color_for_real_scan() {
    // A real SilverFast HDR scan resolves independently to a linear transfer and
    // scanner-device meaning, then reaches the render — reported with evidence.
    let tmp = TempDir::new("inputcolor");
    let out = tmp.path("out.tiff");
    let (code, stdout, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    assert_eq!(code, 0, "convert should succeed: {err}");
    let ic = &json(&stdout)["input_color"];
    assert_eq!(ic["transfer"], "linear");
    assert_eq!(ic["meaning"], "scanner-device");
    assert_eq!(ic["transfer_decoded"], false);
    assert_eq!(ic["icc_embedded"], false);
    // Both axes carry structural evidence.
    let ev = ic["evidence"].as_array().unwrap();
    assert!(
        ev.iter()
            .any(|e| e["axis"] == "transfer" && e["kind"] == "structural")
    );
    assert!(
        ev.iter()
            .any(|e| e["axis"] == "meaning" && e["kind"] == "structural")
    );
}

#[test]
fn inspect_reports_input_color_evidence() {
    let (code, stdout, err) = run(&["inspect", fixture("hdri-64bit.tif").to_str().unwrap()]);
    assert_eq!(code, 0, "inspect should succeed: {err}");
    let ic = &json(&stdout)["input_color"];
    assert_eq!(ic["transfer"], "linear");
    assert_eq!(ic["meaning"], "scanner-device");
    assert!(ic["evidence"].as_array().is_some_and(|e| !e.is_empty()));
}

#[test]
fn convert_rejects_colorimetric_assertion_on_scanner_scan() {
    // An explicit meaning that contradicts the raw-mode scanner structure fails
    // loudly (usage error, exit 2) — it never overrides container structure.
    let tmp = TempDir::new("colorimetric");
    let out = tmp.path("out.tiff");
    let (code, _stdout, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--input-meaning",
        "colorimetric",
    ]);
    assert_eq!(
        code, 2,
        "colorimetric-vs-structure must be a usage error: {err}"
    );
    assert!(err.contains("contradicts"), "stderr: {err}");
    assert!(!out.exists());
}

#[test]
fn convert_rejects_the_retired_input_color_recipe_key() {
    // A recipe carrying the removed combined `input.color` key fails to load with
    // a pinned migration message — it never silently asserts both axes.
    let tmp = TempDir::new("retired-input-color");
    let out = tmp.path("out.tiff");
    let recipe = tmp.path("recipe.json");
    std::fs::write(
        &recipe,
        r#"{"recipe_version":3,"input":{"color":"linear"}}"#,
    )
    .unwrap();
    let (code, _stdout, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(
        code, 2,
        "the retired input.color must be a usage error: {err}"
    );
    assert!(err.contains("input.transfer"), "stderr: {err}");
    assert!(!out.exists());
}

#[test]
fn generic_rgb16_without_silverfast_provenance_is_rejected() {
    // A plain RGB16 TIFF with no SilverFast Software tag and no IR plane carries
    // no raw-mode provenance — meaning resolves Unknown, so `convert` rejects it
    // (exit 4, not a silently-wrong negative) and `inspect` reports the ambiguity.
    let tmp = TempDir::new("generic");
    let src = tmp.path("generic.tif");
    write_uniform_rgb48(&src, [30000, 20000, 15000], 8, 8);
    let out = tmp.path("out.tiff");

    let (code, _stdout, err) = run(&[
        "convert",
        src.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    assert_eq!(code, 4, "generic RGB16 must be Unsupported (exit 4): {err}");
    assert!(
        err.contains("--input-transfer linear --input-meaning scanner-device"),
        "error must suggest the explicit-assertion escape hatch: {err}"
    );
    assert!(!out.exists());

    // inspect stays diagnostic — reports meaning unknown with evidence, no failure.
    let (code, stdout, _err) = run(&["inspect", src.to_str().unwrap()]);
    assert_eq!(code, 0, "inspect never fails on ambiguity");
    let ic = &json(&stdout)["input_color"];
    assert_eq!(ic["meaning"], "unknown");
    assert!(ic["evidence"].as_array().is_some_and(|e| !e.is_empty()));
}

#[test]
fn explicit_assertion_escape_hatch_converts_generic_rgb16() {
    // The user can take responsibility for a raw scan lacking provenance by
    // asserting both axes explicitly — that reaches the render (exit 0), and the
    // report records the assertions' provenance.
    let tmp = TempDir::new("escape");
    let src = tmp.path("generic.tif");
    write_uniform_rgb48(&src, [30000, 20000, 15000], 8, 8);
    let out = tmp.path("out.tiff");

    let (code, stdout, err) = run(&[
        "convert",
        src.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--input-transfer",
        "linear",
        "--input-meaning",
        "scanner-device",
    ]);
    assert_eq!(
        code, 0,
        "explicit assertions must convert a generic RGB16: {err}"
    );
    assert!(is_tiff(&out));
    let ic = &json(&stdout)["input_color"];
    assert_eq!(ic["transfer"], "linear");
    assert_eq!(ic["meaning"], "scanner-device");
    // Both axes carry a user-assertion evidence record with CLI provenance.
    let ev = ic["evidence"].as_array().unwrap();
    assert!(ev.iter().any(|e| {
        e["kind"] == "user-assertion"
            && e["provenance"]
                .as_str()
                .is_some_and(|p| p.contains("CLI flag"))
    }));
}

#[test]
fn input_assertion_provenance_distinguishes_cli_from_recipe() {
    // M2: the CLI-vs-recipe provenance is observable end-to-end. A recipe-sourced
    // assertion reports `input.… (recipe)`; a CLI-flag assertion reports
    // `--input-… (CLI flag)`.
    let tmp = TempDir::new("prov");
    let src = tmp.path("generic.tif");
    write_uniform_rgb48(&src, [30000, 20000, 15000], 8, 8);
    let recipe = tmp.path("recipe.json");
    std::fs::write(
        &recipe,
        r#"{"recipe_version":3,"input":{"transfer":"linear","meaning":"scanner-device"},
            "calibration":{"film_base":{"explicit":[0.9,0.55,0.42]}}}"#,
    )
    .unwrap();

    // Recipe-only: both assertions attributed to the recipe.
    let out1 = tmp.path("out1.tiff");
    let (code, stdout, err) = run(&[
        "convert",
        src.to_str().unwrap(),
        "-o",
        out1.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "recipe assertions convert: {err}");
    let ev = json(&stdout)["input_color"]["evidence"].clone();
    let ev = ev.as_array().unwrap();
    assert!(ev.iter().any(|e| {
        e["kind"] == "user-assertion"
            && e["provenance"]
                .as_str()
                .is_some_and(|p| p.contains("(recipe)"))
    }));

    // CLI flag over the recipe: the transfer assertion now reports CLI provenance.
    let out2 = tmp.path("out2.tiff");
    let (code, stdout, err) = run(&[
        "convert",
        src.to_str().unwrap(),
        "-o",
        out2.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
        "--input-transfer",
        "linear",
    ]);
    assert_eq!(code, 0, "cli override converts: {err}");
    let ev = json(&stdout)["input_color"]["evidence"].clone();
    let ev = ev.as_array().unwrap();
    assert!(ev.iter().any(|e| {
        e["axis"] == "transfer"
            && e["kind"] == "user-assertion"
            && e["provenance"]
                .as_str()
                .is_some_and(|p| p.contains("CLI flag"))
    }));
}

#[test]
fn assume_linear_flag_is_a_migration_error_through_the_binary() {
    // M3: the deprecated combined flag must fail loudly (exit 2) with migration
    // guidance — it must never silently assert both axes.
    let tmp = TempDir::new("assumelinear");
    let out = tmp.path("out.tiff");
    let (code, _stdout, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--assume-linear",
    ]);
    assert_eq!(code, 2, "--assume-linear must be a usage error: {err}");
    assert!(err.contains("--input-transfer"), "stderr: {err}");
    assert!(!out.exists());
}

#[test]
fn ir_plane_bit_identical_across_input_resolution() {
    // H1: IR is measurement data, never color-transformed — so the exported IR
    // plane must be byte-identical regardless of how the input color resolves
    // (auto vs an explicit scanner-device assertion take different resolver paths).
    let tmp = TempDir::new("ir-identity");
    let src = fixture("hdri-64bit.tif");
    let src = src.to_str().unwrap();

    let out_auto = tmp.path("out-auto.tiff");
    let ir_auto = tmp.path("ir-auto.tiff");
    let (code, _o, err) = run(&[
        "convert",
        src,
        "-o",
        out_auto.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--export-ir",
        ir_auto.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "auto convert: {err}");

    let out_expl = tmp.path("out-expl.tiff");
    let ir_expl = tmp.path("ir-expl.tiff");
    let (code, _o, err) = run(&[
        "convert",
        src,
        "-o",
        out_expl.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--export-ir",
        ir_expl.to_str().unwrap(),
        "--input-transfer",
        "linear",
        "--input-meaning",
        "scanner-device",
    ]);
    assert_eq!(code, 0, "explicit-assertion convert: {err}");

    let a = std::fs::read(&ir_auto).unwrap();
    let b = std::fs::read(&ir_expl).unwrap();
    assert_eq!(
        a, b,
        "exported IR must be byte-identical across input resolution"
    );
}

#[test]
fn roll_frame_report_includes_resolved_input_color() {
    // P2: a roll frame report must carry the resolved input semantics (mirrors
    // single-frame `convert`), not drop them.
    let tmp = TempDir::new("roll-ic");
    let out_dir = tmp.path("out");
    let recipe = tmp.path("recipe.json");
    std::fs::write(
        &recipe,
        r#"{"recipe_version":3,"calibration":{"film_base":{"explicit":[0.9,0.55,0.42]}}}"#,
    )
    .unwrap();
    let (code, stdout, err) = run(&[
        "roll",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "roll should succeed: {err}");
    let frame = &json(&stdout)["frames"][0];
    assert_eq!(frame["input_color"]["transfer"], "linear");
    assert_eq!(frame["input_color"]["meaning"], "scanner-device");
}

#[test]
fn roll_frame_report_makes_the_measurement_area_observable() {
    // `measure.inset` reaches the measurement region from the shared recipe *and*
    // from a per-frame override, so a roll frame has to report the resolved area —
    // otherwise the knob is accepted and its effect invisible, which is exactly the
    // defect reporting it unconditionally on `convert` exists to prevent.
    let tmp = TempDir::new("roll-area");
    let out_dir = tmp.path("out");
    let recipe = tmp.path("recipe.json");
    std::fs::write(
        &recipe,
        r#"{"recipe_version":3,"calibration":{"film_base":{"explicit":[0.9,0.55,0.42]}}}"#,
    )
    .unwrap();
    let frames = tmp.path("frames.json");
    let src = fixture("hdr-48bit.tif");
    std::fs::write(
        &frames,
        format!(
            r#"{{"frames":[{{"input":{:?},"params":{{"measure":{{"inset":0.12}}}}}}]}}"#,
            src.to_str().unwrap()
        ),
    )
    .unwrap();
    let (code, stdout, err) = run(&[
        "roll",
        "--frames",
        frames.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "roll should succeed: {err}");
    let report = json(&stdout);
    // The shared recipe keeps the default 5 % (23 px on this frame); the frame
    // resolved the override's 12 %.
    let area = &report["frames"][0]["effective_area"];
    assert_eq!(
        area["inset"], 55,
        "the per-frame inset must reach the region and be reported: {report}"
    );
    assert_eq!(
        area["region"][0], 55,
        "the reported rectangle must follow the resolved inset: {report}"
    );
}

#[test]
fn roll_rejects_colorimetric_shared_recipe_before_decode() {
    // M1: an unconditionally-unsupported shared assertion fails fast, before the
    // first (large) scan is decoded — exit 4 with an actionable message.
    let tmp = TempDir::new("roll-colorimetric");
    let out_dir = tmp.path("out");
    let recipe = tmp.path("recipe.json");
    // The shared recipe states a base (no default) so the rejection under test
    // is the colorimetric one, not the missing-base usage error.
    std::fs::write(
        &recipe,
        r#"{"recipe_version":3,"input":{"meaning":"colorimetric"},"calibration":{"film_base":{"explicit":[0.9,0.6,0.5]}}}"#,
    )
    .unwrap();
    let (code, _stdout, err) = run(&[
        "roll",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(
        code, 4,
        "colorimetric shared recipe must be Unsupported: {err}"
    );
    assert!(err.contains("colorimetric"), "stderr: {err}");
    // Fail-fast: no output directory contents were produced.
    assert!(
        !out_dir.join("hdr-48bit_positive.tiff").exists(),
        "no frame should be written on the pre-flight reject"
    );
}

// --- XMP-based SilverFast provenance gate (adversarial-review hardening) ------

/// Attribute list for a genuine raw negative scan.
const XMP_NEG: &str = r#"Silverfast:Company="LaserSoft Imaging" Silverfast:HDRScan="Yes" Silverfast:Gamma="1" Silverfast:Negative="Yes""#;

#[test]
fn rgb16_plus_gray16_without_xmp_is_rejected() {
    // Adversarial hole #1: a generic RGB16 + matching Gray16 multipage (an IR-like
    // second page) must NOT be treated as a raw scanner scan without XMP.
    let tmp = TempDir::new("ir-forge");
    let src = tmp.path("forged.tif");
    write_rgb16(&src, None, None, true); // IR page, no XMP
    let out = tmp.path("out.tiff");
    let (code, _stdout, err) = run(&[
        "convert",
        src.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    assert_eq!(code, 4, "RGB16+Gray16 without XMP must be rejected: {err}");
    assert!(!out.exists());
}

#[test]
fn software_silverfast_string_without_xmp_is_rejected() {
    // Adversarial hole #2: a `Software="SilverFast …"` string (which a processed
    // export keeps) is NOT sufficient provenance without the XMP mode metadata.
    let tmp = TempDir::new("sw-forge");
    let src = tmp.path("sw.tif");
    write_rgb16(&src, None, Some("SilverFast 9.2.8 (Jun 11 2026)"), false);
    let out = tmp.path("out.tiff");
    let (code, _stdout, err) = run(&[
        "convert",
        src.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    assert_eq!(
        code, 4,
        "Software string without XMP must be rejected: {err}"
    );
    assert!(!out.exists());
}

#[test]
fn silverfast_xmp_negative_converts() {
    // A genuine raw negative (XMP Company+HDRScan=Yes+Gamma=1+Negative=Yes) reaches
    // the render and reports scanner-device / linear.
    let tmp = TempDir::new("xmp-neg");
    let src = tmp.path("neg.tif");
    write_rgb16(&src, Some(&silverfast_xmp(XMP_NEG)), None, false);
    let out = tmp.path("out.tiff");
    let (code, stdout, err) = run(&[
        "convert",
        src.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    assert_eq!(code, 0, "synthetic SilverFast negative must convert: {err}");
    assert!(is_tiff(&out));
    let ic = &json(&stdout)["input_color"];
    assert_eq!(ic["transfer"], "linear");
    assert_eq!(ic["meaning"], "scanner-device");
}

#[test]
fn silverfast_xmp_nonlinear_gamma_is_rejected() {
    // Contradiction path is LIVE: a raw-mode scan (HDRScan=Yes) whose XMP Gamma is
    // non-linear (a processed export) → ambiguous transfer → convert exits 4;
    // inspect stays diagnostic and reports transfer unknown.
    let tmp = TempDir::new("xmp-gamma");
    let src = tmp.path("g.tif");
    let attrs = r#"Silverfast:Company="LaserSoft Imaging" Silverfast:HDRScan="Yes" Silverfast:Gamma="2.2" Silverfast:Negative="Yes""#;
    write_rgb16(&src, Some(&silverfast_xmp(attrs)), None, false);
    let out = tmp.path("out.tiff");
    let (code, _stdout, err) = run(&[
        "convert",
        src.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    assert_eq!(
        code, 4,
        "non-linear gamma on raw mode must be rejected: {err}"
    );
    assert!(!out.exists());

    let (code, stdout, _err) = run(&["inspect", src.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(json(&stdout)["input_color"]["transfer"], "unknown");
}

#[test]
fn silverfast_positive_mode_is_rejected() {
    // A positive-mode scan (XMP Negative=No) passes the transfer/meaning gate but
    // must be rejected loudly with the distinct positive-mode message rather than
    // silently converted as a negative.
    let tmp = TempDir::new("xmp-pos");
    let src = tmp.path("pos.tif");
    let attrs = r#"Silverfast:Company="LaserSoft Imaging" Silverfast:HDRScan="Yes" Silverfast:Gamma="1" Silverfast:Negative="No""#;
    write_rgb16(&src, Some(&silverfast_xmp(attrs)), None, false);
    let out = tmp.path("out.tiff");
    let (code, _stdout, err) = run(&[
        "convert",
        src.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    assert_eq!(code, 4, "positive-mode scan must be rejected: {err}");
    assert!(err.contains("positive-mode"), "stderr: {err}");
    assert!(!out.exists());
}

#[test]
fn silverfast_malformed_gamma_is_ambiguous_and_rejected() {
    // F1 end-to-end: a raw-mode scan whose XMP Gamma is locale-formatted ("2,2")
    // must NOT silently resolve to linear — decode warns, transfer resolves
    // Unknown, and convert exits 4 (rather than converting a possibly-non-linear
    // scan as linear).
    let tmp = TempDir::new("xmp-badgamma");
    let src = tmp.path("g.tif");
    let attrs = r#"Silverfast:Company="LaserSoft Imaging" Silverfast:HDRScan="Yes" Silverfast:Gamma="2,2" Silverfast:Negative="Yes""#;
    write_rgb16(&src, Some(&silverfast_xmp(attrs)), None, false);
    let out = tmp.path("out.tiff");
    let (code, _stdout, err) = run(&[
        "convert",
        src.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    assert_eq!(
        code, 4,
        "malformed gamma must be rejected, not silently linear: {err}"
    );
    assert!(!out.exists());

    // inspect stays diagnostic: transfer unknown + a breadcrumb naming the value.
    let (code, stdout, _err) = run(&["inspect", src.to_str().unwrap()]);
    assert_eq!(code, 0);
    let report = json(&stdout);
    assert_eq!(report["input_color"]["transfer"], "unknown");
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("2,2")),
        "inspect report must carry the malformed-gamma breadcrumb: {stdout}"
    );
}

#[test]
fn silverfast_unrecognized_negative_value_still_converts_a_negative() {
    // F3 end-to-end: a genuine negative whose `Negative` reads as an unrecognized
    // token (not "yes"/"no") must NOT be misread as positive-mode and rejected —
    // an unrecognized value is `None`, not an explicit "No", so it still converts.
    let tmp = TempDir::new("xmp-weirdneg");
    let src = tmp.path("n.tif");
    let attrs = r#"Silverfast:Company="LaserSoft Imaging" Silverfast:HDRScan="Yes" Silverfast:Gamma="1" Silverfast:Negative="y""#;
    write_rgb16(&src, Some(&silverfast_xmp(attrs)), None, false);
    let out = tmp.path("out.tiff");
    let (code, _stdout, err) = run(&[
        "convert",
        src.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    assert_eq!(
        code, 0,
        "an unrecognized Negative value must not trigger positive-mode rejection: {err}"
    );
    assert!(is_tiff(&out));
}

// --- telemetry (opt-in performance + context record) -------------------------

#[test]
fn telemetry_file_writes_full_record() {
    // `--telemetry-file <path>` writes one valid JSON event with every schema
    // field populated (finite timings, correct dims/bytes).
    let tmp = TempDir::new("tel-file");
    let out = tmp.path("out.tiff");
    let rec = tmp.path("run.json");
    let (code, _stdout, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--telemetry-file",
        rec.to_str().unwrap(),
    ]);
    assert_eq!(
        code, 0,
        "convert with --telemetry-file should succeed:\n{err}"
    );

    let record: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&rec).unwrap()).unwrap();

    assert_eq!(record["schema_version"], 11);
    assert!(record["timestamp_ms"].as_u64().unwrap() > 0);
    let id = record["event_id"].as_str().unwrap();
    let hex = |b: u8| b.is_ascii_hexdigit() && !b.is_ascii_uppercase();
    assert!(id.len() == 32 && id.bytes().all(hex), "event_id: {id}");
    assert_eq!(record["event"], "conversion");
    assert_eq!(record["command"], "convert");
    assert_eq!(record["stage"], "finalize");
    assert!(record["nc_version"].is_string());
    assert!(record["target"].is_string());
    assert!(record["cpu_count"].is_number() || record["cpu_count"].is_null());

    // Image facts match the known HDR fixture (502x462, 3ch, 16-bit, no IR).
    let image = &record["image"];
    assert_eq!(image["format"], "hdr");
    assert_eq!(image["width"], 502);
    assert_eq!(image["height"], 462);
    assert_eq!(image["channels"], 3);
    assert_eq!(image["bit_depth"], 16);
    assert_eq!(image["ir_present"], false);
    let mp = image["megapixels"].as_f64().unwrap();
    assert!(
        (mp - (502.0 * 462.0 / 1_000_000.0)).abs() < 1e-9,
        "megapixels: {mp}"
    );
    assert!(image["input_bytes"].as_u64().unwrap() > 0);
    assert!(image["output_bytes"].as_u64().unwrap() > 0);

    // Every stage a rendered destination runs is timed, by name (schema 9).
    let timing = &record["timing_ms"];
    for key in [
        "total",
        "decode",
        "film_base",
        "reconstruction",
        "scene_correction",
        "look",
        "fit_range",
        "fit_gamut",
        "destination",
        "encode",
    ] {
        assert!(
            timing[key].as_f64().is_some_and(f64::is_finite),
            "timing_ms.{key} must be finite: {timing}"
        );
    }
    for gone in ["algorithm", "color"] {
        assert!(timing.get(gone).is_none(), "{timing}");
    }
    // No IR plane in this fixture → no ir_export timing.
    assert!(timing.get("ir_export").is_none() || timing["ir_export"].is_null());

    let conv = &record["conversion"];
    // Schema 8 names the destination, every axis resolved, where the preset was.
    assert_eq!(conv["destination"]["display"]["gamut"], "display-p3");
    assert!(conv.get("preset").is_none(), "{conv}");
    // Schema 5 dropped the one-valued reconstruction type, and schema 7 the one-valued
    // curve.
    assert!(conv.get("reconstruction").is_none(), "{conv}");
    assert!(conv.get("curve").is_none(), "{conv}");
    assert!(conv["params_hash"].as_str().unwrap().len() == 16);
    assert_eq!(
        conv["film_base_source"]["explicit"],
        serde_json::json!([0.9, 0.55, 0.42])
    );
    assert_eq!(conv["output_depth"], "u16");

    let outcome = &record["outcome"];
    assert_eq!(outcome["status"], "success");
    assert_eq!(outcome["error_kind"], "none");
    assert_eq!(outcome["exit_code"], 0);
    assert!(outcome["warnings"].is_number());
    assert!(outcome["total_samples"].as_u64().unwrap() > 0);
    assert!(outcome["clipped"].is_number());
    assert!(outcome["non_finite"].is_number());
}

#[test]
fn strict_failure_writes_a_strict_failure_event() {
    // A `--strict` run that exits non-zero on a warning is a failure of its own
    // kind, `strict`, not a successful conversion. The output was written, so the
    // event carries the whole run. Force a clipping warning with a large
    // `--exposure` (as in `u16_clipping_is_reported_and_strict_promotes_it`).
    let tmp = TempDir::new("tel-strict");
    let (code, err, event) = convert_with_event(
        &tmp,
        &fixture("hdr-48bit.tif"),
        &tmp.path("out.tiff"),
        &[
            "--film-base",
            "0.9,0.55,0.42",
            "--exposure",
            "12",
            "--strict",
        ],
    );
    assert_eq!(code, 1, "--strict clipping run must exit 1: {err}");
    assert_failure(&event, "finalize", "strict", 1);
    assert!(event["image"]["output_bytes"].as_u64().unwrap() > 0);
    assert!(event["outcome"]["clipped"].as_u64().unwrap() > 0);
    assert!(event["timing_ms"]["encode"].is_number(), "{event}");
}

/// The one event a `--telemetry-file` run wrote.
fn telemetry_event(path: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("no telemetry event at {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap()
}

/// A failure event names where the run ended, its category and its exit code.
fn assert_failure(event: &serde_json::Value, stage: &str, kind: &str, exit: i64) {
    assert_eq!(event["schema_version"], 11, "{event}");
    assert_eq!(event["stage"], stage, "{event}");
    let outcome = &event["outcome"];
    assert_eq!(outcome["status"], "failure", "{event}");
    assert_eq!(outcome["error_kind"], kind, "{event}");
    assert_eq!(outcome["exit_code"], exit, "{event}");
}

/// A convert of `input` with `--telemetry-file`, plus `extra` flags; the exit code,
/// stderr and the event it wrote.
fn convert_with_event(
    tmp: &TempDir,
    input: &Path,
    out: &Path,
    extra: &[&str],
) -> (i32, String, serde_json::Value) {
    let rec = tmp.path("event.json");
    let mut args = vec![
        "convert",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--telemetry-file",
        rec.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    let (code, _stdout, err) = run(&args);
    (code, err, telemetry_event(&rec))
}

#[test]
fn a_usage_failure_before_decode_writes_a_setup_event_with_nothing_invented() {
    // No film base: refused by validation, before anything resolved or decoded.
    let tmp = TempDir::new("tel-usage");
    let (code, err, event) =
        convert_with_event(&tmp, &fixture("hdr-48bit.tif"), &tmp.path("out.tiff"), &[]);
    assert_eq!(code, 2, "{err}");
    assert_failure(&event, "setup", "usage", 2);
    for absent in ["image", "conversion"] {
        assert!(event.get(absent).is_none(), "{absent}: {event}");
    }
    let timing = event["timing_ms"].as_object().unwrap();
    assert_eq!(timing.keys().collect::<Vec<_>>(), ["total"], "{event}");
    for absent in ["total_samples", "clipped", "non_finite"] {
        assert!(event["outcome"].get(absent).is_none(), "{event}");
    }
}

#[test]
fn a_decode_failure_times_nothing_it_did_not_finish() {
    let tmp = TempDir::new("tel-decode");
    let bad = tmp.path("not-a.tiff");
    std::fs::write(&bad, b"this is not a TIFF file").unwrap();
    let (code, err, event) = convert_with_event(
        &tmp,
        &bad,
        &tmp.path("out.tiff"),
        &["--film-base", "0.9,0.55,0.42"],
    );
    assert_eq!(code, 3, "{err}");
    // The memory preflight probes the header before decode, so an unreadable file
    // fails there; either way nothing after it ran.
    assert_failure(&event, "preflight", "decode", 3);
    assert!(event.get("image").is_none(), "{event}");
    assert!(event["conversion"]["params_hash"].is_string(), "{event}");
    assert!(event["timing_ms"].get("decode").is_none(), "{event}");
}

#[test]
fn unsupported_input_names_the_stage_before_the_check_and_keeps_its_time() {
    // `--export-ir` on an HDR scan (no IR plane) is refused right after decode:
    // decode completed, so it is timed, and the refusal belongs to it.
    let tmp = TempDir::new("tel-unsupported");
    let ir = tmp.path("ir.tiff");
    let (code, err, event) = convert_with_event(
        &tmp,
        &fixture("hdr-48bit.tif"),
        &tmp.path("out.tiff"),
        &[
            "--film-base",
            "0.9,0.55,0.42",
            "--export-ir",
            ir.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 4, "{err}");
    assert_failure(&event, "decode", "unsupported", 4);
    assert_eq!(event["image"]["ir_present"], false, "{event}");
    assert!(event["image"]["output_bytes"].is_null(), "{event}");
    assert!(event["timing_ms"]["decode"].is_number(), "{event}");
    assert!(event["timing_ms"].get("film_base").is_none(), "{event}");
}

#[test]
fn a_write_failure_names_the_encode_stage_without_timing_it() {
    // Output into a nonexistent directory: the encoder cannot create it.
    let tmp = TempDir::new("tel-write");
    let (code, err, event) = convert_with_event(
        &tmp,
        &fixture("hdr-48bit.tif"),
        &tmp.path("no-such-dir/out.tiff"),
        &["--film-base", "0.9,0.55,0.42"],
    );
    assert_eq!(code, 5, "{err}");
    assert_failure(&event, "encode", "write", 5);
    let timing = &event["timing_ms"];
    assert!(timing["destination"].is_number(), "{event}");
    assert!(timing.get("encode").is_none(), "{event}");
    assert!(event["image"]["output_bytes"].is_null(), "{event}");
    assert!(event["outcome"].get("clipped").is_none(), "{event}");
}

#[test]
fn a_scan_truncated_after_its_header_fails_in_the_decode_stage() {
    // The header probe passes; the pixel read inside the decode stage fails, so the
    // stage is named and not timed.
    let tmp = TempDir::new("tel-decode-stage");
    let truncated = tmp.path("truncated.tif");
    let bytes = std::fs::read(fixture("hdr-48bit.tif")).unwrap();
    std::fs::write(&truncated, &bytes[..100_000]).unwrap();
    let (code, err, event) = convert_with_event(
        &tmp,
        &truncated,
        &tmp.path("out.tiff"),
        &["--film-base", "0.9,0.55,0.42"],
    );
    assert_eq!(code, 3, "{err}");
    assert_failure(&event, "decode", "decode", 3);
    assert!(event.get("image").is_none(), "{event}");
    assert!(event["timing_ms"].get("decode").is_none(), "{event}");
}

#[test]
fn a_telemetry_file_over_the_params_recipe_is_refused_and_never_written() {
    // The recipe the run reads is off-limits to telemetry: a good run refuses the
    // pair up front, and a run whose recipe fails to load writes no event over it.
    let tmp = TempDir::new("tel-onto-params");
    let recipe = tmp.path("recipe.json");
    let dump = |recipe: &Path| {
        run(&[
            "convert",
            fixture("hdr-48bit.tif").to_str().unwrap(),
            "-o",
            tmp.path("out.tiff").to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
            "--params",
            recipe.to_str().unwrap(),
            "--telemetry-file",
            recipe.to_str().unwrap(),
        ])
    };
    std::fs::write(&recipe, r#"{"recipe_version": 3}"#).unwrap();
    let (code, _stdout, err) = dump(&recipe);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("would overwrite --params"), "{err}");

    let bad = br#"{"not": "a recipe"}"#;
    std::fs::write(&recipe, bad).unwrap();
    let (code, _stdout, err) = dump(&recipe);
    assert_eq!(code, 2, "{err}");
    assert_eq!(
        std::fs::read(&recipe).unwrap(),
        bad,
        "the recipe was overwritten"
    );
    assert!(err.contains("telemetry: no event written"), "{err}");
}

#[test]
fn a_failure_event_never_lands_on_a_flag_export_ir_path() {
    // Validation fails (no film base) before the recipe resolves `--export-ir`; the
    // flag's path is still off-limits.
    let tmp = TempDir::new("tel-onto-ir");
    let ir = tmp.path("ir.tiff");
    std::fs::write(&ir, b"an earlier IR export").unwrap();
    let (code, _stdout, err) = run(&[
        "convert",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "-o",
        tmp.path("out.tiff").to_str().unwrap(),
        "--export-ir",
        ir.to_str().unwrap(),
        "--telemetry-file",
        ir.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert_eq!(std::fs::read(&ir).unwrap(), b"an earlier IR export");
    assert!(err.contains("telemetry: no event written"), "{err}");
}

#[test]
fn a_failure_event_never_lands_on_a_recipe_export_ir_or_a_completed_output() {
    // Validation fails (no film base) before the write-target guard. The recipe's
    // `--export-ir` path and the path `-o out` would complete to are off-limits.
    let tmp = TempDir::new("tel-onto-recipe-ir");
    let input = fixture("hdri-64bit.tif");
    let refuses = |sink: &Path, args: &[&str]| {
        std::fs::write(sink, b"an earlier artifact").unwrap();
        let (code, _stdout, err) = run(args);
        assert_eq!(code, 2, "{err}");
        assert_eq!(
            std::fs::read(sink).unwrap(),
            b"an earlier artifact",
            "{sink:?}"
        );
        assert!(err.contains("telemetry: no event written"), "{err}");
    };

    let ir = tmp.path("ir.tiff");
    let recipe = tmp.path("recipe.json");
    let body = serde_json::json!({"recipe_version": 3, "input": {"export_ir": ir}});
    std::fs::write(&recipe, body.to_string()).unwrap();
    refuses(
        &ir,
        &[
            "convert",
            input.to_str().unwrap(),
            "-o",
            tmp.path("out2.tiff").to_str().unwrap(),
            "--params",
            recipe.to_str().unwrap(),
            "--telemetry-file",
            ir.to_str().unwrap(),
        ],
    );

    let completed = tmp.path("out.tiff");
    refuses(
        &completed,
        &[
            "convert",
            input.to_str().unwrap(),
            "-o",
            tmp.path("out").to_str().unwrap(),
            "--telemetry-file",
            completed.to_str().unwrap(),
        ],
    );
}

#[test]
fn a_failure_event_never_lands_on_the_input() {
    // Before the write-target guard runs, a failure event is written only when its
    // sink is clear of the input and the outputs. Here validation fails first (no
    // film base) and the sink is the input: the scan must survive untouched.
    let tmp = TempDir::new("tel-onto-input");
    let input = tmp.path("scan.tif");
    std::fs::copy(fixture("hdr-48bit.tif"), &input).unwrap();
    let before = std::fs::read(&input).unwrap();
    let (code, _stdout, err) = run(&[
        "convert",
        input.to_str().unwrap(),
        "-o",
        tmp.path("out.tiff").to_str().unwrap(),
        "--telemetry-file",
        input.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert_eq!(
        std::fs::read(&input).unwrap(),
        before,
        "the input was overwritten"
    );
    assert!(err.contains("telemetry: no event written"), "{err}");
}

#[test]
fn telemetry_file_records_ir_export_timing() {
    // An HDRi conversion with --export-ir carries the ir_export stage timing.
    let tmp = TempDir::new("tel-ir");
    let out = tmp.path("out.tiff");
    let ir = tmp.path("ir.tiff");
    let rec = tmp.path("run.json");
    let (code, _stdout, err) = run(&[
        "convert",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--export-ir",
        ir.to_str().unwrap(),
        "--telemetry-file",
        rec.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "HDRi export-ir + telemetry should succeed:\n{err}");
    let record: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&rec).unwrap()).unwrap();
    assert_eq!(record["image"]["ir_present"], true);
    assert!(
        record["timing_ms"]["ir_export"]
            .as_f64()
            .is_some_and(f64::is_finite),
        "ir_export timing must be present when --export-ir ran: {record}"
    );
}

#[test]
fn telemetry_log_appends_one_line_per_run() {
    // `--telemetry` appends exactly one JSONL line per run to NC_TELEMETRY_LOG.
    let tmp = TempDir::new("tel-log");
    let log = tmp.path("telemetry.jsonl");
    let convert = |out: &Path| {
        run_env(
            &[
                "convert",
                fixture("hdr-48bit.tif").to_str().unwrap(),
                "-o",
                out.to_str().unwrap(),
                "--film-base",
                "0.9,0.55,0.42",
                "--telemetry",
                "--report",
                "none",
            ],
            &[("NC_TELEMETRY_LOG", log.to_str().unwrap())],
        )
    };
    let out1 = tmp.path("a.tiff");
    let out2 = tmp.path("b.tiff");
    let (c1, _, e1) = convert(&out1);
    let (c2, _, e2) = convert(&out2);
    assert_eq!(
        (c1, c2),
        (0, 0),
        "telemetry runs should succeed:\n{e1}\n{e2}"
    );

    let contents = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = contents.lines().collect();
    assert_eq!(lines.len(), 2, "two runs must append two lines: {contents}");
    // Each line is an independent, valid JSON object.
    for line in lines {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["schema_version"], 11);
    }
}

#[test]
fn telemetry_both_sinks_receive_the_record() {
    // `--telemetry` + `--telemetry-file` together write to both the JSONL log and
    // the one-off file ("Both").
    let tmp = TempDir::new("tel-both");
    let out = tmp.path("out.tiff");
    let log = tmp.path("telemetry.jsonl");
    let rec = tmp.path("run.json");
    let (code, _stdout, err) = run_env(
        &[
            "convert",
            fixture("hdr-48bit.tif").to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
            "--telemetry",
            "--telemetry-file",
            rec.to_str().unwrap(),
            "--report",
            "none",
        ],
        &[("NC_TELEMETRY_LOG", log.to_str().unwrap())],
    );
    assert_eq!(code, 0, "both-sink telemetry should succeed:\n{err}");
    assert!(log.exists(), "JSONL log must be written");
    assert!(rec.exists(), "one-off file must be written");
    let log_line = std::fs::read_to_string(&log).unwrap();
    let file_line = std::fs::read_to_string(&rec).unwrap();
    // Same record content in both sinks (the one-off adds a trailing newline).
    assert_eq!(log_line.trim(), file_line.trim());
}

#[test]
fn telemetry_does_not_perturb_the_output() {
    // THE determinism invariant: telemetry on vs off must produce byte-identical
    // output TIFF — telemetry never touches the deterministic
    // path. Point NC_TELEMETRY_LOG at a temp file for the on-run so the default
    // log is never touched.
    let tmp = TempDir::new("tel-invariant");
    let log = tmp.path("telemetry.jsonl");
    let base = |out: &Path| {
        vec![
            "convert".to_string(),
            fixture("hdri-64bit.tif").to_str().unwrap().to_string(),
            "-o".to_string(),
            out.to_str().unwrap().to_string(),
            "--film-base".to_string(),
            "0.9,0.55,0.42".to_string(),
            "--report".to_string(),
            "none".to_string(),
        ]
    };

    // Telemetry OFF.
    let off = tmp.path("off.tiff");
    let (c_off, _, _) = run(&base(&off).iter().map(String::as_str).collect::<Vec<_>>());

    // Telemetry ON (both sinks).
    let on = tmp.path("on.tiff");
    let rec = tmp.path("on-run.json");
    let mut on_args = base(&on);
    on_args.extend(["--telemetry", "--telemetry-file", rec.to_str().unwrap()].map(String::from));
    let (c_on, _, _) = run_env(
        &on_args.iter().map(String::as_str).collect::<Vec<_>>(),
        &[("NC_TELEMETRY_LOG", log.to_str().unwrap())],
    );

    assert_eq!((c_off, c_on), (0, 0));
    assert_eq!(
        std::fs::read(&off).unwrap(),
        std::fs::read(&on).unwrap(),
        "output TIFF must be byte-identical with telemetry on vs off"
    );
    // The telemetry record itself was produced (sanity: the feature actually ran).
    assert!(rec.exists() && log.exists());
}

#[test]
fn a_telemetry_write_failure_keeps_a_failed_runs_exit_code() {
    // The run fails on its own (no film base, exit 2); the event's sink is under a
    // regular file, so writing it fails too. The exit code stays the run's.
    let tmp = TempDir::new("tel-failsoft-failure");
    let blocker = tmp.path("blocker");
    std::fs::write(&blocker, b"not a directory").unwrap();
    let (code, _stdout, stderr) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        tmp.path("out.tiff").to_str().unwrap(),
        "--telemetry-file",
        tmp.path("blocker/rec.json").to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("telemetry: could not write"), "{stderr}");
}

#[test]
fn telemetry_write_failure_is_fail_soft_even_under_strict() {
    // A telemetry write failure must NOT fail a successful conversion, and
    // --strict must not promote it (the image already succeeded). Force a write
    // failure by pointing --telemetry-file under a path whose parent is a regular
    // file (so create_dir_all fails). Use --film-master so the conversion itself
    // raises no warnings (f32 never clips; the HDR fixture has no IR plane), which
    // isolates the telemetry failure from any legitimate --strict trigger.
    let tmp = TempDir::new("tel-failsoft");
    let out = tmp.path("out.tiff");
    let blocker = tmp.path("blocker");
    std::fs::write(&blocker, b"not a directory").unwrap();
    let bad = tmp.path("blocker/rec.json"); // parent is a file → write fails

    let (code, _stdout, stderr) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-master",
        "--film-base",
        "0.9,0.55,0.42",
        "--telemetry-file",
        bad.to_str().unwrap(),
        "--strict",
    ]);
    assert_eq!(
        code, 0,
        "a telemetry write failure must not fail the run, even with --strict:\n{stderr}"
    );
    assert!(is_tiff(&out), "the output TIFF must still be written");
    assert!(
        stderr.to_lowercase().contains("telemetry"),
        "the telemetry failure must be warned on stderr: {stderr}"
    );
}

#[test]
fn telemetry_file_colliding_with_output_is_usage_error() {
    // A --telemetry-file that would clobber the output (a config error, distinct
    // from a runtime write failure) fails loudly up front, before decoding.
    let tmp = TempDir::new("tel-collide");
    let out = tmp.path("out.tiff");
    let (code, _stdout, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--telemetry-file",
        out.to_str().unwrap(),
    ]);
    assert_eq!(
        code, 2,
        "telemetry-file over the output must be a usage error: {err}"
    );
    assert!(
        !out.exists(),
        "no artifact may be written on a rejected run"
    );
}

#[test]
fn telemetry_log_colliding_with_output_is_usage_error() {
    // The persistent `--telemetry` log (here via NC_TELEMETRY_LOG) is guarded the
    // same way as --telemetry-file: a path that would append into the output is a
    // loud usage error up front, not a silent post-write corruption.
    let tmp = TempDir::new("tel-log-collide");
    let out = tmp.path("out.tiff");
    let (code, _stdout, err) = run_env(
        &[
            "convert",
            fixture("hdr-48bit.tif").to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
            "--telemetry",
        ],
        &[("NC_TELEMETRY_LOG", out.to_str().unwrap())],
    );
    assert_eq!(
        code, 2,
        "telemetry log over the output must be a usage error: {err}"
    );
    assert!(
        !out.exists(),
        "no artifact may be written on a rejected run"
    );
}

#[test]
fn telemetry_file_dash_writes_json_to_stdout() {
    // `-` = stdout. Paired with --report none so stdout is exactly the one
    // telemetry line (a single parseable JSON object), and it must NOT be rejected
    // as a collision.
    let tmp = TempDir::new("tel-stdout");
    let out = tmp.path("out.tiff");
    let (code, stdout, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--telemetry-file",
        "-",
        "--report",
        "none",
    ]);
    assert_eq!(code, 0, "telemetry to stdout should succeed:\n{err}");
    let record = json(&stdout);
    assert_eq!(record["schema_version"], 11);
    assert_eq!(record["image"]["format"], "hdr");
}

#[test]
fn telemetry_params_hash_matches_identical_conversions() {
    // The load-bearing dedup contract: identical params ⇒ identical params_hash; a
    // changed knob ⇒ a different hash.
    let tmp = TempDir::new("tel-hash");
    let fix = fixture("hdr-48bit.tif");
    let convert = |out: &Path, extra: &[&str]| -> serde_json::Value {
        let out = out.to_str().unwrap();
        let mut argv = vec![
            "convert",
            fix.to_str().unwrap(),
            "-o",
            out,
            "--film-base",
            "0.9,0.55,0.42",
            "--telemetry-file",
            "-",
            "--report",
            "none",
        ];
        argv.extend_from_slice(extra);
        let (code, stdout, err) = run(&argv);
        assert_eq!(code, 0, "{err}");
        json(&stdout)
    };
    let a = tmp.path("a.tiff");
    let b = tmp.path("b.tiff");
    let c = tmp.path("c.tiff");
    let ra = convert(&a, &[]);
    let rb = convert(&b, &[]);
    let rc = convert(&c, &["--density-gamma", "1.5"]);

    let ha = ra["conversion"]["params_hash"].as_str().unwrap();
    let hb = rb["conversion"]["params_hash"].as_str().unwrap();
    let hc = rc["conversion"]["params_hash"].as_str().unwrap();
    assert_eq!(ha, hb, "identical params must share a hash");
    assert_ne!(ha, hc, "a changed knob must change the hash");
}

#[test]
fn telemetry_log_write_failure_is_fail_soft() {
    // The JSONL-log sink is fail-soft too: point NC_TELEMETRY_LOG under a path
    // whose parent is a regular file (create_dir_all fails), and the conversion
    // must still exit 0 with a stderr warning.
    let tmp = TempDir::new("tel-log-failsoft");
    let out = tmp.path("out.tiff");
    let blocker = tmp.path("blocker");
    std::fs::write(&blocker, b"not a directory").unwrap();
    let bad_log = tmp.path("blocker/telemetry.jsonl"); // parent is a file

    let (code, _stdout, stderr) = run_env(
        &[
            "convert",
            fixture("hdr-48bit.tif").to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
            "--telemetry",
            "--report",
            "none",
        ],
        &[("NC_TELEMETRY_LOG", bad_log.to_str().unwrap())],
    );
    assert_eq!(
        code, 0,
        "a JSONL-log write failure must not fail the run:\n{stderr}"
    );
    assert!(is_tiff(&out), "the output TIFF must still be written");
    assert!(
        stderr.to_lowercase().contains("telemetry"),
        "the log write failure must be warned on stderr: {stderr}"
    );
}

#[test]
fn telemetry_outcome_reports_clipping_and_warnings() {
    // End-to-end pinning of the orchestrator → record `outcome` wiring
    // (`report.warnings.len()` and `EncodeReport::clipped_total`), which the
    // shape-only tests never exercise. A +12-stop `--print-exposure` guarantees
    // u16 clipping (and thus a clipping warning), so both counters must be > 0.
    let tmp = TempDir::new("tel-outcome-clip");
    let out = tmp.path("out.tiff");
    let (code, stdout, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--exposure",
        "12",
        "--telemetry-file",
        "-",
        "--report",
        "none",
    ]);
    assert_eq!(code, 0, "clipping run should still succeed:\n{err}");
    let record = json(&stdout);
    let outcome = &record["outcome"];
    assert!(
        outcome["clipped"].as_u64().unwrap() > 0,
        "a +12-stop exposure must report clipped samples: {outcome}"
    );
    assert!(
        outcome["warnings"].as_u64().unwrap() >= 1,
        "the clipping warning must be counted in outcome.warnings: {outcome}"
    );
}

#[test]
fn telemetry_outcome_counts_ir_ignored_warning() {
    // A separate warning source than clipping: converting an HDRi scan *without*
    // --export-ir raises the "IR plane preserved but not used" warning, which must
    // flow into outcome.warnings — proving the count isn't clipping-specific.
    let tmp = TempDir::new("tel-outcome-ir");
    let out = tmp.path("out.tiff");
    let (code, stdout, err) = run(&[
        "convert",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-master", // f32 never clips, so the IR-ignored warning is isolated
        "--film-base",
        "0.9,0.55,0.42",
        "--telemetry-file",
        "-",
        "--report",
        "none",
    ]);
    assert_eq!(code, 0, "HDRi convert should succeed:\n{err}");
    let record = json(&stdout);
    let outcome = &record["outcome"];
    assert_eq!(outcome["clipped"].as_u64().unwrap(), 0, "f32 must not clip");
    assert!(
        outcome["warnings"].as_u64().unwrap() >= 1,
        "the IR-ignored warning must be counted in outcome.warnings: {outcome}"
    );
}

#[test]
fn telemetry_key_in_recipe_is_rejected() {
    // Telemetry flags are *operational*, not recipe keys: a recipe (`--params`)
    // carrying a `telemetry` key must be rejected by `deny_unknown_fields` (exit 2,
    // usage), never silently accepted as if telemetry were a conversion knob.
    let tmp = TempDir::new("tel-recipe-key");
    let recipe = tmp.path("recipe.json");
    std::fs::write(&recipe, r#"{"recipe_version":3,"telemetry":true}"#).unwrap();
    let out = tmp.path("out.tiff");
    let (code, _stdout, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(
        code, 2,
        "a telemetry key in a recipe must be a usage error (exit 2): {err}"
    );
    assert!(
        !out.exists(),
        "no artifact may be written on a rejected recipe"
    );
}

#[test]
fn telemetry_params_hash_covers_the_decode() {
    // params_hash (over the effective recipe JSON) must cover the decode's keys, so
    // tweaking one changes the hash.
    let tmp = TempDir::new("tel-curve");
    let fix = fixture("hdr-48bit.tif");
    let convert = |out: &Path, extra: &[&str]| -> serde_json::Value {
        let out = out.to_str().unwrap();
        let mut argv = vec![
            "convert",
            fix.to_str().unwrap(),
            "-o",
            out,
            "--film-base",
            "0.9,0.55,0.42",
            "--telemetry-file",
            "-",
            "--report",
            "none",
        ];
        argv.extend_from_slice(extra);
        let (code, stdout, err) = run(&argv);
        assert_eq!(code, 0, "telemetry should succeed:\n{err}");
        json(&stdout)
    };
    let ra = convert(&tmp.path("a.tiff"), &[]);
    let rb = convert(&tmp.path("b.tiff"), &["--density-gamma", "1.5"]);

    assert_ne!(
        ra["conversion"]["params_hash"], rb["conversion"]["params_hash"],
        "a changed decode knob must change params_hash"
    );
}

// ---------------------------------------------------------------------------
// roll (batch) — convert N frames from one shared, frozen recipe
// ---------------------------------------------------------------------------

/// Write `contents` to `path`, returning the path (for building recipes /
/// manifests in a test's temp dir).
fn write_file(path: &Path, contents: &str) -> PathBuf {
    std::fs::write(path, contents).unwrap();
    path.to_path_buf()
}

/// A hand-authored frozen roll recipe: an explicit roll-fixed film base, so
/// every frame converts deterministically. It states no `output`,
/// so every frame is the default SDR TIFF; the container-aware naming has its own
/// coverage in `roll_checks_explicit_manifest_suffixes_and_derives_per_frame_names`.
const ROLL_RECIPE: &str = r#"{
  "recipe_version": 3,
  "calibration": {
    "film_base": { "explicit": [0.9, 0.55, 0.42] }
  }
}"#;

#[test]
fn roll_converts_a_batch_from_a_shared_frozen_recipe() {
    let tmp = TempDir::new("roll-batch");
    let recipe = write_file(&tmp.path("roll.json"), ROLL_RECIPE);
    let out_dir = tmp.path("out");
    let (code, stdout, err) = run(&[
        "roll",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "roll should succeed:\n{stdout}\n{err}");

    // Per-frame outputs, named <stem>_positive.tiff, and no sidecars.
    let hdr_out = out_dir.join("hdr-48bit_positive.tiff");
    let hdri_out = out_dir.join("hdri-64bit_positive.tiff");
    assert!(is_tiff(&hdr_out), "first frame output must be a TIFF");
    assert!(is_tiff(&hdri_out), "second frame output must be a TIFF");
    assert!(!sidecar_of(&hdr_out).exists());

    let report = json(&stdout);
    assert_eq!(report["command"], "roll");
    assert_eq!(report["summary"]["total"], 2);
    assert_eq!(report["summary"]["succeeded"], 2);
    assert_eq!(report["summary"]["failed"], 0);
    let frames = report["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 2);
    for f in frames {
        assert_eq!(f["status"], "ok");
        // Every frame ran the shared frozen base. f32 round-trips through JSON as
        // f64, so compare approximately.
        let fb = &f["film_base"];
        assert!(
            (fb["r"].as_f64().unwrap() - 0.9).abs() < 1e-6
                && (fb["g"].as_f64().unwrap() - 0.55).abs() < 1e-6
                && (fb["b"].as_f64().unwrap() - 0.42).abs() < 1e-6,
            "per-frame film base: {fb}"
        );
    }
}

#[test]
fn roll_is_byte_identical_on_rerun() {
    // Determinism: the same batch + same recipe ⇒ byte-identical output per frame.
    let tmp = TempDir::new("roll-determinism");
    let recipe = write_file(&tmp.path("roll.json"), ROLL_RECIPE);
    let run_into = |dir: &Path| {
        let (code, _out, err) = run(&[
            "roll",
            fixture("hdr-48bit.tif").to_str().unwrap(),
            "--out-dir",
            dir.to_str().unwrap(),
            "--params",
            recipe.to_str().unwrap(),
        ]);
        assert_eq!(code, 0, "{err}");
        std::fs::read(dir.join("hdr-48bit_positive.tiff")).unwrap()
    };
    let a = run_into(&tmp.path("out-a"));
    let b = run_into(&tmp.path("out-b"));
    assert_eq!(a, b, "re-running a roll must be byte-identical");
}

#[test]
fn roll_frame_local_override_applies_to_just_that_frame() {
    // A manifest gives frame 2 a per-frame exposure override; frame 1 runs
    // the shared recipe unchanged. Prove per-frame isolation by matching each
    // roll output byte-for-byte against the equivalent single `hanten convert`.
    let tmp = TempDir::new("roll-override");
    let recipe = write_file(&tmp.path("roll.json"), ROLL_RECIPE);
    let hdr = fixture("hdr-48bit.tif");
    let hdri = fixture("hdri-64bit.tif");
    let manifest = write_file(
        &tmp.path("frames.json"),
        &format!(
            r#"{{ "frames": [
                 {{ "input": {hdr:?} }},
                 {{ "input": {hdri:?}, "params": {{ "scene_correction": {{ "exposure": 0.5 }} }} }}
               ] }}"#,
            hdr = hdr.to_str().unwrap(),
            hdri = hdri.to_str().unwrap(),
        ),
    );
    let out_dir = tmp.path("out");
    let (code, stdout, err) = run(&[
        "roll",
        "--frames",
        manifest.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(
        code, 0,
        "roll with manifest should succeed:\n{stdout}\n{err}"
    );

    // The override is recorded on frame 2 only.
    let report = json(&stdout);
    let frames = report["frames"].as_array().unwrap();
    assert!(frames[0].get("overrides").is_none() || frames[0]["overrides"].is_null());
    assert_eq!(frames[1]["overrides"]["scene_correction"]["exposure"], 0.5);

    // Frame 1 (no override) == single convert with just the shared recipe.
    let ref1 = tmp.path("ref1.tiff");
    let (c1, _o, e1) = run(&[
        "convert",
        hdr.to_str().unwrap(),
        "-o",
        ref1.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(c1, 0, "{e1}");
    assert_eq!(
        std::fs::read(out_dir.join("hdr-48bit_positive.tiff")).unwrap(),
        std::fs::read(&ref1).unwrap(),
        "un-overridden frame must match a plain convert"
    );

    // Frame 2 == single convert with the shared recipe + the same override.
    let ref2 = tmp.path("ref2.tiff");
    let (c2, _o, e2) = run(&[
        "convert",
        hdri.to_str().unwrap(),
        "-o",
        ref2.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
        "--exposure",
        "0.5",
    ]);
    assert_eq!(c2, 0, "{e2}");
    assert_eq!(
        std::fs::read(out_dir.join("hdri-64bit_positive.tiff")).unwrap(),
        std::fs::read(&ref2).unwrap(),
        "overridden frame must match a convert carrying the same override"
    );
    // The override actually changed the pixels (frame 2 differs from its no-override form).
    let ref2_plain = tmp.path("ref2-plain.tiff");
    let (c3, _o, e3) = run(&[
        "convert",
        hdri.to_str().unwrap(),
        "-o",
        ref2_plain.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(c3, 0, "{e3}");
    assert_ne!(
        std::fs::read(&ref2).unwrap(),
        std::fs::read(&ref2_plain).unwrap(),
        "the print-exposure override must change the output"
    );
}

#[test]
fn a_roll_frame_resolves_as_the_equivalent_convert_and_warns_only_on_roll_wide_values() {
    // Each frame's `params` merges onto the shared recipe; the frame must render, and
    // hash its recipe, exactly as `convert --params <the merged recipe>`. The overrides
    // pick the cases a merge could resolve differently: a clamp (frame-local), a
    // `rendering` switch whose destination is derived rather than stated, and a decode
    // key. Only the last two are roll-wide.
    let tmp = TempDir::new("roll-equivalence");
    let shared = r#""recipe_version": 3,
        "calibration": {"film_base": {"explicit": [0.9, 0.55, 0.42]}}"#;
    let roll = r#""roll": {"white_balance": [1.05, 1.0, 0.95], "white_stops": 1.7}"#;
    let recipe = write_file(&tmp.path("roll.json"), &format!("{{{shared}, {roll}}}"));
    let input = fixture("hdr-48bit.tif");
    let cases = [
        (
            "clamp",
            r#"{"roll": {"white_stops": 2.0}}"#,
            r#""roll": {"white_balance": [1.05, 1.0, 0.95], "white_stops": 2.0}"#.to_string(),
        ),
        (
            "direct",
            r#"{"rendering": "direct"}"#,
            format!(r#"{roll}, "rendering": "direct""#),
        ),
        (
            "decode",
            r#"{"reconstruction": {"linearization": 1.7}}"#,
            format!(r#"{roll}, "reconstruction": {{"linearization": 1.7}}"#),
        ),
    ];
    let frames: Vec<String> = cases
        .iter()
        .map(|(name, params, _)| {
            format!(r#"{{"input": {input:?}, "output": "{name}.tiff", "params": {params}}}"#)
        })
        .collect();
    let manifest = write_file(
        &tmp.path("frames.json"),
        &format!(r#"{{"frames": [{}]}}"#, frames.join(",")),
    );
    let out_dir = tmp.path("out");
    let (code, stdout, err) = run(&[
        "roll",
        "--frames",
        manifest.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);

    for (i, (name, _, merged)) in cases.iter().enumerate() {
        let merged = write_file(
            &tmp.path(&format!("{name}.json")),
            &format!("{{{shared}, {merged}}}"),
        );
        let reference = tmp.path(&format!("{name}-convert.tiff"));
        let (code, stdout, err) = run(&[
            "convert",
            input.to_str().unwrap(),
            "-o",
            reference.to_str().unwrap(),
            "--params",
            merged.to_str().unwrap(),
        ]);
        assert_eq!(code, 0, "{name}: {err}");
        let frame = &report["frames"][i];
        assert_eq!(
            frame["identity"]["params_hash"],
            json(&stdout)["identity"]["params_hash"],
            "{name}"
        );
        assert_eq!(
            std::fs::read(out_dir.join(format!("{name}.tiff"))).unwrap(),
            std::fs::read(&reference).unwrap(),
            "{name}"
        );
    }
    // `direct` derived its own destination (the HDR float TIFF), not the roll's.
    assert_eq!(read_tiff_bits(&out_dir.join("direct.tiff")), 32);

    let warned: Vec<&str> = report["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|w| w.as_str())
        .filter(|w| w.contains("override resolves"))
        .collect();
    let keys: Vec<&str> = warned
        .iter()
        .map(|w| w.split('`').nth(3).unwrap())
        .collect();
    assert_eq!(
        keys,
        ["rendering", "reconstruction.linearization"],
        "{warned:#?}"
    );
    // The derived destination rides the `rendering` warning; no `output` one names a
    // key the frame never wrote.
    assert!(
        warned[0].contains(r#"to "direct" (destination {"range":"hdr""#),
        "{}",
        warned[0]
    );
    assert!(
        warned[1].contains("to 1.7, where the roll's is 1.8"),
        "{}",
        warned[1]
    );
}

#[test]
fn roll_records_a_failed_frame_and_exits_nonzero() {
    // Batch resilience: a bad frame (missing input → decode error) is recorded in
    // the report and the roll continues, converting the good frame; the roll then
    // exits non-zero. stdout stays the JSON report even on the failing exit.
    let tmp = TempDir::new("roll-partial");
    let recipe = write_file(&tmp.path("roll.json"), ROLL_RECIPE);
    let out_dir = tmp.path("out");
    let missing = tmp.path("does-not-exist.tif");
    let (code, stdout, _err) = run(&[
        "roll",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        missing.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 1, "a failed frame must make the roll exit non-zero");
    let report = json(&stdout);
    assert_eq!(report["summary"]["succeeded"], 1);
    assert_eq!(report["summary"]["failed"], 1);
    // The good frame still produced an output.
    assert!(is_tiff(&out_dir.join("hdr-48bit_positive.tiff")));
    // The failed frame carries an error message and "failed" status.
    let failed = report["frames"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["status"] == "failed")
        .expect("a failed frame entry");
    assert!(
        failed["error"].is_string(),
        "failed frame has an error: {failed}"
    );
}

#[test]
fn roll_rejects_same_stem_output_collision() {
    // Two inputs with the same stem in different dirs map to one output name —
    // caught loudly up front, before anything is written.
    let tmp = TempDir::new("roll-collision");
    let recipe = write_file(&tmp.path("roll.json"), ROLL_RECIPE);
    let dir_a = tmp.path("a");
    let dir_b = tmp.path("b");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    std::fs::copy(fixture("hdr-48bit.tif"), dir_a.join("frame.tif")).unwrap();
    std::fs::copy(fixture("hdr-48bit.tif"), dir_b.join("frame.tif")).unwrap();
    let (code, _out, err) = run(&[
        "roll",
        dir_a.join("frame.tif").to_str().unwrap(),
        dir_b.join("frame.tif").to_str().unwrap(),
        "--out-dir",
        tmp.path("out").to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "an output-name collision is a usage error");
    assert!(err.contains("collides"), "stderr should explain: {err}");
}

#[test]
fn roll_directory_input_expands_to_sorted_tiffs() {
    // A positional directory expands to its .tif/.tiff files (sorted), non-TIFFs
    // ignored. Copy the fixture under two names + a stray .txt, roll the dir.
    let tmp = TempDir::new("roll-dir");
    let recipe = write_file(&tmp.path("roll.json"), ROLL_RECIPE);
    let scans = tmp.path("scans");
    std::fs::create_dir_all(&scans).unwrap();
    std::fs::copy(fixture("hdr-48bit.tif"), scans.join("b.tif")).unwrap();
    std::fs::copy(fixture("hdr-48bit.tif"), scans.join("a.tiff")).unwrap();
    std::fs::write(scans.join("notes.txt"), b"not a scan").unwrap();
    let out_dir = tmp.path("out");
    let (code, stdout, err) = run(&[
        "roll",
        scans.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "directory roll should succeed:\n{stdout}\n{err}");
    let report = json(&stdout);
    assert_eq!(report["summary"]["total"], 2, "only the two TIFFs convert");
    // Expanded in sorted order: a.tiff before b.tif.
    let frames = report["frames"].as_array().unwrap();
    assert!(
        frames[0]["input"].as_str().unwrap().ends_with("a.tiff"),
        "frames are sorted: {report}"
    );
    assert!(frames[1]["input"].as_str().unwrap().ends_with("b.tif"));
    assert!(is_tiff(&out_dir.join("a_positive.tiff")));
    assert!(is_tiff(&out_dir.join("b_positive.tiff")));
    // The .txt is not treated as a frame.
    assert!(!out_dir.join("notes_positive.tiff").exists());
}

#[test]
fn roll_empty_batch_errors_loudly_on_both_paths() {
    // An empty `--frames` manifest and positional inputs matching no files both
    // fail loudly as usage errors (exit 2), before anything is written.
    let tmp = TempDir::new("roll-empty");
    let recipe = write_file(&tmp.path("roll.json"), ROLL_RECIPE);

    // (a) empty manifest.
    let manifest = write_file(&tmp.path("empty.json"), r#"{ "frames": [] }"#);
    let (code, _out, err) = run(&[
        "roll",
        "--frames",
        manifest.to_str().unwrap(),
        "--out-dir",
        tmp.path("out-a").to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "an empty manifest is a usage error");
    assert!(
        err.contains("lists no frames"),
        "stderr should explain: {err}"
    );

    // (b) a positional directory that contains no TIFFs.
    let empty_dir = tmp.path("empty-dir");
    std::fs::create_dir_all(&empty_dir).unwrap();
    let (code, _out, err) = run(&[
        "roll",
        empty_dir.to_str().unwrap(),
        "--out-dir",
        tmp.path("out-b").to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "inputs matching no files is a usage error");
    assert!(
        err.contains("matched no files"),
        "stderr should explain: {err}"
    );
}

/// A shared recipe with a NON-explicit (region) film base — every frame
/// re-estimates its own Dmin, so the roll is not truly frozen.
const ROLL_RECIPE_REGION: &str = r#"{
  "recipe_version": 3,
  "calibration": { "film_base": { "region": [0, 0, 502, 462] } }
}"#;

#[test]
fn roll_warns_when_film_base_is_not_frozen() {
    // A non-explicit shared base is a loud roll-level warning (the roll is not
    // color-consistent), but not a hard failure — the batch still converts.
    let tmp = TempDir::new("roll-notfrozen");
    let recipe = write_file(&tmp.path("region.json"), ROLL_RECIPE_REGION);
    let (code, stdout, err) = run(&[
        "roll",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "--out-dir",
        tmp.path("out").to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(
        code, 0,
        "a non-frozen base warns, not fails:\n{stdout}\n{err}"
    );
    let report = json(&stdout);
    assert_eq!(report["summary"]["succeeded"], 1);
    // The roll-level warning names the problem and the fix, and is echoed to stderr.
    let w = report["warnings"]
        .as_array()
        .expect("roll-level warnings array");
    assert!(
        w.iter().any(|m| m.as_str().unwrap().contains("NOT frozen")
            && m.as_str().unwrap().contains("hanten measure-base")),
        "roll-level not-frozen warning present: {report}"
    );
    assert!(
        err.contains("NOT frozen"),
        "warning echoed to stderr: {err}"
    );
}

#[test]
fn roll_strict_promotes_a_warning_while_still_emitting_the_report() {
    // `--strict` turns the not-frozen roll-level warning into a non-zero exit, but
    // the machine-readable report still lands on stdout first (pairs with the
    // warning test above). The frames themselves convert (failed == 0).
    let tmp = TempDir::new("roll-strict");
    let recipe = write_file(&tmp.path("region.json"), ROLL_RECIPE_REGION);
    let (code, stdout, err) = run(&[
        "roll",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "--out-dir",
        tmp.path("out").to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
        "--strict",
    ]);
    assert_eq!(code, 1, "--strict promotes the warning to a failing exit");
    let report = json(&stdout); // report still emitted before the gate
    assert_eq!(
        report["summary"]["failed"], 0,
        "the frame converted; the non-zero exit is the strict gate, not a frame failure"
    );
    assert!(
        !report["warnings"].as_array().unwrap().is_empty(),
        "the promoted warning is still in the report: {report}"
    );
    assert!(err.contains("strict"), "stderr should explain: {err}");
}

#[test]
fn roll_warns_on_per_frame_film_base_override() {
    // film_base is meant to be roll-fixed, but a per-frame override that sets it is
    // applied (the frame converts with its overridden base) with a loud,
    // `--strict`-promotable warning — not rejected.
    let tmp = TempDir::new("roll-fb-override");
    let recipe = write_file(&tmp.path("roll.json"), ROLL_RECIPE);
    let hdr = fixture("hdr-48bit.tif");
    let manifest_txt = format!(
        r#"{{ "frames": [
             {{ "input": {hdr:?},
                "params": {{ "calibration": {{ "film_base": {{ "explicit": [0.8, 0.5, 0.4] }} }} }} }}
           ] }}"#,
        hdr = hdr.to_str().unwrap(),
    );
    let manifest = write_file(&tmp.path("frames.json"), &manifest_txt);
    let roll_args = |out: &str, strict: bool| -> Vec<String> {
        let mut a = vec![
            "roll".to_string(),
            "--frames".to_string(),
            manifest.to_str().unwrap().to_string(),
            "--out-dir".to_string(),
            tmp.path(out).to_str().unwrap().to_string(),
            "--params".to_string(),
            recipe.to_str().unwrap().to_string(),
        ];
        if strict {
            a.push("--strict".to_string());
        }
        a
    };

    // Without --strict: the frame converts (exit 0) with a loud roll-level warning.
    let args = roll_args("out", false);
    let (code, stdout, err) = run(&args.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(
        code, 0,
        "an override warns, it does not fail:\n{stdout}\n{err}"
    );
    let report = json(&stdout);
    assert_eq!(
        report["summary"]["succeeded"], 1,
        "the frame still converts"
    );
    let w = report["warnings"]
        .as_array()
        .expect("roll-level warnings array");
    assert!(
        w.iter().any(|m| m
            .as_str()
            .unwrap()
            .contains("resolves `calibration.film_base`")),
        "the per-frame film_base override warns loudly: {report}"
    );
    assert!(
        err.contains("resolves `calibration.film_base`"),
        "warning echoed to stderr: {err}"
    );

    // With --strict: the same warning promotes to a non-zero exit, report still emits.
    let args = roll_args("out-strict", true);
    let (code, stdout, err) = run(&args.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(
        code, 1,
        "--strict promotes the override warning to a failing exit"
    );
    let report = json(&stdout);
    assert_eq!(
        report["summary"]["failed"], 0,
        "the frame converted; the exit is the strict gate"
    );
    assert!(!report["warnings"].as_array().unwrap().is_empty());
    assert!(err.contains("strict"), "stderr should explain: {err}");
}

#[test]
fn roll_failed_frame_keeps_a_warning_raised_before_the_failure() {
    // A frame that warns (a non-uniform `--base-region` sample, raised by the film-base
    // estimate) and *then* fails (its output path is an existing directory, so the write
    // fails after the render) still reports the earlier warning.
    let tmp = TempDir::new("roll-warn-then-fail");
    let recipe = write_file(
        &tmp.path("warn-then-fail.json"),
        r#"{ "recipe_version": 3, "calibration": { "film_base": { "region": [0, 0, 40, 40] } } }"#,
    );
    let out = tmp.path("out");
    std::fs::create_dir_all(out.join("frame.tiff")).unwrap();
    let manifest = write_file(
        &tmp.path("frames.json"),
        &format!(
            r#"{{ "frames": [ {{ "input": {:?}, "output": "frame.tiff" }} ] }}"#,
            fixture("hdr-48bit.tif").to_str().unwrap()
        ),
    );
    let (code, stdout, _err) = run(&[
        "roll",
        "--frames",
        manifest.to_str().unwrap(),
        "--out-dir",
        out.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 1, "the failed frame makes the roll exit non-zero");
    let report = json(&stdout);
    let f = &report["frames"][0];
    assert_eq!(f["status"], "failed");
    assert!(f["error"].is_string(), "failed frame carries an error: {f}");
    assert!(
        f["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("is not uniform")),
        "the warning raised before the failure survives in the report: {f}"
    );
}

#[test]
fn roll_two_frame_output_is_byte_identical_on_rerun() {
    // Determinism across a MULTI-frame batch: every per-frame output is
    // byte-identical when the same batch + recipe runs twice.
    let tmp = TempDir::new("roll-determinism2");
    let recipe = write_file(&tmp.path("roll.json"), ROLL_RECIPE);
    let run_into = |dir: &Path| {
        let (code, _out, err) = run(&[
            "roll",
            fixture("hdr-48bit.tif").to_str().unwrap(),
            fixture("hdri-64bit.tif").to_str().unwrap(),
            "--out-dir",
            dir.to_str().unwrap(),
            "--params",
            recipe.to_str().unwrap(),
        ]);
        assert_eq!(code, 0, "{err}");
    };
    let a = tmp.path("out-a");
    let b = tmp.path("out-b");
    run_into(&a);
    run_into(&b);
    for name in ["hdr-48bit_positive.tiff", "hdri-64bit_positive.tiff"] {
        assert_eq!(
            std::fs::read(a.join(name)).unwrap(),
            std::fs::read(b.join(name)).unwrap(),
            "{name} must be byte-identical across runs"
        );
    }
}

#[test]
fn roll_manifest_output_into_subdirectory_is_created() {
    // A manifest output naming a subdirectory (`sub/x.tiff`) has its parent
    // created before the encode, so the write succeeds.
    let tmp = TempDir::new("roll-subdir");
    let recipe = write_file(&tmp.path("roll.json"), ROLL_RECIPE);
    let hdr = fixture("hdr-48bit.tif");
    let manifest = write_file(
        &tmp.path("frames.json"),
        &format!(
            r#"{{ "frames": [ {{ "input": {hdr:?}, "output": "sub/deep/x.tiff" }} ] }}"#,
            hdr = hdr.to_str().unwrap(),
        ),
    );
    let out_dir = tmp.path("out");
    let (code, stdout, err) = run(&[
        "roll",
        "--frames",
        manifest.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "subdir output should be created:\n{stdout}\n{err}");
    assert!(
        is_tiff(&out_dir.join("sub/deep/x.tiff")),
        "the manifest subdirectory output was written"
    );
}

#[test]
fn a_roll_manifest_output_naming_the_out_dir_is_refused_not_written_beside_it() {
    // `"output": "."` is the natural manifest spelling for "put it in the
    // --out-dir", and `out_dir.join(".")` makes `<out-dir>/.`. `Path::file_name()`
    // normalises the `.` away, so completing it wrote `<out-dir>.jpg` — every frame
    // of the roll *outside* the directory the user named, at exit 0, with the report
    // agreeing and `ensure_roll_targets_distinct` unable to see it (it only compares
    // targets against each other). Refused at exit 2 instead.
    let tmp = TempDir::new("roll-out-dir-dot");
    let recipe = write_file(&tmp.path("roll.json"), ROLL_RECIPE);
    let hdr = fixture("hdr-48bit.tif");
    let manifest = write_file(
        &tmp.path("frames.json"),
        &format!(
            r#"{{ "frames": [ {{ "input": {hdr:?}, "output": "." }} ] }}"#,
            hdr = hdr.to_str().unwrap(),
        ),
    );
    let out_dir = tmp.path("out");
    let (code, stdout, err) = run(&[
        "roll",
        "--frames",
        manifest.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{stdout}\n{err}");
    assert!(err.contains("names a directory"), "{err}");
    // Attributed to the frame, with a remedy the manifest can act on — and *not*
    // the convert-shaped one, which would tell a reader already running
    // `hanten roll --out-dir` to use it. Asserting the losing wording is absent is
    // the only check that can tell the two arms apart.
    assert!(err.contains("frame "), "{err}");
    assert!(err.contains(hdr.to_str().unwrap()), "{err}");
    assert!(!err.contains("hanten roll --out-dir"), "{err}");
    // The file that used to appear beside the out-dir must not exist.
    assert!(
        !tmp.path("out.tiff").exists() && !tmp.path("out.jpg").exists(),
        "a sibling of the --out-dir was written: {err}"
    );

    // Falsifiable control: the same manifest with a file name works, and lands
    // *inside* the out-dir.
    let ok_manifest = write_file(
        &tmp.path("frames-ok.json"),
        &format!(
            r#"{{ "frames": [ {{ "input": {hdr:?}, "output": "frame" }} ] }}"#,
            hdr = hdr.to_str().unwrap(),
        ),
    );
    let (code, stdout, err) = run(&[
        "roll",
        "--frames",
        ok_manifest.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{stdout}\n{err}");
    assert!(is_tiff(&out_dir.join("frame.tiff")), "{err}");
}

// --- the film master ---------------------------------------------------------

/// Read the interleaved f32 samples out of a float TIFF, together with the
/// per-sample bit depth and TIFF `SampleFormat` code (3 = IEEE float). Used to
/// prove the `film-master` container is genuinely unclamped 32-bit float rather
/// than a quantized image that merely happens to look right.
fn read_f32_tiff(path: &Path) -> (Vec<f32>, u16, u16) {
    use tiff::decoder::{Decoder, DecodingResult};
    use tiff::tags::Tag;
    let mut dec = Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()))
        .unwrap()
        .with_limits(tiff::decoder::Limits::unlimited());
    // Both tags are 3-element SHORT arrays here (one entry per sample), so read
    // them as vectors and require every channel to agree.
    let mut all_equal = |tag: Tag| -> u16 {
        let v = dec.get_tag_u16_vec(tag).unwrap();
        assert_eq!(v.len(), 3, "{tag:?} must have one entry per sample: {v:?}");
        assert!(
            v.iter().all(|x| *x == v[0]),
            "{tag:?} channels differ: {v:?}"
        );
        v[0]
    };
    let bits = all_equal(Tag::BitsPerSample);
    let format = all_equal(Tag::SampleFormat);
    let samples = match dec.read_image().unwrap() {
        DecodingResult::F32(v) => v,
        other => panic!("film-master must be a float TIFF, got a different sample type: {other:?}"),
    };
    (samples, bits, format)
}

/// The per-sample bit depth of a written TIFF, without caring about the sample type
/// — for asserting that a run landed on 16-bit where `read_f32_tiff` would panic.
fn read_tiff_bits(path: &Path) -> u16 {
    use tiff::decoder::Decoder;
    use tiff::tags::Tag;
    let mut dec = Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()))
        .unwrap()
        .with_limits(tiff::decoder::Limits::unlimited());
    let v = dec.get_tag_u16_vec(Tag::BitsPerSample).unwrap();
    assert!(
        v.iter().all(|x| *x == v[0]),
        "BitsPerSample channels differ: {v:?}"
    );
    v[0]
}

/// The embedded ICC blob (`ICCProfile`, tag 34675) of a written TIFF. Only ever
/// compared against *another run of the same binary* — lcms2's synthesized bytes
/// differ per target, so a checked-in hash would be red on the other CI host.
fn read_icc_tag(path: &Path) -> Vec<u8> {
    use tiff::decoder::Decoder;
    use tiff::tags::Tag;
    let mut dec = Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()))
        .unwrap()
        .with_limits(tiff::decoder::Limits::unlimited());
    dec.get_tag_u8_vec(Tag::Unknown(34675))
        .unwrap_or_else(|e| panic!("{} has no ICCProfile tag: {e}", path.display()))
}

/// The samples of a single-channel TIFF, in whichever type it was written as.
#[derive(Debug)]
enum GraySamples {
    U16(Vec<u16>),
    F32(Vec<f32>),
}

/// Read a one-channel TIFF (the `--export-ir` plane): per-sample bit depth, TIFF
/// `SampleFormat` code (1 = unsigned int, 3 = IEEE float), and the samples.
fn read_gray_tiff(path: &Path) -> (u16, u16, GraySamples) {
    use tiff::decoder::{Decoder, DecodingResult};
    use tiff::tags::Tag;
    let mut dec = Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()))
        .unwrap()
        .with_limits(tiff::decoder::Limits::unlimited());
    let one = |tag: Tag, dec: &mut Decoder<_>| -> u16 {
        let v = dec.get_tag_u16_vec(tag).unwrap();
        assert_eq!(v.len(), 1, "{tag:?} must have one entry: {v:?}");
        v[0]
    };
    let bits = one(Tag::BitsPerSample, &mut dec);
    let format = one(Tag::SampleFormat, &mut dec);
    let samples = match dec.read_image().unwrap() {
        DecodingResult::U16(v) => GraySamples::U16(v),
        DecodingResult::F32(v) => GraySamples::F32(v),
        other => panic!("unexpected IR sample type: {other:?}"),
    };
    (bits, format, samples)
}

#[test]
fn film_master_writes_unclamped_float_acescg_and_reports_the_branch() {
    // The master round-trips unclamped finite ACEScg through a float TIFF and says
    // in the report exactly what ran: the decode, no rendering stage, NC film RGB v1
    // provenance.
    //
    // A steep slope and a low anchor are *reconstruction* controls (which the master
    // accepts), chosen so the placement pushes most samples well above 1.0 — that is
    // what makes the unclamped round-trip observable instead of vacuous. Value magnitudes only; no whole-file or post-transform checksum.
    let tmp = TempDir::new("film-master");
    let out = tmp.path("master.tiff");
    let (code, stdout, err) = run(&[
        "convert",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-master",
        "--film-base",
        "0.9,0.55,0.42",
        // This asserts the master is *unclamped*, which needs samples above 1.0: a
        // steep slope with mid-grey just above the base puts the anchor at
        // `0.05 + 0.745/5 ≈ 0.2`, pushing plenty of content past it.
        "--density-gamma",
        "5",
        "--anchor-mid-offset",
        "0.05",
    ]);
    assert_eq!(
        code, 0,
        "film-master convert should succeed:\n{stdout}\n{err}"
    );
    assert!(is_tiff(&out));

    let (samples, bits, format) = read_f32_tiff(&out);
    assert_eq!(bits, 32, "the master is 32-bit");
    assert_eq!(format, 3, "the master is IEEE float (SampleFormat 3)");
    let above_one = samples.iter().filter(|v| **v > 1.0).count();
    assert!(
        above_one > samples.len() / 2,
        "the fixture must actually exercise the unclamped range \
         ({above_one} of {} samples above 1.0)",
        samples.len()
    );
    assert!(
        samples.iter().all(|v| v.is_finite()),
        "every written sample must be finite here"
    );

    let report = json(&stdout);
    // Nothing was clipped or lost: the float path never reaches the u16 quantizer.
    assert_eq!(report["loss"]["clipped_low"], 0);
    assert_eq!(report["loss"]["clipped_high"], 0);
    assert_eq!(report["loss"]["non_finite"], 0);
    // What ran: the fixed decode and nothing after it.
    let nf = &report["chain"];
    assert_eq!(nf["destination"], "film-master");
    assert_eq!(nf["stages"], serde_json::json!([]));
    for stage in ["scene_correction", "look", "fit_range"] {
        assert!(nf.get(stage).is_none(), "{stage}: {nf}");
    }
    assert_eq!(report["working_mapping"], "nc-film-rgb-v1");
    let anchor = nf["decode"]["anchor"]
        .as_f64()
        .expect("the derived anchor is reported");
    assert!((anchor - 0.198_945_5).abs() < 1e-5, "{anchor}");

    // The recipe that states the same values reproduces the master byte-for-byte.
    let recipe = write_file(
        &tmp.path("master.json"),
        r#"{"recipe_version":3,
            "calibration":{"film_base":{"explicit":[0.9,0.55,0.42]}},
            "reconstruction":{"linearization":5.0,"anchor":{"mid-at-base-offset":0.05}},
            "output":"film-master"}"#,
    );
    let again = tmp.path("master2.tiff");
    let (code, _, err) = run(&[
        "convert",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "-o",
        again.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
        "--report",
        "none",
    ]);
    assert_eq!(code, 0, "the film-master recipe must load:\n{err}");
    assert_eq!(
        std::fs::read(&out).unwrap(),
        std::fs::read(&again).unwrap(),
        "the film-master recipe must reproduce the master"
    );
}

#[test]
fn film_master_embeds_its_own_icc_distinct_from_the_display_destinations() {
    // The written master's ICC tag is what tells a downstream tool the pixels are
    // linear ACEScg. What only the binary can show is that the tag reaches the file
    // and is not a display profile — two runs of one build, never a checked-in ICC
    // hash (lcms2's bytes differ per target).
    let tmp = TempDir::new("film-master-icc");
    let input = fixture("hdri-64bit.tif");
    let convert = |name: &str, extra: &[&str]| -> PathBuf {
        let out = tmp.path(name);
        let mut args = vec![
            "convert",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
            "--report",
            "none",
        ];
        args.extend_from_slice(extra);
        let (code, _stdout, err) = run(&args);
        assert_eq!(code, 0, "{name} should convert:\n{err}");
        out
    };
    let master = convert("master.tiff", &["--film-master"]);
    let icc = read_icc_tag(&master);
    assert!(
        icc.len() > 100,
        "an ICC profile must be embedded: {}",
        icc.len()
    );
    for display in ["display-p3", "adobe-rgb"] {
        let other = convert(&format!("{display}.tiff"), &["--gamut", display]);
        assert_ne!(
            icc,
            read_icc_tag(&other),
            "the master must not carry {display}'s profile"
        );
    }
}

#[test]
fn film_master_ir_export_follows_the_destination_depth_and_carries_the_plane() {
    // `--export-ir` writes the IR plane at the destination's depth, so under the film
    // master it flips 16-bit → f32. Correct by construction (one depth for the whole
    // run), but it is a user-visible container change, so pin it — together with the
    // rule that the IR plane is *carried*, never converted: the f32 export's samples
    // must equal a 16-bit destination's export, up to u16 quantization.
    let tmp = TempDir::new("film-master-ir");
    let input = fixture("hdri-64bit.tif");
    let convert = |name: &str, extra: &[&str]| -> PathBuf {
        let out = tmp.path(&format!("{name}.tiff"));
        let ir = tmp.path(&format!("{name}-ir.tiff"));
        let mut args = vec![
            "convert",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
            "--export-ir",
            ir.to_str().unwrap(),
            "--report",
            "none",
        ];
        args.extend_from_slice(extra);
        let (code, _stdout, err) = run(&args);
        assert_eq!(code, 0, "{name} should convert:\n{err}");
        ir
    };

    // A 16-bit destination: a 16-bit unsigned-integer IR export.
    let sdr_ir = convert("sdr", &[]);
    let (sdr_bits, sdr_format, sdr_samples) = read_gray_tiff(&sdr_ir);
    assert_eq!(
        (sdr_bits, sdr_format),
        (16, 1),
        "a 16-bit destination's IR export is 16-bit unsigned integer"
    );
    let GraySamples::U16(sdr_u16) = sdr_samples else {
        panic!("the SDR IR export must be u16, got {sdr_samples:?}");
    };

    // Under the film master the same flag writes f32 — its depth, unasked for.
    let master_ir = convert("master", &["--film-master"]);
    let (bits, format, master_samples) = read_gray_tiff(&master_ir);
    assert_eq!(
        (bits, format),
        (32, 3),
        "the film master's IR export follows its f32 depth"
    );
    let GraySamples::F32(master_f32) = master_samples else {
        panic!("the film-master IR export must be f32");
    };

    // Same plane, carried not consumed: the f32 samples reproduce the u16 ones.
    assert_eq!(master_f32.len(), sdr_u16.len());
    for (i, (&f, &q)) in master_f32.iter().zip(&sdr_u16).enumerate() {
        let requantized = (f.clamp(0.0, 1.0) * 65535.0).round() as u16;
        assert!(
            requantized.abs_diff(q) <= 1,
            "IR sample {i}: f32 {f} requantizes to {requantized}, the u16 export was {q}"
        );
    }
}

#[test]
fn film_master_telemetry_names_the_destination_and_the_written_depth() {
    // The record's `conversion.destination` is what distinguishes a master from a
    // display run, and `conversion.output_depth` says which depth was written; the
    // byte count pins that f32 is what actually landed on disk.
    let tmp = TempDir::new("film-master-telemetry");
    let out = tmp.path("master.tiff");
    let rec = tmp.path("run.json");
    let (code, _stdout, err) = run(&[
        "convert",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-master",
        "--film-base",
        "0.9,0.55,0.42",
        "--telemetry-file",
        rec.to_str().unwrap(),
        "--report",
        "none",
    ]);
    assert_eq!(code, 0, "film-master + telemetry should succeed:\n{err}");

    let record: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&rec).unwrap()).unwrap();
    let conv = &record["conversion"];
    assert_eq!(record["schema_version"], 11);
    assert_eq!(conv["destination"], "film-master");
    // The film master runs no chain stage, so none is timed.
    for stage in ["scene_correction", "look", "fit_range", "fit_gamut"] {
        assert!(record["timing_ms"].get(stage).is_none(), "{record}");
    }
    assert_eq!(
        conv["output_depth"], "f32",
        "the master writes f32, so the record's depth must say so: {conv}"
    );
    // Cross-check against the file the run actually wrote: 4 bytes per sample.
    let (samples, bits, _) = read_f32_tiff(&out);
    assert_eq!(bits, 32);
    let bytes = record["image"]["output_bytes"].as_u64().unwrap();
    assert!(
        bytes >= samples.len() as u64 * 4,
        "output_bytes {bytes} must cover {} f32 samples",
        samples.len()
    );

    // …and a 16-bit destination on the same fixture reports `u16`, so the assertion
    // above is about the destination and not a constant.
    let sdr_out = tmp.path("sdr.tiff");
    let sdr_rec = tmp.path("sdr.json");
    let (code, _stdout, err) = run(&[
        "convert",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "-o",
        sdr_out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--telemetry-file",
        sdr_rec.to_str().unwrap(),
        "--report",
        "none",
    ]);
    assert_eq!(code, 0, "{err}");
    let sdr: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&sdr_rec).unwrap()).unwrap();
    assert_eq!(sdr["conversion"]["destination"]["display"]["range"], "sdr");
    assert_eq!(sdr["conversion"]["output_depth"], "u16");
}

#[test]
fn roll_accepts_a_film_master_recipe() {
    // `hanten roll` has no output flags at all — its destination comes only from the
    // shared recipe — so `output` must be honoured there too, and the automatic
    // `<stem>_positive.tiff` name is correct for the master's TIFF container.
    let tmp = TempDir::new("roll-film-master");
    let recipe = write_file(
        &tmp.path("roll.json"),
        r#"{"recipe_version":3,"calibration":{"film_base":{"explicit":[0.9,0.55,0.42]}},
            "output":"film-master"}"#,
    );
    let out_dir = tmp.path("out");
    let (code, stdout, err) = run(&[
        "roll",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "a film-master roll recipe must convert:\n{err}");
    let report = json(&stdout);
    assert_eq!(report["summary"]["succeeded"], 1);
    let out = out_dir.join("hdri-64bit_positive.tiff");
    let (_, bits, format) = read_f32_tiff(&out);
    assert_eq!((bits, format), (32, 3), "each frame is an f32 master");
    // A roll with no per-frame override must stay warning-free about the destination,
    // so the override warning is genuinely caused by an override.
    let warnings = report["warnings"].as_array().cloned().unwrap_or_default();
    assert!(
        !warnings
            .iter()
            .any(|w| w.as_str().unwrap_or("").contains("resolves `output`")),
        "an un-overridden roll must not warn about the destination: {warnings:?}"
    );
}

#[test]
fn roll_frame_override_of_output_warns_and_is_strict_promotable() {
    // `output` is roll-fixed like `film_base`, and coarser: overriding it per frame
    // emits a frame of a different *image class* (a rendered u16 TIFF among unclamped
    // linear ACEScg masters).
    //
    // **The fixture must be IR-free.** `hdri-64bit.tif` carries an IR plane, so every
    // frame raises a per-frame "IR preserved but not used" warning, and
    // `strict_failure` is already true via `frames.iter().any(|f| !f.warnings.is_empty())`
    // — a no-override roll on that fixture exits 1 under `--strict` all by itself, which
    // made the promotion assertion below unfalsifiable. `hdr-48bit.tif` has no IR
    // plane, so `--strict` there exits 0 unless *this* warning fires, and the control
    // run below pins that.
    let tmp = TempDir::new("roll-preset-override");
    let input = fixture("hdr-48bit.tif");
    let recipe = write_file(
        &tmp.path("roll.json"),
        r#"{"recipe_version":3,"calibration":{"film_base":{"explicit":[0.9,0.55,0.42]}},
            "output":"film-master"}"#,
    );
    let manifest_for = |name: &str, body: &str| -> PathBuf { write_file(&tmp.path(name), body) };
    let overridden = manifest_for(
        "frames.json",
        &format!(
            r#"{{ "frames": [
                 {{ "input": {i:?}, "output": "master.tiff" }},
                 {{ "input": {i:?}, "output": "downgraded.tiff",
                    "params": {{ "output": {{ "display": {{}} }} }} }}
               ] }}"#,
            i = input.to_str().unwrap(),
        ),
    );
    let control = manifest_for(
        "frames-control.json",
        &format!(
            r#"{{ "frames": [ {{ "input": {i:?}, "output": "master.tiff" }} ] }}"#,
            i = input.to_str().unwrap(),
        ),
    );
    let roll = |manifest: &Path, out_dir: &Path, extra: &[&str]| -> (i32, String, String) {
        let mut argv: Vec<String> = [
            "roll",
            "--frames",
            manifest.to_str().unwrap(),
            "--out-dir",
            out_dir.to_str().unwrap(),
            "--params",
            recipe.to_str().unwrap(),
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        argv.extend(extra.iter().map(|s| s.to_string()));
        run(&argv.iter().map(String::as_str).collect::<Vec<_>>())
    };

    let out_dir = tmp.path("out");
    let (code, stdout, err) = roll(&overridden, &out_dir, &[]);
    assert_eq!(code, 0, "the override is applied, not rejected:\n{err}");

    // The warning names the frame and the key, and rides in the roll report (not just
    // stderr) so an agent piping stdout sees it.
    let report = json(&stdout);
    let warnings: Vec<String> = report["warnings"]
        .as_array()
        .expect("roll report must carry a warnings array")
        .iter()
        .map(|w| w.as_str().unwrap().to_string())
        .collect();
    let hit = warnings
        .iter()
        .find(|w| w.contains("resolves `output`"))
        .unwrap_or_else(|| panic!("no `output` override warning in {warnings:?}"));
    assert!(hit.contains("hdr-48bit.tif"), "{hit}");
    assert!(hit.contains("a different image"), "{hit}");
    assert!(
        err.contains("resolves `output`"),
        "and on stderr too: {err}"
    );

    // The override really did produce a different image class — that is the harm.
    assert_eq!(read_tiff_bits(&out_dir.join("master.tiff")), 32);
    assert_eq!(read_tiff_bits(&out_dir.join("downgraded.tiff")), 16);

    // Same shape as its two siblings: `--strict` promotes it to a non-zero exit…
    let (code, _stdout, err) = roll(&overridden, &tmp.path("strict-out"), &["--strict"]);
    assert_ne!(code, 0, "--strict must promote the warning:\n{err}");

    // …and the control that makes that falsifiable: the *same* recipe, fixture, and
    // `--strict` flag with no per-frame override exits 0 with no roll-level warning. So
    // the promotion above is caused by this warning and nothing else.
    let (code, stdout, err) = roll(&control, &tmp.path("control-out"), &["--strict"]);
    assert_eq!(
        code, 0,
        "an un-overridden --strict roll on the IR-free fixture must exit 0:\n{err}"
    );
    let control_report = json(&stdout);
    assert!(
        control_report["warnings"].is_null()
            || control_report["warnings"].as_array().unwrap().is_empty(),
        "control run must raise no roll-level warning: {}",
        control_report["warnings"]
    );
}

// ---------------------------------------------------------------------------
// Conversion identity + versioning (`core/conversion-versioning`)
// ---------------------------------------------------------------------------

/// The convert invocation the identity tests share: the default SDR Display P3 TIFF, an
/// explicit film base (so nothing is estimated per frame), and a clean stdout report.
fn convert_p3(input: &Path, out: &Path, extra: &[&str]) -> (i32, String, String) {
    let mut args = vec![
        "convert",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ];
    args.extend_from_slice(extra);
    run(&args)
}

#[test]
fn report_carries_every_identity_layer() {
    // Every report carries nc_version, the git commit and the behavioral
    // pipeline_version, and a convert report the hash of the recipe it ran.
    let tmp = TempDir::new("identity");
    let out = tmp.path("out.tiff");
    let (code, stdout, err) = convert_p3(&fixture("hdri-64bit.tif"), &out, &[]);
    assert_eq!(code, 0, "{err}");
    let id = &json(&stdout)["identity"];

    assert_eq!(id["nc_version"], env!("CARGO_PKG_VERSION"));
    // This worktree is a git checkout, so the commit must be a real short hash —
    // never the string "unknown" (absence is modelled as an omitted field).
    let commit = id["git_commit"]
        .as_str()
        .unwrap_or_else(|| panic!("git_commit must be present in a git build: {id}"));
    assert!(
        commit.len() >= 7 && commit.chars().all(|c| c.is_ascii_hexdigit()),
        "git_commit must be a short hex hash, got {commit:?}"
    );
    assert!(
        id["git_dirty"].is_boolean(),
        "git_dirty must be a bool: {id}"
    );
    // The report's pipeline_version must be THIS build's, not merely "an integer":
    // cross-check it against the only other place the binary publishes the label.
    assert_eq!(
        id["pipeline_version"].as_u64(),
        Some(pipeline_version_from_version_flag()),
        "the report's pipeline_version must match `nc --version`: {id}"
    );
    let hash = id["params_hash"].as_str().unwrap_or_else(|| panic!("{id}"));
    assert!(
        hash.len() == 16 && hash.chars().all(|c| c.is_ascii_hexdigit()),
        "{id}"
    );
    assert!(!id["target"].as_str().unwrap().is_empty());
}

/// The `pipeline_version` this binary prints from `--version` — the independent
/// witness a report's value is checked against.
fn pipeline_version_from_version_flag() -> u64 {
    let (code, stdout, err) = run(&["--version"]);
    assert_eq!(code, 0, "{err}");
    stdout
        .lines()
        .find_map(|l| l.strip_prefix("pipeline_version: "))
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_else(|| panic!("--version must print `pipeline_version: <n>`:\n{stdout}"))
        .parse()
        .expect("pipeline_version must be an integer")
}

#[test]
fn inspect_and_estimate_carry_build_identity_without_a_params_hash() {
    // "`identity`, every report" (design-spec §9) — not just conversions. An
    // `inspect`/`estimate` result is an artifact someone files, and `params_hash` is
    // genuinely absent there because no recipe was resolved (which is what makes
    // `Identity::new`'s `None` a real state rather than a construction artifact).
    let expected_version = pipeline_version_from_version_flag();
    for args in [
        vec!["inspect", fixture("hdri-64bit.tif").to_str().unwrap()],
        vec![
            "measure-base",
            fixture("hdri-64bit.tif").to_str().unwrap(),
            "--base-region",
            "0,0,502,462",
        ],
    ] {
        let (code, stdout, err) = run(&args);
        assert_eq!(code, 0, "{args:?}: {err}");
        let report = json(&stdout);
        let id = report
            .get("identity")
            .unwrap_or_else(|| panic!("{args:?}: no identity in {report}"));
        assert_eq!(id["nc_version"], env!("CARGO_PKG_VERSION"), "{args:?}");
        assert_eq!(id["pipeline_version"].as_u64(), Some(expected_version));
        assert!(!id["target"].as_str().unwrap().is_empty(), "{args:?}");
        assert!(
            id.get("params_hash").is_none(),
            "{args:?} resolves no recipe, so params_hash must be OMITTED: {id}"
        );
    }
}

#[test]
fn convert_report_echoes_a_declared_film_type_only() {
    // `--film-type` gates nothing; it is a provenance declaration, so the report must
    // carry it or the flag is accepted and dropped. The default (`unknown`) is omitted.
    let tmp = TempDir::new("filmtype");
    let out = tmp.path("out.tiff");
    let (code, stdout, err) =
        convert_p3(&fixture("hdr-48bit.tif"), &out, &["--film-type", "silver"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(json(&stdout)["film_type"], "silver", "{stdout}");

    let (code, stdout, err) = convert_p3(&fixture("hdr-48bit.tif"), &out, &[]);
    assert_eq!(code, 0, "{err}");
    assert!(json(&stdout).get("film_type").is_none(), "{stdout}");

    // A stated `unknown` is omitted by every command alike.
    let (code, stdout, err) =
        convert_p3(&fixture("hdr-48bit.tif"), &out, &["--film-type", "unknown"]);
    assert_eq!(code, 0, "{err}");
    assert!(json(&stdout).get("film_type").is_none(), "{stdout}");
    let scan = fixture("hdr-48bit.tif");
    let scan = scan.to_str().unwrap();
    for argv in [
        &["inspect", scan][..],
        &["measure-base", scan, "--base-region", "0,0,60,60"],
    ] {
        let command = argv[0];
        let (code, stdout, err) = run(&[argv, &["--film-type", "unknown"]].concat());
        assert_eq!(code, 0, "{command}: {err}");
        assert!(
            json(&stdout).get("film_type").is_none(),
            "{command}: {stdout}"
        );
    }
}

#[test]
fn params_hash_is_the_hash_of_the_dumped_recipe_bytes() {
    // The documented contract (`Recipe::params_hash`): the report's
    // `identity.params_hash` and the record's `conversion.params_hash` are FNV-1a-64
    // over exactly the bytes `--dump-params` writes, so either can be matched to a kept
    // recipe file.
    let tmp = TempDir::new("paramshash");
    let out = tmp.path("out.tiff");
    let dump = tmp.path("params.json");
    let rec = tmp.path("run.json");
    let (code, stdout, err) = convert_p3(
        &fixture("hdr-48bit.tif"),
        &out,
        &[
            "--dump-params",
            dump.to_str().unwrap(),
            "--telemetry-file",
            rec.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{err}");
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in &std::fs::read(&dump).unwrap() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    let record: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&rec).unwrap()).unwrap();
    assert_eq!(
        record["conversion"]["params_hash"],
        format!("{h:016x}"),
        "{record}"
    );
    assert_eq!(
        json(&stdout)["identity"]["params_hash"],
        format!("{h:016x}"),
        "{stdout}"
    );
}

#[test]
fn the_reports_recipe_replays_the_run() {
    // The report is the only record a run keeps of its recipe (no sidecar), so its
    // `recipe` must reload through `--params` to the same image and the same hash. A
    // non-default knob makes the echo carry something a default replay would miss.
    let tmp = TempDir::new("recipe-echo");
    let first = tmp.path("first.tiff");
    let (code, stdout, err) = convert_p3(&fixture("hdr-48bit.tif"), &first, &["--exposure", "0.5"]);
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);
    let recipe = tmp.path("echo.json");
    std::fs::write(&recipe, report["recipe"].to_string()).unwrap();

    let second = tmp.path("second.tiff");
    let (code, replay, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        second.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        std::fs::read(&first).unwrap(),
        std::fs::read(&second).unwrap()
    );
    let replay = json(&replay);
    assert_eq!(
        replay["identity"]["params_hash"], report["identity"]["params_hash"],
        "{replay}"
    );
    assert_eq!(replay["recipe"], report["recipe"]);
}

#[test]
fn recipe_dumped_by_this_build_replays_clean_under_strict() {
    // The documented reproducibility path is `--dump-params` → replay, and it must
    // survive `--strict`: nothing else checks the one file the tool itself writes.
    //
    // The IR-free fixture is required: `hdri-64bit.tif` emits the "IR preserved but
    // not used" warning on every frame, which would fail `--strict` here regardless. The
    // roll measurement is stated so the dump carries one; without it the replay warns
    // that it fell back, and `--strict` rightly fails on that.
    let tmp = TempDir::new("dumpreplay");
    let first = tmp.path("first.tiff");
    let dump = tmp.path("params.json");
    let (code, _, err) = convert_p3(
        &fixture("hdr-48bit.tif"),
        &first,
        &[&["--dump-params", dump.to_str().unwrap()][..], &MEASURED].concat(),
    );
    assert_eq!(code, 0, "{err}");

    let replay = tmp.path("replay.tiff");
    let (code, _, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        replay.to_str().unwrap(),
        "--params",
        dump.to_str().unwrap(),
        "--strict",
    ]);
    assert_eq!(
        code, 0,
        "a recipe this build just dumped must replay clean under --strict; stderr:\n{err}"
    );
    assert_eq!(
        std::fs::read(&first).unwrap(),
        std::fs::read(&replay).unwrap(),
        "the replay must be byte-identical"
    );
}

#[test]
fn version_flag_prints_the_full_build_identity() {
    // `nc --version` must be enough to attribute an output on its own.
    let (code, stdout, _) = run(&["--version"]);
    assert_eq!(code, 0);
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")), "{stdout}");
    assert!(stdout.contains("pipeline_version:"), "{stdout}");
    assert!(stdout.contains("commit:"), "{stdout}");
    assert!(stdout.contains("target:"), "{stdout}");
}

#[test]
fn identity_fields_are_not_recipe_keys() {
    // The whole reason for the envelope: identity must NEVER be a recipe key. Each
    // one, placed bare in a versioned recipe, is a loud unknown-key usage error (exit
    // 2) —
    // if any of these silently deserialized, `deny_unknown_fields` would have been
    // weakened and future sidecars would smuggle provenance into the config.
    let tmp = TempDir::new("not-keys");
    for key in [
        r#""nc_version": "0.1.0""#,
        r#""pipeline_version": 1"#,
        r#""params_hash": "0000000000000000""#,
        r#""git_commit": "abc123""#,
        r#""identity": {}"#,
    ] {
        let recipe = write_file(
            &tmp.path("r.json"),
            &format!(r#"{{ "recipe_version": 3, {key} }}"#),
        );
        let out = tmp.path("out.tiff");
        let (code, _, err) = run(&[
            "convert",
            fixture("hdri-64bit.tif").to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--params",
            recipe.to_str().unwrap(),
            "--report",
            "none",
        ]);
        assert_eq!(code, 2, "bare identity key {key} must be rejected: {err}");
        assert!(err.contains("unknown field"), "{key}: {err}");
    }
}

#[test]
fn meta_without_params_is_a_pointed_usage_error() {
    // A half-written envelope is a malformed envelope, not a bare recipe: it gets a
    // pointed message instead of the opaque `unknown field 'meta'` serde default.
    let tmp = TempDir::new("half-envelope");
    let recipe = write_file(
        &tmp.path("r.json"),
        r#"{ "meta": { "pipeline_version": 1 } }"#,
    );
    let out = tmp.path("out.tiff");
    let (code, _, err) = run(&[
        "convert",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
        "--report",
        "none",
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("`meta` block but no `params`"),
        "the error must name the envelope shape: {err}"
    );
}

#[test]
fn unknown_meta_fields_are_ignored_but_the_recipe_body_is_still_strict() {
    // `meta` is provenance, so an OLDER build must tolerate a NEWER build's extra
    // meta fields (forward compatibility) — while the `params` body keeps its full
    // `deny_unknown_fields` strictness.
    let tmp = TempDir::new("meta-fwd");
    let out = tmp.path("out.tiff");
    let ok = write_file(
        &tmp.path("ok.json"),
        r#"{ "meta": { "invented_future_field": [1, 2], "pipeline_version": 8 },
             "params": { "recipe_version": 3, "scene_correction": { "exposure": 0.25 } } }"#,
    );
    let (code, _, err) = run(&[
        "convert",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--params",
        ok.to_str().unwrap(),
        "--report",
        "none",
    ]);
    assert_eq!(code, 0, "unknown meta fields must be ignored:\n{err}");

    // Same flags as the accepted case above, so the ONLY difference is the typo —
    // otherwise the exit code could be blamed on the missing `--film-base`.
    let bad = write_file(
        &tmp.path("bad.json"),
        r#"{ "meta": {}, "params": { "recipe_version": 3, "scene_correction": { "exposur": 0.25 } } }"#,
    );
    let (code, _, err) = run(&[
        "convert",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "-o",
        tmp.path("bad.tiff").to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--params",
        bad.to_str().unwrap(),
        "--report",
        "none",
    ]);
    assert_eq!(code, 2, "a typo inside `params` must still be loud: {err}");
    assert!(
        err.contains("exposur"),
        "the error must name the offending key, not just fail: {err}"
    );

    // `params` itself is the envelope discriminator, so a `params` key *inside* the
    // recipe body is an unknown recipe key — pinning that a future stage section
    // can't quietly claim the name and turn every recipe into an envelope.
    let nested = write_file(
        &tmp.path("nested.json"),
        r#"{ "meta": {}, "params": { "recipe_version": 3, "params": {} } }"#,
    );
    let (code, _, err) = run(&[
        "convert",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "-o",
        tmp.path("nested.tiff").to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--params",
        nested.to_str().unwrap(),
        "--report",
        "none",
    ]);
    assert_eq!(code, 2, "`params` must not be a recipe key: {err}");
    assert!(err.contains("unknown field `params`"), "{err}");
}

#[test]
fn a_non_object_recipe_body_is_refused_instead_of_converting_with_defaults() {
    // serde's derived visitor accepts a *sequence* for a struct and every recipe
    // field has a default, so both of these used to convert with ALL-DEFAULT
    // parameters at exit 0, advertising a params_hash byte-identical to the default
    // recipe's — a truncated or mis-generated sidecar silently ignoring the recipe
    // the operator believes is applied.
    let tmp = TempDir::new("non-object");
    for (tag, body) in [
        ("params-array", r#"{ "params": [] }"#),
        ("bare-array", "[]"),
        ("params-number", r#"{ "params": 3 }"#),
    ] {
        let recipe = write_file(&tmp.path(&format!("{tag}.json")), body);
        let (code, _, err) = run(&[
            "convert",
            fixture("hdri-64bit.tif").to_str().unwrap(),
            "-o",
            tmp.path(&format!("{tag}.tiff")).to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
            "--params",
            recipe.to_str().unwrap(),
            "--report",
            "none",
        ]);
        assert_eq!(code, 2, "{tag} must be refused: {err}");
        assert!(err.contains("must be a"), "{tag}: {err}");
    }
}

#[test]
fn an_unreadable_meta_pipeline_version_is_loud_not_silently_ignored() {
    // Mapped to `None`, an unreadable value is indistinguishable from an absent one
    // and disables the skew check entirely; truncated with `as u32`, 4294967297
    // becomes 1 and *matches* a build at pipeline_version 1, suppressing the warning
    // by pretending to agree with it. Both are the silent replay this label exists
    // to prevent.
    let tmp = TempDir::new("bad-meta-version");
    for (tag, value) in [
        ("float", "1.0"),
        ("string", "\"1\""),
        ("negative", "-1"),
        ("null", "null"),
        ("overflow", "4294967297"),
    ] {
        let recipe = write_file(
            &tmp.path(&format!("{tag}.json")),
            &format!(
                r#"{{ "meta": {{ "pipeline_version": {value} }},
                      "params": {{ "recipe_version": 3, "calibration": {{ "film_base": {{ "explicit": [0.9, 0.55, 0.42] }} }} }} }}"#
            ),
        );
        let (code, _, err) = run(&[
            "convert",
            fixture("hdri-64bit.tif").to_str().unwrap(),
            "-o",
            tmp.path(&format!("{tag}.tiff")).to_str().unwrap(),
            "--params",
            recipe.to_str().unwrap(),
            "--report",
            "none",
        ]);
        assert_eq!(
            code, 2,
            "meta.pipeline_version {value} must be refused: {err}"
        );
        assert!(err.contains("meta.pipeline_version"), "{tag}: {err}");
    }
}

#[test]
fn a_malformed_meta_container_is_refused_like_a_malformed_field() {
    // The container/field asymmetry: a corrupt *field* inside `meta` was already a
    // loud exit 2, but a corrupt `meta` *block* degraded to "records no version" and
    // replayed with no skew check at all — silently reproducing the very mismatch the
    // label exists to surface.
    let tmp = TempDir::new("bad-meta-container");
    for (tag, meta) in [
        ("null", "null"),
        ("string", "\"x\""),
        ("array", "[]"),
        ("number", "123"),
    ] {
        let recipe = write_file(
            &tmp.path(&format!("{tag}.json")),
            &format!(
                r#"{{ "meta": {meta},
                      "params": {{ "recipe_version": 3, "calibration": {{ "film_base": {{ "explicit": [0.9, 0.55, 0.42] }} }} }} }}"#
            ),
        );
        let (code, _, err) = run(&[
            "convert",
            fixture("hdri-64bit.tif").to_str().unwrap(),
            "-o",
            tmp.path(&format!("{tag}.tiff")).to_str().unwrap(),
            "--params",
            recipe.to_str().unwrap(),
            "--report",
            "none",
        ]);
        assert_eq!(code, 2, "meta={meta} must be refused: {err}");
        assert!(err.contains("`meta` must be an object"), "{tag}: {err}");
    }
}

#[test]
fn output_stats_report_the_written_samples_for_both_depths() {
    // `output_stats.mean` is the entire cross-version comparison basis — `nctool
    // compare` hard-fails without it — so its presence and shape are a contract, not
    // an implementation detail.
    let tmp = TempDir::new("output-stats");
    let out = tmp.path("u16.tiff");
    let (code, stdout, err) = convert_p3(&fixture("hdri-64bit.tif"), &out, &[]);
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);
    let mean = report["output_stats"]["mean"]
        .as_array()
        .unwrap_or_else(|| panic!("output_stats.mean must be present: {report}"));
    assert_eq!(mean.len(), 3, "one mean per channel: {mean:?}");
    for v in mean {
        let v = v.as_f64().expect("a finite number");
        assert!(
            v.is_finite() && (0.0..=1.0).contains(&v),
            "a u16 mean is the quantized value normalized into [0,1], got {v}"
        );
    }

    // f32 output is written verbatim, so the mean is reported in *that* domain
    // (unclamped) — the reason `nctool compare` records the depth beside the mean.
    let master = tmp.path("master.tiff");
    let (code, stdout, err) = run(&[
        "convert",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "-o",
        master.to_str().unwrap(),
        "--film-master",
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);
    assert_eq!(
        report["output_stats"]["mean"].as_array().map(Vec::len),
        Some(3),
        "output_stats must be reported for an f32 output too: {report}"
    );

    // A blown-out render ties the two report fields together: the clamped samples
    // the mean is taken over are the same ones `loss` counts. The display tone
    // overshoots display white by design, so its loss reaches the u16 encode.
    let clipped = tmp.path("clipped.tiff");
    let (code, stdout, err) = convert_p3(
        &fixture("hdri-64bit.tif"),
        &clipped,
        &["--exposure", "40.0"],
    );
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);
    assert!(
        report["loss"]["clipped_high"].as_u64().unwrap_or(0) > 0,
        "a heavily over-exposed print must clip high: {report}"
    );
    let mean = report["output_stats"]["mean"][0].as_f64().unwrap();
    assert!(
        mean > 0.9,
        "the mean of the CLAMPED written samples must sit near display white, got {mean}"
    );
}

#[test]
fn roll_frames_carry_their_own_identity_and_comparison_basis() {
    // A per-frame override genuinely changes THAT frame's effective recipe, so the
    // difference has to be visible per frame or the docs' claim that a roll is
    // comparable is empty.
    let tmp = TempDir::new("roll-frame-identity");
    let recipe = write_file(&tmp.path("roll.json"), ROLL_RECIPE);
    let hdr = fixture("hdr-48bit.tif");
    let hdri = fixture("hdri-64bit.tif");
    let manifest = write_file(
        &tmp.path("frames.json"),
        &format!(
            r#"{{ "frames": [
                 {{ "input": {hdr:?} }},
                 {{ "input": {hdri:?}, "params": {{ "scene_correction": {{ "exposure": 0.5 }} }} }}
               ] }}"#,
            hdr = hdr.to_str().unwrap(),
            hdri = hdri.to_str().unwrap(),
        ),
    );
    let out_dir = tmp.path("out");
    let (code, stdout, err) = run(&[
        "roll",
        "--frames",
        manifest.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);

    let by_stem = |stem: &str| -> serde_json::Value {
        report["frames"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["input"].as_str().unwrap().contains(stem))
            .unwrap_or_else(|| panic!("no frame for {stem} in {report}"))
            .clone()
    };
    let plain = by_stem("hdr-48bit");
    let overridden = by_stem("hdri-64bit");

    // Every ok frame carries a full identity plus its comparison basis.
    for (label, frame) in [("plain", &plain), ("overridden", &overridden)] {
        assert_eq!(
            frame["identity"]["nc_version"],
            env!("CARGO_PKG_VERSION"),
            "{label}: {frame}"
        );
        assert_eq!(
            frame["output_stats"]["mean"].as_array().map(Vec::len),
            Some(3),
            "{label} frame must carry output_stats: {frame}"
        );
    }

    // The un-overridden frame ran the shared recipe's exposure; the overridden one
    // reports its own.
    let exposure = |frame: &serde_json::Value| {
        frame["chain"]["scene_correction"]["exposure"]
            .as_f64()
            .unwrap_or_else(|| panic!("no resolved exposure in {frame}"))
    };
    assert_eq!(exposure(&plain), 0.0);
    assert_eq!(exposure(&overridden), 0.5);
}

#[test]
fn roll_warns_about_a_version_skewed_shared_recipe() {
    // `roll` has its own skew wiring, distinct from `convert`'s: the mismatch is a
    // roll-level fact (one shared recipe, N frames), so it rides the roll's warnings
    // rather than any single frame's.
    let tmp = TempDir::new("roll-skew");
    let stale = write_file(
        &tmp.path("stale.json"),
        r#"{ "meta": { "pipeline_version": 9999 },
             "params": { "recipe_version": 3,
                         "calibration": { "film_base": { "explicit": [0.9, 0.55, 0.42] } } } }"#,
    );
    let out_dir = tmp.path("out");
    let (code, stdout, err) = run(&[
        "roll",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        stale.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "the recipe still applies:\n{err}");
    let report = json(&stdout);
    assert!(
        report["warnings"]
            .to_string()
            .contains("pipeline_version 9999"),
        "the roll-level warnings must carry the skew: {report}"
    );

    // And `--strict` promotes it, after the report lands.
    let (code, stdout, _) = run(&[
        "roll",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "--out-dir",
        tmp.path("strict").to_str().unwrap(),
        "--params",
        stale.to_str().unwrap(),
        "--strict",
    ]);
    assert_eq!(
        code, 1,
        "--strict must promote the roll's version-skew warning"
    );
    assert_eq!(json(&stdout)["command"], "roll", "the report still lands");
}

#[test]
fn replaying_another_pipeline_versions_recipe_warns_and_strict_promotes_it() {
    // A recipe captured under a different behavioral pipeline_version still
    // applies, but its default render has changed underneath it — the loud,
    // `--strict`-promotable warning is the whole point of the version label.
    let tmp = TempDir::new("version-skew");
    let stale = write_file(
        &tmp.path("stale.json"),
        r#"{ "meta": { "pipeline_version": 9999 },
             "params": { "recipe_version": 3,
                         "calibration": { "film_base": { "explicit": [0.9, 0.55, 0.42] } } } }"#,
    );
    let out = tmp.path("out.tiff");
    let (code, stdout, err) = run(&[
        "convert",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--params",
        stale.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "the recipe still applies:\n{err}");
    let warnings = json(&stdout)["warnings"].to_string();
    assert!(
        warnings.contains("pipeline_version 9999"),
        "the version skew must be reported: {warnings}"
    );

    // Same recipe under --strict ⇒ non-zero exit, report still emitted.
    let (code, stdout, _) = run(&[
        "convert",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "-o",
        tmp.path("strict.tiff").to_str().unwrap(),
        "--params",
        stale.to_str().unwrap(),
        "--strict",
    ]);
    assert_eq!(code, 1, "--strict must promote the version-skew warning");
    assert_eq!(
        json(&stdout)["command"],
        "convert",
        "the report still lands"
    );

    // A recipe recording THIS build's version does not warn.
    let current = json(
        &run(&[
            "convert",
            fixture("hdri-64bit.tif").to_str().unwrap(),
            "-o",
            tmp.path("cur.tiff").to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
        ])
        .1,
    )["identity"]["pipeline_version"]
        .as_u64()
        .unwrap();
    let matching = write_file(
        &tmp.path("matching.json"),
        &format!(
            r#"{{ "meta": {{ "pipeline_version": {current} }},
                  "params": {{ "recipe_version": 3, "calibration": {{ "film_base": {{ "explicit": [0.9, 0.55, 0.42] }} }} }} }}"#
        ),
    );
    // No `--strict` here: this HDRi fixture legitimately warns about its unconsumed
    // IR plane, so a strict exit would prove nothing about the version label. The
    // assertion is on the warning *text* — no version-skew warning appears.
    let (code, stdout, _) = run(&[
        "convert",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "-o",
        tmp.path("match.tiff").to_str().unwrap(),
        "--params",
        matching.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "a matching pipeline_version must convert cleanly");
    assert!(
        !json(&stdout)["warnings"]
            .to_string()
            .contains("pipeline_version"),
        "a matching pipeline_version must not warn"
    );
}

#[test]
fn identity_stamping_does_not_perturb_the_output_pixels() {
    // Identity is operational metadata in the same class as `--report`/telemetry:
    // it must never move a pixel. Drive the same conversion through every path that
    // touches the identity code — report on/off, a bare recipe, an enveloped
    // recipe carrying a `meta` block, and a version-skew warning — and assert one
    // single set of TIFF bytes across all of them.
    let tmp = TempDir::new("no-perturb");
    let input = fixture("hdri-64bit.tif");
    let base = tmp.path("base.tiff");
    let dump = tmp.path("bare.json");
    let (code, _, err) = convert_p3(
        &input,
        &base,
        &["--dump-params", dump.to_str().unwrap(), "--report", "none"],
    );
    assert_eq!(code, 0, "{err}");
    let expected = std::fs::read(&base).unwrap();

    let bare = std::fs::read_to_string(&dump).unwrap();
    let skewed = write_file(
        &tmp.path("skew.json"),
        &format!(r#"{{ "meta": {{ "pipeline_version": 9999 }}, "params": {bare} }}"#),
    );
    let envelope = write_file(
        &tmp.path("envelope.json"),
        &format!(
            r#"{{ "meta": {{ "pipeline_version": {} }}, "params": {bare} }}"#,
            pipeline_version_from_version_flag()
        ),
    );
    let variants: [(&str, Vec<&str>); 4] = [
        ("report json", vec!["--params", dump.to_str().unwrap()]),
        (
            "report none",
            vec!["--params", dump.to_str().unwrap(), "--report", "none"],
        ),
        (
            "enveloped recipe",
            vec!["--params", envelope.to_str().unwrap()],
        ),
        ("version skew", vec!["--params", skewed.to_str().unwrap()]),
    ];
    for (label, extra) in variants {
        let out = tmp.path(&format!("{}.tiff", label.replace(' ', "-")));
        let mut args = vec![
            "convert",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ];
        args.extend_from_slice(&extra);
        let (code, _, err) = run(&args);
        assert_eq!(code, 0, "{label}: {err}");
        assert_eq!(
            std::fs::read(&out).unwrap(),
            expected,
            "{label} must produce byte-identical pixels"
        );
    }
}

// ---------------------------------------------------------------------------
// Memory preflight (`io/memory-preflight`)
// ---------------------------------------------------------------------------

#[test]
fn memory_preflight_reports_the_estimate_and_budget_decision() {
    // Every command that decodes reports what the preflight decided, with the
    // per-phase breakdown behind the number the gate compared.
    let tmp = TempDir::new("mem-report");
    let out = tmp.path("out.tiff");
    let (code, stdout, _err) = run(&[
        "convert",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    assert_eq!(code, 0);
    let mem = json(&stdout)["memory"].clone();
    assert_eq!(mem["budget_source"], "default");
    assert_eq!(mem["budget_bytes"], 6u64 * 1024 * 1024 * 1024);
    assert_eq!(mem["decision"], "ok");
    let peak = mem["estimated_peak_bytes"].as_u64().unwrap();
    let accounted = mem["accounted_bytes"].as_u64().unwrap();
    assert!(peak > accounted, "the estimate includes the allowance");
    // The full-pipeline profile sizes all four phases. Which one peaks is per profile
    // (`memory`'s `which_phase_peaks_is_per_profile_and_measured_not_assumed`): for the
    // 16-bit TIFF profile it is the encode, where the quantize buffer joins the
    // rendition, with the render below it. The film-base phase — one image, since an
    // explicit `--film-base` samples nothing — is below both.
    assert!(mem["decode_bytes"].as_u64().unwrap() > 0);
    assert_eq!(accounted, mem["encode_bytes"].as_u64().unwrap());
    assert!(mem["render_bytes"].as_u64().unwrap() < accounted, "{mem}");
    let film_base = mem["film_base_bytes"].as_u64().unwrap();
    assert!(
        film_base > 0 && film_base < accounted,
        "film-base phase must be sized and below the encode peak: {mem}"
    );

    // `inspect` gates on the decode-only profile — no render, no encode. It gathers
    // no film-base sample, so its peak is the decode phase (the read buffer beside
    // the image).
    let (code, stdout, _err) = run(&["inspect", fixture("hdri-64bit.tif").to_str().unwrap()]);
    assert_eq!(code, 0);
    let mem = json(&stdout)["memory"].clone();
    assert_eq!(mem["render_bytes"], 0);
    assert_eq!(mem["encode_bytes"], 0);
    assert_eq!(mem["accounted_bytes"], mem["decode_bytes"], "{mem}");

    // `estimate` reports the same block on the same profile — and its sampling plan
    // reaches the model rather than being a constant: the film-base term scales
    // with the rectangle actually sampled.
    let (code, stdout, err) = run(&[
        "measure-base",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "--base-region",
        "0,0,60,60",
    ]);
    assert_eq!(code, 0, "{err}");
    let small = json(&stdout)["memory"].clone();
    assert_eq!(small["budget_source"], "default");
    assert_eq!(small["decision"], "ok");
    assert_eq!(small["render_bytes"], 0);
    assert_eq!(small["encode_bytes"], 0);
    assert_eq!(small["accounted_bytes"], small["decode_bytes"]);

    // With no source flag, `estimate` counts the effective area into a fixed-size
    // histogram rather than gathering it, so it is charged no sample at all.
    let (code, stdout, err) = run(&["measure-base", fixture("hdr-48bit.tif").to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    let area = json(&stdout)["memory"].clone();
    assert!(
        area["film_base_bytes"].as_u64().unwrap() < small["film_base_bytes"].as_u64().unwrap(),
        "the area measurement must cost less than a gathered 60x60 rectangle:\n{area}\n{small}"
    );
}

#[test]
fn over_budget_convert_is_rejected_before_decoding_with_exit_six() {
    // The gate must fire *before* the pipeline allocates or writes anything: exit
    // 6 (resource), a message naming both numbers, and no output file left behind.
    let tmp = TempDir::new("mem-reject");
    let out = tmp.path("out.tiff");
    let (code, stdout, err) = run(&[
        "convert",
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--max-memory",
        "1MiB",
    ]);
    assert_eq!(code, 6, "over-budget must exit 6:\n{stdout}\n{err}");
    assert!(err.contains("resource:"), "{err}");
    assert!(err.contains("--max-memory"), "{err}");
    assert!(err.contains("1.0 MiB"), "message names the budget:\n{err}");
    assert!(
        err.contains("estimated peak"),
        "message names the estimate:\n{err}"
    );
    assert!(!out.exists(), "no output image may be written");
    assert!(
        stdout.is_empty(),
        "a rejected run emits no report:\n{stdout}"
    );
}

/// A **header-only** classic TIFF: IFD0 advertises `width`x`height` 16-bit RGB in
/// one strip, and no strip data is written at all. `probe` reads tags only, so it
/// reports the advertised shape; anything that actually decodes fails. The file is
/// ~130 bytes whatever the advertised dimensions, which is what makes an
/// "oversized input" test fast and portable.
fn write_header_only_rgb16_tiff(path: &std::path::Path, width: u32, height: u32) {
    const SHORT: u16 = 3;
    const LONG: u16 = 4;
    // 9 entries: dimensions, bits/sample, compression, photometric, strip offsets,
    // samples/pixel, rows/strip, strip byte counts (ascending tag order).
    let ifd_end = 8 + 2 + 9 * 12 + 4; // IFD0 starts at 8
    let bits_offset = ifd_end as u32; // [16, 16, 16] doesn't fit in 4 bytes
    let data_offset = bits_offset + 6;

    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(b"II"); // little-endian
    b.extend_from_slice(&42u16.to_le_bytes());
    b.extend_from_slice(&8u32.to_le_bytes()); // offset of IFD0
    b.extend_from_slice(&9u16.to_le_bytes()); // entry count
    let entry = |tag: u16, ty: u16, count: u32, value: u32, b: &mut Vec<u8>| {
        b.extend_from_slice(&tag.to_le_bytes());
        b.extend_from_slice(&ty.to_le_bytes());
        b.extend_from_slice(&count.to_le_bytes());
        // Little-endian: a SHORT value sits in the low two bytes of the field.
        b.extend_from_slice(&value.to_le_bytes());
    };
    entry(256, LONG, 1, width, &mut b); // ImageWidth
    entry(257, LONG, 1, height, &mut b); // ImageLength
    entry(258, SHORT, 3, bits_offset, &mut b); // BitsPerSample
    entry(259, SHORT, 1, 1, &mut b); // Compression = none
    entry(262, SHORT, 1, 2, &mut b); // PhotometricInterpretation = RGB
    entry(273, LONG, 1, data_offset, &mut b); // StripOffsets
    entry(277, SHORT, 1, 3, &mut b); // SamplesPerPixel
    entry(278, LONG, 1, height, &mut b); // RowsPerStrip (one strip)
    entry(279, LONG, 1, 6, &mut b); // StripByteCounts (deliberately short)
    b.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    assert_eq!(b.len(), ifd_end);
    b.extend_from_slice(
        &[
            16u16.to_le_bytes(),
            16u16.to_le_bytes(),
            16u16.to_le_bytes(),
        ]
        .concat(),
    );
    std::fs::write(path, &b).unwrap();
}

#[test]
fn an_oversized_header_is_rejected_while_the_heap_is_still_empty() {
    // The central claim of `io/memory-preflight`: the gate runs *before* the large
    // allocation. A header-only TIFF advertising 100000x100000 RGB16 (a 30 GB
    // convert peak) with no pixel data is what discriminates the two orderings:
    // `probe` reads tags only and succeeds, so a preflight *before* decode rejects
    // it with the resource error (exit 6) having allocated nothing, whereas a gate
    // placed after decode would have to try the read first and would surface a
    // decode/limits error (exit 3) instead — or OOM.
    let tmp = TempDir::new("mem-header-only");
    let input = tmp.path("oversized.tif");
    write_header_only_rgb16_tiff(&input, 100_000, 100_000);
    assert!(
        std::fs::metadata(&input).unwrap().len() < 1024,
        "the oversized input must stay a tiny file"
    );
    let out = tmp.path("out.tiff");

    let (code, stdout, err) = run(&[
        "convert",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    assert_eq!(
        code, 6,
        "an oversized header must be rejected as a resource error, not decoded:\n{stdout}\n{err}"
    );
    assert!(err.contains("resource:"), "{err}");
    assert!(err.contains("100000x100000"), "{err}");
    assert!(!out.exists(), "nothing may be written");
    assert!(
        stdout.is_empty(),
        "a rejected run emits no report:\n{stdout}"
    );

    // Same file, same command, with a budget large enough to admit the estimate:
    // now the run gets as far as the decode, which fails on the absent pixel data.
    // That is the proof the exit 6 above came from the preflight rather than from
    // the file being unreadable.
    let (code, _stdout, err) = run(&[
        "convert",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--max-memory",
        "512GiB",
    ]);
    assert_ne!(
        code, 6,
        "with room in the budget the gate must not fire:\n{err}"
    );
    assert!(!out.exists());
}

#[test]
fn roll_reports_the_preflight_decision_per_frame() {
    // Frames can differ in dimensions (so in estimated peak) under one shared
    // budget, and the gate runs per frame — so the decision is reported per frame,
    // not once for the roll.
    let tmp = TempDir::new("mem-roll-report");
    let recipe = write_file(&tmp.path("roll.json"), ROLL_RECIPE);
    let out_dir = tmp.path("out");
    let (code, stdout, err) = run(&[
        "roll",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
        "--max-memory",
        "2GiB",
    ]);
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);
    let frames = report["frames"].as_array().expect("frames array");
    assert_eq!(frames.len(), 2);
    for frame in frames {
        let mem = &frame["memory"];
        assert_eq!(mem["budget_source"], "flag");
        assert_eq!(mem["budget_bytes"], 2u64 * 1024 * 1024 * 1024);
        assert_eq!(mem["decision"], "ok");
        assert!(mem["estimated_peak_bytes"].as_u64().unwrap() > 0);
    }
    // The HDRi frame carries an IR plane, so it must estimate above the HDR one.
    let hdr = frames[0]["memory"]["estimated_peak_bytes"]
        .as_u64()
        .unwrap();
    let hdri = frames[1]["memory"]["estimated_peak_bytes"]
        .as_u64()
        .unwrap();
    assert!(
        hdri > hdr,
        "the IR-carrying frame must estimate higher ({hdri} vs {hdr})"
    );
}

#[test]
fn roll_gates_each_frame_against_the_shared_budget() {
    // The gate is per frame, not per roll: a budget between the two frames'
    // estimates must convert the smaller one and fail only its sibling — with the
    // sibling's resource error in its own frame entry, and the roll still exiting
    // non-zero.
    let tmp = TempDir::new("mem-roll-mixed");
    let recipe = write_file(&tmp.path("roll.json"), ROLL_RECIPE);
    let hdr_in = fixture("hdr-48bit.tif");
    let hdri_in = fixture("hdri-64bit.tif");

    // Read both estimates from a roll that fits, rather than hardcoding fixture
    // arithmetic that would rot with the model.
    let probe_dir = tmp.path("probe");
    let (code, stdout, err) = run(&[
        "roll",
        hdr_in.to_str().unwrap(),
        hdri_in.to_str().unwrap(),
        "--out-dir",
        probe_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    let frames = json(&stdout)["frames"].as_array().unwrap().clone();
    let small = frames[0]["memory"]["estimated_peak_bytes"]
        .as_u64()
        .unwrap();
    let large = frames[1]["memory"]["estimated_peak_bytes"]
        .as_u64()
        .unwrap();
    assert!(small < large);
    let between = ((small + large) / 2).to_string();

    let out_dir = tmp.path("out");
    let (code, stdout, err) = run(&[
        "roll",
        hdr_in.to_str().unwrap(),
        hdri_in.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
        "--max-memory",
        &between,
    ]);
    assert_eq!(code, 1, "the over-budget frame must fail the roll:\n{err}");
    let report = json(&stdout);
    assert_eq!(report["summary"]["succeeded"], 1);
    assert_eq!(report["summary"]["failed"], 1);
    let frames = report["frames"].as_array().unwrap();
    assert_eq!(frames[0]["status"], "ok");
    assert_eq!(frames[1]["status"], "failed");
    let error = frames[1]["error"].as_str().unwrap();
    assert!(
        error.contains("resource:") && error.contains("estimated peak"),
        "the failed frame must carry its own resource error: {error}"
    );
    // The frame that fitted was written; its sibling was not.
    assert!(
        out_dir.join("hdr-48bit_positive.tiff").exists(),
        "the in-budget frame must still be converted"
    );
    assert!(
        !out_dir.join("hdri-64bit_positive.tiff").exists(),
        "the over-budget frame must write nothing"
    );
}

#[test]
fn over_budget_rejection_covers_inspect_estimate_and_roll() {
    // All four decoding commands are gated, each on its own profile.
    let tmp = TempDir::new("mem-reject-all");
    let input = fixture("hdri-64bit.tif");
    let in_str = input.to_str().unwrap();

    let (code, _out, err) = run(&["inspect", in_str, "--max-memory", "1KiB"]);
    assert_eq!(code, 6, "inspect must be gated too:\n{err}");
    let (code, _out, err) = run(&["measure-base", in_str, "--max-memory", "1KiB"]);
    assert_eq!(code, 6, "estimate must be gated too:\n{err}");

    let recipe = write_file(&tmp.path("roll.json"), ROLL_RECIPE);
    let out_dir = tmp.path("out");
    let (code, _out, err) = run(&[
        "roll",
        in_str,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
        "--max-memory",
        "1KiB",
    ]);
    // Roll gates per frame; a frame that fails the preflight fails the roll (its
    // own exit code is the roll-level "frames failed" error, not exit 6).
    assert_ne!(code, 0, "an over-budget frame must fail the roll:\n{err}");
    assert!(
        err.contains("resource:") || err.contains("estimated peak"),
        "the frame's resource error must surface:\n{err}"
    );
}

#[test]
fn decode_only_commands_pass_a_budget_that_rejects_the_full_pipeline() {
    // The per-profile gate is not cosmetic: a budget between the decode-only and
    // full-pipeline estimates must admit `inspect` while rejecting `convert`.
    let tmp = TempDir::new("mem-profile");
    let input = fixture("hdri-64bit.tif");
    let in_str = input.to_str().unwrap();

    // Read the two estimates from the reports themselves rather than hardcoding
    // fixture-size arithmetic that would rot with the model.
    let (_c, stdout, _e) = run(&["inspect", in_str]);
    let decode_only = json(&stdout)["memory"]["estimated_peak_bytes"]
        .as_u64()
        .unwrap();
    let out = tmp.path("out.tiff");
    let (_c, stdout, _e) = run(&[
        "convert",
        in_str,
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    let full = json(&stdout)["memory"]["estimated_peak_bytes"]
        .as_u64()
        .unwrap();
    assert!(
        decode_only < full,
        "decode-only ({decode_only}) must estimate below the full pipeline ({full})"
    );

    // A budget in between: inspect proceeds, convert is rejected.
    let between = (decode_only + full) / 2;
    let budget = between.to_string();
    let (code, _out, err) = run(&["inspect", in_str, "--max-memory", &budget]);
    assert_eq!(code, 0, "inspect fits the in-between budget:\n{err}");
    let out2 = tmp.path("out2.tiff");
    let (code, _out, err) = run(&[
        "convert",
        in_str,
        "-o",
        out2.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--max-memory",
        &budget,
    ]);
    assert_eq!(code, 6, "convert exceeds the in-between budget:\n{err}");
    assert!(!out2.exists());
}

#[test]
fn max_memory_is_operational_not_a_recipe_key() {
    // Like `--report`/`--strict`/`--telemetry`: the budget must not enter the
    // recipe, must not appear in the dumped recipe, and must not change a single
    // output byte. A recipe *carrying* the key must be rejected (`deny_unknown_fields`).
    let tmp = TempDir::new("mem-not-recipe");
    let input = fixture("hdri-64bit.tif");
    let in_str = input.to_str().unwrap();

    let plain = tmp.path("plain.tiff");
    let budgeted = tmp.path("budgeted.tiff");
    let dump = tmp.path("budgeted.json");
    for (out, extra) in [
        (&plain, Vec::new()),
        (
            &budgeted,
            vec![
                "--max-memory",
                "3GiB",
                "--dump-params",
                dump.to_str().unwrap(),
            ],
        ),
    ] {
        let mut args = vec![
            "convert",
            in_str,
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
        ];
        args.extend_from_slice(&extra);
        let (code, _out, err) = run(&args);
        assert_eq!(code, 0, "{err}");
    }
    assert_eq!(
        std::fs::read(&plain).unwrap(),
        std::fs::read(&budgeted).unwrap(),
        "--max-memory must not perturb the output image"
    );
    let dumped = std::fs::read_to_string(&dump).unwrap();
    // Only the key itself: a bare `contains("memory")` over the whole recipe would
    // fail on any future recipe key that merely has the substring in its name.
    assert!(
        !dumped.contains("max_memory"),
        "the budget must not appear in the effective recipe:\n{dumped}"
    );

    // …and it is not accepted as a recipe key.
    let recipe = write_file(
        &tmp.path("bad.json"),
        r#"{"recipe_version": 3, "max_memory": 4294967296,
            "calibration": {"film_base": {"explicit": [0.9, 0.55, 0.42]}}}"#,
    );
    let out = tmp.path("nope.tiff");
    let (code, _stdout, err) = run(&[
        "convert",
        in_str,
        "-o",
        out.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "an unknown recipe key is a usage error:\n{err}");
    assert!(err.contains("max_memory"), "{err}");
    assert!(!out.exists());
}

#[test]
fn malformed_max_memory_is_a_usage_error() {
    let tmp = TempDir::new("mem-bad-flag");
    let out = tmp.path("out.tiff");
    for bad in ["0", "lots", "4.5GiB", "12PiB"] {
        let (code, _stdout, err) = run(&[
            "convert",
            fixture("hdri-64bit.tif").to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
            "--max-memory",
            bad,
        ]);
        assert_eq!(
            code, 2,
            "--max-memory {bad:?} must be a usage error:\n{err}"
        );
        assert!(!out.exists());
    }
}

#[test]
fn convert_requires_a_stated_film_base_but_estimate_does_not() {
    // The contract this PR introduces, end to end at the binary boundary.
    let tmp = TempDir::new("stated-base");
    let out = tmp.path("out.tif");
    let scan = fixture("hdr-48bit.tif");

    // convert with no base: usage error (exit 2), before anything is written.
    let (code, _stdout, err) = run(&[
        "convert",
        scan.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(
        code, 2,
        "an unstated film base must be a usage error: {err}"
    );
    assert!(err.contains("no film base selected"), "stderr: {err}");
    assert!(
        !out.exists(),
        "nothing may be written on the fast-fail path"
    );

    // The same run with a stated base gets past the gate; an explicit base isolates
    // the gate under test.
    let (code, _stdout, err) = run(&[
        "convert",
        scan.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.6,0.5",
    ]);
    assert_eq!(code, 0, "a stated base must convert: {err}");
    assert!(out.exists());

    // `estimate` exists to *produce* a base, so it must not require one —
    // otherwise the documented "measure once, reuse" workflow is circular. With no
    // source it measures the frame's effective area.
    let (code, _stdout, err) = run(&["measure-base", scan.to_str().unwrap()]);
    assert_ne!(
        code, 2,
        "estimate must not demand a base it is being asked to measure: {err}"
    );
    assert!(
        !err.contains("no film base selected"),
        "estimate must not emit the convert-only requirement: {err}"
    );
}

#[test]
fn roll_requires_a_stated_film_base_and_every_remedy_it_names_works() {
    // `roll` converts, so it must state a base too, and it takes `convert`'s film-base
    // flags — so the diagnosis is `convert`'s, and each way out it names must run.
    let tmp = TempDir::new("roll-stated-base");
    let out_dir = tmp.path("out");
    let scan = fixture("hdr-48bit.tif");

    let (code, _stdout, err) = run(&[
        "roll",
        scan.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
    ]);
    assert_eq!(
        code, 2,
        "roll with no stated film base must be a usage error: {err}"
    );
    assert!(err.contains("no film base selected"), "stderr: {err}");
    for remedy in ["--film-base", "--base-region", "calibration.film_base"] {
        assert!(err.contains(remedy), "{remedy} is not offered: {err}");
    }
    assert!(
        !err.contains("--auto-base"),
        "a removed flag is offered: {err}"
    );
    assert!(
        !out_dir.exists(),
        "nothing may be written on the fast-fail path"
    );

    // Each remedy gets past the gate and converts: the flag…
    let (code, stdout, err) = run(&[
        "roll",
        scan.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--film-base",
        "0.9,0.6,0.5",
    ]);
    assert_eq!(code, 0, "--film-base must convert:\n{stdout}\n{err}");
    std::fs::remove_dir_all(&out_dir).unwrap();
    // …the region, which converts with its not-frozen warning…
    let (code, stdout, err) = run(&[
        "roll",
        scan.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--base-region",
        "0,0,40,40",
    ]);
    assert_eq!(code, 0, "--base-region must convert:\n{stdout}\n{err}");
    assert!(err.contains("NOT frozen"), "{err}");
    std::fs::remove_dir_all(&out_dir).unwrap();
    // …and a recipe carrying `calibration.film_base`.
    let recipe = write_file(
        &tmp.path("roll.json"),
        r#"{"recipe_version":3,"calibration": {"film_base": {"explicit": [0.9, 0.6, 0.5]}}}"#,
    );
    let (code, stdout, err) = run(&[
        "roll",
        scan.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "a stated base must convert:\n{stdout}\n{err}");
    assert!(out_dir.join("hdr-48bit_positive.tiff").exists());
}

#[test]
fn roll_reports_the_specific_problem_before_the_missing_base() {
    // Ordering, not just correctness: `validate`'s own policy is
    // least-specific-diagnosis-last, and "no film base selected" is the least
    // specific diagnosis there is. A recipe that is *both* baseless and
    // roll-invalid must name the roll-invalid setting, or the user adds a base
    // only to be told about a second, unrelated problem.
    let tmp = TempDir::new("roll-order");
    let out_dir = tmp.path("out");
    let recipe = tmp.path("recipe.json");
    // Baseless AND colorimetric — two independent reasons to refuse.
    std::fs::write(
        &recipe,
        r#"{"recipe_version":3,"input":{"meaning":"colorimetric"}}"#,
    )
    .unwrap();
    let (code, _stdout, err) = run(&[
        "roll",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 4, "the colorimetric rejection must win: {err}");
    assert!(
        !err.contains("no film base selected"),
        "the least-specific diagnosis must not pre-empt the specific one: {err}"
    );
}

#[test]
fn a_suffix_mismatch_outranks_the_missing_base() {
    // Same least-specific-diagnosis-last policy as the roll gate: the output
    // path's suffix is a property of *this invocation*, while "no film base
    // selected" is the least specific diagnosis available. A run that is wrong
    // both ways must name the suffix, or the user supplies a base only to be
    // told the path was never going to work.
    let tmp = TempDir::new("suffix-order");
    let bad = tmp.path("out.jpg");
    let (code, _stdout, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        bad.to_str().unwrap(),
        "--transfer",
        "pq",
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("does not end in .tif or .tiff"),
        "the suffix rule must win: {err}"
    );
    assert!(
        !err.contains("no film base selected"),
        "the least-specific diagnosis must not pre-empt it: {err}"
    );
    assert!(!bad.exists());
}

#[test]
fn the_display_tone_headroom_reaches_the_pixels() {
    let tmp = TempDir::new("reinhard-tone");
    let scan = fixture("hdr-48bit.tif");
    let render = |tag: &str, extra: &[&str]| {
        let out = tmp.path(&format!("{tag}.tiff"));
        let mut argv = vec![
            "convert",
            scan.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.6,0.5",
        ];
        argv.extend_from_slice(extra);
        let (code, _stdout, err) = run(&argv);
        assert_eq!(code, 0, "{tag}: {err}");
        std::fs::read(&out).unwrap()
    };

    // The headroom reaches the operator, and naming the default is naming nothing.
    let default = render("default", &[]);
    assert_eq!(default, render("w64", &["--display-tone-headroom", "6"]));
    let w16 = render("w16", &["--display-tone-headroom", "4"]);
    assert_ne!(default, w16, "the headroom did not reach the render");
}

#[test]
fn the_retired_display_tone_flags_are_refused_with_a_migration_error() {
    // Removed-value errors, not clap's parse failure listing the old names. (Their
    // recipe key went with the removed chain's recipe, which is refused whole.)
    let tmp = TempDir::new("display-tone-removed");
    let scan = fixture("hdr-48bit.tif");
    for extra in [
        &["--display-tone", "shoulder"][..],
        &["--display-tone", "none"],
        &["--display-tone", "reinhard"],
        &["--highlight-compress", "0.5"],
    ] {
        let out = tmp.path("out.tif");
        let mut argv = vec![
            "convert",
            scan.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.6,0.5",
        ];
        argv.extend_from_slice(extra);
        let (code, _stdout, err) = run(&argv);
        assert_eq!(code, 2, "{extra:?}: {err}");
        assert!(err.contains("was removed"), "{extra:?}: {err}");
        assert!(err.contains("--display-tone-headroom"), "{extra:?}: {err}");
        assert!(!err.contains("possible values"), "{extra:?}: {err}");
        assert!(!out.exists(), "{extra:?}: a refused run wrote a file");
    }
}

#[test]
fn the_display_tone_headroom_is_gated_before_anything_is_opened() {
    // The headroom bound is a **value** rule, so it runs with the recipe's validation —
    // not in the stage, which runs after the decode. Proof: a nonexistent input still
    // exits 2 (usage), never 3 (decode), and `--dump-params` writes nothing.
    let tmp = TempDir::new("reinhard-gate");
    let missing = tmp.path("does-not-exist.tif");
    let dumped = tmp.path("dumped.json");
    //
    // Each case asserts the **rule's own wording**, not just exit 2: clap also exits 2,
    // so a bare code check cannot tell a parse error from the validation rule. That
    // mattered here — `-1` was refused by clap as an "unexpected argument" until
    // `--display-tone-headroom` gained `allow_hyphen_values`, leaving the rule's
    // negative branch unreachable from the flag whose name its own message prints.
    let bad = [
        (
            vec!["--display-tone-headroom", "30"],
            "beyond the supported maximum",
        ),
        (
            vec!["--display-tone-headroom", "-1"],
            "must be finite and non-negative",
        ),
    ];
    for (extra, expected) in bad {
        let out = tmp.path("out.tiff");
        let mut argv = vec![
            "convert",
            missing.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.6,0.5",
            "--dump-params",
            dumped.to_str().unwrap(),
        ];
        argv.extend(extra.iter().copied());
        let (code, _stdout, err) = run(&argv);
        assert_eq!(code, 2, "{extra:?} reached the decoder: {err}");
        assert!(err.contains(expected), "{extra:?}: {err}");
        assert!(
            err.contains("--display-tone-headroom"),
            "{extra:?}: clap answered for the validation rule: {err}"
        );
        assert!(
            !dumped.exists(),
            "{extra:?}: an invalid recipe was written to disk before failing"
        );
    }
    // Falsifiable control: the same invocation with a usable headroom gets past
    // validation and fails at the *decode* instead.
    let out = tmp.path("out.tiff");
    let (code, _stdout, err) = run(&[
        "convert",
        missing.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.6,0.5",
        "--display-tone-headroom",
        "6",
    ]);
    assert_eq!(code, 3, "a valid headroom must reach the decoder: {err}");
}

#[test]
fn roll_refuses_an_out_of_range_headroom_in_the_shared_recipe() {
    // `roll` never calls `validate_convert`, so a rule that lives only there — or only
    // in the stage — is no gate at all for it: every frame decoded and reconstructed
    // before failing, once per frame. The shared recipe is validated up front, so this
    // must exit 2 with nothing written.
    let tmp = TempDir::new("roll-headroom");
    let recipe = write_file(
        &tmp.path("roll.json"),
        r#"{
  "recipe_version": 3,
  "calibration": {
    "film_base": { "explicit": [0.9, 0.55, 0.42] }
  },
  "fit_range": { "headroom_stops": 60.0 }
}"#,
    );
    let out_dir = tmp.path("out");
    let (code, _stdout, err) = run(&[
        "roll",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "the shared recipe must be refused up front: {err}");
    assert!(err.contains("fit_range.headroom_stops"), "{err}");
    assert!(!out_dir.exists(), "a frame was written before refusing");
}

#[test]
fn the_gain_map_jpeg_rejects_a_non_jpeg_suffix_and_rolls_with_a_jpg_name() {
    let tmp = TempDir::new("gain-map-refusals");
    let output = tmp.path("out.tiff");
    let (code, _stdout, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--range",
        "hdr",
        "--film-base",
        "1,1,1",
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains(".jpg"), "{err}");
    assert!(!output.exists());

    // Roll runs it and derives `<stem>_positive.jpg`. Roll takes its destination from
    // the shared recipe, since it accepts no destination flag.
    let out_dir = tmp.path("out");
    std::fs::create_dir_all(&out_dir).unwrap();
    let recipe = write_file(
        &tmp.path("roll.json"),
        r#"{"recipe_version":3,"output":{"display":{"range":"hdr"}},
            "calibration":{"film_base":{"explicit":[1,1,1]}}}"#,
    );
    let (code, _stdout, err) = run(&[
        "roll",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    let rolled = out_dir.join("hdr-48bit_positive.jpg");
    assert!(
        rolled.exists(),
        "roll must derive `.jpg` for a JPEG container"
    );
    // And the rolled frame really is the dual-dialect file, not a renamed TIFF.
    let bytes = std::fs::read(&rolled).unwrap();
    assert_eq!(&bytes[..2], &[0xff, 0xd8]);
    assert!(
        bytes
            .windows(b"urn:iso:std:iso:ts:21496:-1".len())
            .any(|w| w == b"urn:iso:std:iso:ts:21496:-1")
    );
}

#[test]
fn roll_checks_explicit_manifest_suffixes_and_derives_per_frame_names() {
    // The manifest path goes through the same suffix rule `convert` uses, so a frame
    // cannot be pointed at a container its destination cannot write.
    let tmp = TempDir::new("roll-container-naming");
    let out_dir = tmp.path("out");
    std::fs::create_dir_all(&out_dir).unwrap();
    let input = fixture("hdr-48bit.tif");

    let manifest = |body: &str| -> PathBuf { write_file(&tmp.path("frames.json"), body) };
    let roll = |frames: &Path| {
        run(&[
            "roll",
            "--frames",
            frames.to_str().unwrap(),
            "--out-dir",
            out_dir.to_str().unwrap(),
            "--params",
            tmp.path("shared.json").to_str().unwrap(),
        ])
    };
    write_file(
        &tmp.path("shared.json"),
        r#"{"recipe_version":3,"output":{"display":{"range":"hdr"}},
            "calibration":{"film_base":{"explicit":[1,1,1]}}}"#,
    );

    // An explicit path whose suffix contradicts the resolved container fails up
    // front, naming the frame and the destination.
    let bad = manifest(&format!(
        r#"{{"frames":[{{"input":"{}","output":"frame.tiff"}}]}}"#,
        input.display()
    ));
    let (code, _stdout, err) = roll(&bad);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("jpeg"), "{err}");
    assert!(err.contains(".jpg"), "{err}");
    assert!(err.contains("frame"), "{err}");

    // A matching explicit path is accepted verbatim and never renamed.
    let good = manifest(&format!(
        r#"{{"frames":[{{"input":"{}","output":"chosen.jpeg"}}]}}"#,
        input.display()
    ));
    let (code, _stdout, err) = roll(&good);
    assert_eq!(code, 0, "{err}");
    assert!(out_dir.join("chosen.jpeg").exists(), "{err}");

    // A per-frame `output` override changes that frame's container, so its *derived*
    // name must follow the frame's destination rather than the roll's. The override
    // still warns loudly (different image class).
    let mixed = manifest(&format!(
        r#"{{"frames":[
             {{"input":"{0}"}},
             {{"input":"{0}","output":"as-master.tiff","params":{{"output":"film-master"}}}}
           ]}}"#,
        input.display()
    ));
    let (code, stdout, err) = roll(&mixed);
    assert_eq!(code, 0, "{err}");
    assert!(out_dir.join("hdr-48bit_positive.jpg").exists(), "{err}");
    assert!(out_dir.join("as-master.tiff").exists(), "{err}");
    let report = json(&stdout);
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap_or_default().contains("resolves `output`")),
        "a per-frame destination override must still warn: {report}"
    );

    // An explicit path stating **no** suffix is completed from the frame's own
    // resolved container, exactly as a `convert` path is — the manifest shares the
    // whole rule, not just its refusing half.
    let bare = manifest(&format!(
        r#"{{"frames":[{{"input":"{}","output":"stem-only"}}]}}"#,
        input.display()
    ));
    let (code, stdout, err) = roll(&bare);
    assert_eq!(code, 0, "{err}");
    assert!(out_dir.join("stem-only.jpg").exists(), "{err}");
    assert!(!out_dir.join("stem-only").exists(), "{err}");
    let report = json(&stdout);
    assert_eq!(
        report["frames"][0]["output"],
        out_dir.join("stem-only.jpg").to_str().unwrap(),
        "the roll report must name the completed path: {report}"
    );
}

#[test]
fn a_bare_output_stem_takes_the_destinations_container_and_everything_names_it() {
    // `-o out` does not have to know the container. Asserted per container, and on
    // everything that derives from the path — the file written, the report, and what
    // is left absent.
    let tmp = TempDir::new("bare-output-stem");
    let input = fixture("hdr-48bit.tif");
    for (name, destination, ext, container) in [
        // The no-flag case: this is a test *about* the default.
        ("default", &[][..], "tiff", "tiff"),
        ("gain-map", &["--range", "hdr"][..], "jpg", "jpeg"),
        ("film-master", &["--film-master"][..], "tiff", "tiff"),
    ] {
        let stem = tmp.path(name);
        let mut argv: Vec<&str> = vec![
            "convert",
            input.to_str().unwrap(),
            "-o",
            stem.to_str().unwrap(),
            "--film-base",
            "1,1,1",
        ];
        argv.extend_from_slice(destination);
        let (code, stdout, err) = run(&argv);
        assert_eq!(code, 0, "{name}: {err}");
        let written = PathBuf::from(format!("{}.{ext}", stem.display()));
        assert!(written.exists(), "{name}: {} missing", written.display());
        assert!(
            !stem.exists(),
            "{name}: the stem itself must not be written"
        );
        assert!(
            !sidecar_of(&written).exists() && !sidecar_of(&stem).exists(),
            "{name}: no sidecar is written"
        );
        // The name is only half of it: the bytes must be the container the name
        // claims. Nothing in the type system couples the path's container to the
        // encoder the destination dispatches to, so this is the coupling.
        assert_eq!(
            sniff_container(&written),
            container,
            "{name}: {} is named .{ext} but its bytes are not {container}",
            written.display()
        );
        let report = json(&stdout);
        assert_eq!(
            report["output"],
            written.to_str().unwrap(),
            "{name}: the report must name what was written"
        );
    }
}

#[test]
fn an_output_path_naming_a_directory_is_refused_not_completed_to_a_sibling() {
    // `-o positives/` is the muscle-memory mistake — roll's sibling flag is spelled
    // `--out-dir positives/`. `Path::file_name()` normalises the trailing separator
    // away, so completing it would write `positives.tiff` *next to* the directory.
    // Refused at exit 2 instead; no byte that decides *which file* is named may be
    // altered or dropped.
    //
    // Both spellings, because they are one hole: `dir/.` escapes a trailing-separator
    // test but `file_name()` normalises its `.` away just the same.
    let tmp = TempDir::new("names-a-directory");
    let input = fixture("hdr-48bit.tif");
    let dir = tmp.path("dir");
    std::fs::create_dir_all(&dir).unwrap();
    for given in [
        format!("{}/", dir.display()),
        format!("{}/.", dir.display()),
    ] {
        let (code, _stdout, err) = run(&[
            "convert",
            input.to_str().unwrap(),
            "-o",
            &given,
            "--film-base",
            "1,1,1",
        ]);
        assert_eq!(code, 2, "{given}: {err}");
        assert!(err.contains("names a directory"), "{given}: {err}");
        assert!(
            !tmp.path("dir.tiff").exists(),
            "{given}: a sibling of the directory was written: {err}"
        );
    }

    // A stated suffix does not make a directory path a file: refused before the render,
    // not at the write (exit 5) — and ahead of the removed-container refusal, whose
    // remedy would otherwise lead straight to `out.tiff/`.
    for name in ["out.tiff", "out.avif"] {
        let given = format!("{}/", tmp.path(name).display());
        let (code, _stdout, err) = run(&[
            "convert",
            input.to_str().unwrap(),
            "-o",
            &given,
            "--film-base",
            "1,1,1",
        ]);
        assert_eq!(code, 2, "{given}: {err}");
        assert!(err.contains("names a directory"), "{given}: {err}");
        assert!(!err.contains("AVIF"), "{given}: {err}");
    }

    // Falsifiable control: the same path without the separator is a stem and works.
    let stem = tmp.path("stem");
    let (code, _stdout, err) = run(&[
        "convert",
        input.to_str().unwrap(),
        "-o",
        stem.to_str().unwrap(),
        "--film-base",
        "1,1,1",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(tmp.path("stem.tiff").exists(), "{err}");
}

#[test]
fn a_stated_suffix_survives_verbatim_and_a_dotted_stem_keeps_its_dot() {
    // The two halves completion must not disturb: a spelling the container accepts
    // is never normalised, and a dot-segment no container claims is a stem.
    let tmp = TempDir::new("stated-suffix");
    let input = fixture("hdr-48bit.tif");
    let convert = |given: &Path| -> (i32, String, String) {
        run(&[
            "convert",
            input.to_str().unwrap(),
            "-o",
            given.to_str().unwrap(),
            "--film-base",
            "1,1,1",
        ])
    };

    // `.tif` is the non-canonical spelling: nc writes `.tiff` when it chooses, and
    // must not rewrite the user's choice to match.
    let stated = tmp.path("stated.tif");
    let (code, stdout, err) = convert(&stated);
    assert_eq!(code, 0, "{err}");
    assert!(stated.exists(), "{err}");
    assert!(
        !tmp.path("stated.tiff").exists(),
        "the spelling was rewritten"
    );
    assert_eq!(json(&stdout)["output"], stated.to_str().unwrap());

    // `.v2` is not a container nc knows, so the whole thing is the stem.
    let dotted = tmp.path("scan.v2");
    let (code, stdout, err) = convert(&dotted);
    assert_eq!(code, 0, "{err}");
    let written = tmp.path("scan.v2.tiff");
    assert!(written.exists(), "{err}");
    assert!(!tmp.path("scan.tiff").exists(), "the stem's dot was eaten");
    assert_eq!(json(&stdout)["output"], written.to_str().unwrap());

    // And a *known* spelling the container refuses is a usage error naming the
    // destination — completion never rescues a stated suffix.
    let (code, _stdout, err) = convert(&tmp.path("wrong.jpg"));
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--container tiff"), "{err}");
}

#[test]
fn the_write_target_guard_sees_the_completed_path() {
    // Ordering: the path is resolved *before* the collision check, so a
    // `--report-file` that only clashes once the suffix is completed is still
    // caught. Resolving after the guard would let the report be overwritten by the
    // image (or vice versa) with every check green.
    let tmp = TempDir::new("completed-target-guard");
    let input = fixture("hdr-48bit.tif");
    let stem = tmp.path("clash");
    let convert = |report_file: &Path| -> (i32, String, String) {
        run(&[
            "convert",
            input.to_str().unwrap(),
            "-o",
            stem.to_str().unwrap(),
            "--film-base",
            "1,1,1",
            "--report-file",
            report_file.to_str().unwrap(),
        ])
    };
    let (code, _stdout, err) = convert(&tmp.path("clash.tiff"));
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--report-file"), "{err}");
    // Falsifiable: the *stem* is not a write target, so pointing the report there
    // is fine — the guard is reacting to the completed path, not to any overlap.
    let (code, _stdout, err) = convert(&stem);
    assert_eq!(code, 0, "{err}");
    assert!(tmp.path("clash.tiff").exists(), "{err}");
}

#[test]
fn the_default_output_is_an_sdr_display_p3_tiff() {
    // The default destination, asserted through the binary with no destination flag.
    let tmp = TempDir::new("default-destination");
    let out = tmp.path("positive.tiff");
    let (code, stdout, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "1,1,1",
    ]);
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);
    assert_eq!(
        report["chain"]["destination"],
        serde_json::json!({ "display": {
            "range": "sdr", "transfer": "native", "gamut": "display-p3", "container": "tiff"
        } })
    );
    assert_eq!(
        report["identity"]["pipeline_version"].as_u64(),
        Some(pipeline_version_from_version_flag())
    );
    assert_eq!(read_tiff_bits(&out), 16, "the default writes a 16-bit TIFF");

    // A `.jpg` path with no destination flag is a usage error, and the message names
    // the destination that writes one.
    let (code, _stdout, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        tmp.path("positive.jpg").to_str().unwrap(),
        "--film-base",
        "1,1,1",
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("--range hdr --container jpeg"),
        "the way out must be named: {err}"
    );
    assert!(!tmp.path("positive.jpg").exists());
}

#[test]
fn telemetry_reports_the_primary_containers_depth_not_the_ir_planes() {
    // The record's depth is the *primary* file's: a gain-map JPEG is 8-bit whatever
    // depth its optional IR TIFF is written at.
    let tmp = TempDir::new("telemetry-depth");
    let input = fixture("hdr-48bit.tif");
    for (name, destination, ext, want) in [
        ("gain-map", &["--range", "hdr"][..], "jpg", "u8"),
        ("pq-tiff", &["--transfer", "pq"][..], "tiff", "u16"),
        (
            "linear-tiff",
            &["--transfer", "linear", "--gamut", "bt2020"][..],
            "tiff",
            "f32",
        ),
        ("sdr", &[][..], "tiff", "u16"),
        ("film-master", &["--film-master"][..], "tiff", "f32"),
    ] {
        let out = tmp.path(&format!("{name}.{ext}"));
        let rec = tmp.path(&format!("{name}.telemetry.json"));
        let mut argv = vec![
            "convert",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "1,1,1",
            "--telemetry-file",
            rec.to_str().unwrap(),
        ];
        argv.extend_from_slice(destination);
        let (code, _stdout, err) = run(&argv);
        assert_eq!(code, 0, "{name}: {err}");
        let record: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&rec).unwrap()).unwrap();
        assert_eq!(
            record["conversion"]["output_depth"], want,
            "{name} must report its primary container's depth"
        );
        // The clip counts' denominator: a gain map examined two renditions.
        let image = &record["image"];
        let pixels = image["width"].as_u64().unwrap() * image["height"].as_u64().unwrap();
        let renditions = if name == "gain-map" { 2 } else { 1 };
        assert_eq!(
            record["outcome"]["total_samples"],
            renditions * pixels * 3,
            "{name}"
        );
    }
}

#[test]
fn the_headroom_changes_the_sdr_render_and_the_report_records_it() {
    // The knob's product claim: on a display destination it is a real pixel change,
    // and the report says which headroom produced them.
    let tmp = TempDir::new("display-tone");
    let scan = fixture("hdr-48bit.tif");
    let mut bytes = Vec::new();
    for stops in ["6", "0"] {
        let out = tmp.path(&format!("w{stops}.tiff"));
        let (code, stdout, err) = run(&[
            "convert",
            scan.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.6,0.5",
            "--exposure=-2.2",
            "--display-tone-headroom",
            stops,
        ]);
        assert_eq!(code, 0, "{stops}: {err}");
        let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(
            report["chain"]["fit_range"]["headroom_stops"],
            stops.parse::<f64>().unwrap(),
            "{stops}"
        );
        bytes.push(std::fs::read(&out).unwrap());
    }
    assert_ne!(
        bytes[0], bytes[1],
        "the headroom must change the rendered pixels"
    );
}

#[test]
fn the_coded_hdr_report_block_names_the_fit_range_operator() {
    // `hdr_coded_tiff` carries the rendition's tone identifier — fit range's operator
    // and black curve — so a consumer reading the contract block alone knows which
    // operator produced it.
    let tmp = TempDir::new("display-tone-coded");
    let scan = fixture("hdr-48bit.tif");
    let out = tmp.path("pq.tiff");
    let (code, stdout, err) = run(&[
        "convert",
        scan.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--transfer",
        "pq",
        "--film-base",
        "0.9,0.6,0.5",
    ]);
    assert_eq!(code, 0, "{err}");
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        report["hdr_coded_tiff"]["tone_curve"],
        "reinhard-peak-lifted-v1+log-shift-to-mid-grey-v1"
    );
}

#[test]
fn film_master_refuses_a_stated_headroom_and_accepts_the_reset() {
    // The film master runs no fit range, so a non-default headroom would be silently
    // ignored — refused by value. The default is the flags-win reset that lets one recipe
    // serve every destination, so it is accepted.
    let tmp = TempDir::new("master-headroom");
    let scan = fixture("hdr-48bit.tif");
    let run_master = |stops: &str| {
        let out = tmp.path(&format!("master-{stops}.tiff"));
        run(&[
            "convert",
            scan.to_str().unwrap(),
            "--film-base",
            "1,1,1",
            "--film-master",
            "--display-tone-headroom",
            stops,
            "-o",
            out.to_str().unwrap(),
        ])
    };
    let (code, _o, err) = run_master("3");
    assert_eq!(code, 2, "expected a usage error, got:\n{err}");
    assert!(err.contains("fit range"), "{err}");
    assert!(err.contains("no rendering stage"), "{err}");
    let (code, _o, err) = run_master("6");
    assert_eq!(
        code, 0,
        "the default headroom must reset, not refuse:\n{err}"
    );
}

/// IR-assisted holder detection is decided by measuring the IR plane, not by a
/// declared `--film-type` (`film-base/ir-usability-detection`). Chemistry is the
/// wrong predictor: separability tracks the *frame's* accumulated density, so an
/// unexposed silver frame separates ~20:1 while its own leader is opaque.
#[test]
fn ir_holder_detection_is_decided_by_measurement_not_by_declaration() {
    let dir = TempDir::new("ir-usability");

    // A frame whose film is IR-transparent — 0.63, where real chromogenic scans
    // measure (0.576-0.728 over 25 frames). No `--film-type` is passed.
    let clear = dir.path("clear.tif");
    write_hdri_with_uniform_ir(&clear, 200, 200, [20000, 12000, 8000], 41_000);
    let (code, stdout, _err) = run(&["inspect", clear.to_str().unwrap()]);
    assert_eq!(code, 0);
    let report = json(&stdout);
    assert_eq!(
        report["ir_separability"]["usable"], true,
        "an IR-transparent frame must measure usable undeclared: {report}"
    );
    assert!(
        report["effective_area"]["holder"].is_object(),
        "the holder must be measured with no --film-type: {report}"
    );

    // A declaration is echoed back rather than parsed and dropped: `inspect` and
    // `estimate` resolve no recipe, so the report is the only place a declaration
    // they were given can survive. Absent when not declared — not `null`.
    let (_, declared_out, _) = run(&[
        "inspect",
        "--film-type",
        "chromogenic",
        clear.to_str().unwrap(),
    ]);
    assert_eq!(json(&declared_out)["film_type"], "chromogenic");
    assert!(
        report.get("film_type").is_none(),
        "an undeclared run must omit the field, not report null: {report}"
    );
    let (_, est_out, _) = run(&[
        "measure-base",
        "--film-type",
        "silver",
        "--base-region",
        "20,20,40,40",
        clear.to_str().unwrap(),
    ]);
    let est = json(&est_out);
    assert_eq!(est["film_type"], "silver");
    assert_eq!(
        est["ir_separability"]["usable"], true,
        "the calibration command must carry the measurement, not just warn: {est}"
    );

    // The declaration is inert now: `silver` used to force this path off, and
    // `chromogenic` used to be the only way to turn it on. Both must produce the
    // same report as stating nothing.
    for declared in ["silver", "chromogenic"] {
        let (code, out, _err) = run(&["inspect", "--film-type", declared, clear.to_str().unwrap()]);
        assert_eq!(code, 0);
        let mut with_flag = json(&out);
        let mut without = report.clone();
        // Wall-clock legitimately differs run to run, and `film_type` is the
        // declaration itself echoed back as provenance. Everything else — the
        // verdict, the measured holder — must be identical: the declaration is
        // recorded, and it decides nothing.
        for k in ["elapsed_ms", "film_type"] {
            with_flag[k] = serde_json::Value::Null;
            without[k] = serde_json::Value::Null;
        }
        assert_eq!(
            with_flag, without,
            "--film-type {declared} must change nothing but its own echo"
        );
    }

    // A frame whose own film is IR-opaque — the Ilford HP5 leader, interior median
    // 0.0165. Holder and film are indistinguishable, so the plane is refused and
    // the holder is not measured rather than the film labelled holder.
    let opaque = dir.path("opaque.tif");
    write_hdri_with_uniform_ir(&opaque, 200, 200, [20000, 12000, 8000], 1_081);
    let (code, stdout, _err) = run(&["inspect", opaque.to_str().unwrap()]);
    assert_eq!(code, 0);
    let report = json(&stdout);
    assert_eq!(report["ir_separability"]["usable"], false);
    assert!(
        report["effective_area"]["holder"].is_null(),
        "an opaque IR plane must measure no holder: {report}"
    );
    let warnings = report["warnings"].as_array().unwrap();
    assert!(
        warnings.iter().any(|w| w
            .as_str()
            .unwrap()
            .contains("cannot separate the film holder")
            && w.as_str().unwrap().contains("0.016")),
        "the fallback must name the measurement that caused it: {warnings:?}"
    );
}

/// `estimate` with no source flag measures an unexposed frame over its effective
/// area at the median (`film-base/holder-masked-measurement`): the IR-measured holder
/// is cut per edge, and the holder's own pixels contribute nothing.
#[test]
fn estimate_measures_an_unexposed_frame_over_its_effective_area() {
    let dir = TempDir::new("estimate-area");
    let dark = dir.path("dark-holder.tif");
    let bright = dir.path("bright-holder.tif");
    write_hdri_unexposed(&dark, 41_000, [655, 655, 655]);
    write_hdri_unexposed(&bright, 41_000, [65535, 65535, 65535]);

    let (code, stdout, err) = run(&["measure-base", "--strict", dark.to_str().unwrap()]);
    assert_eq!(
        code, 0,
        "a clean unexposed frame must pass --strict:\n{err}"
    );
    let report = json(&stdout);
    assert_eq!(report["film_base_source"], "effective_area", "{report}");
    assert_eq!(report["film_base_percentile"], 0.5, "{report}");
    let area = &report["effective_area"];
    assert_eq!(area["holder_applied"], true, "{report}");
    assert!(
        area["holder"]["left"].as_u64().unwrap() > area["holder"]["right"].as_u64().unwrap(),
        "the holder is cut per edge, not to the worst edge everywhere: {report}"
    );
    assert!(report["film_base_flag"].is_string(), "{report}");

    // The holder's RGB is extreme on the other side, and the base does not move.
    let (code, stdout, err) = run(&["measure-base", bright.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(json(&stdout)["film_base"], report["film_base"]);

    // Film too IR-opaque to tell from the holder: the holder is not measured, the
    // area is the inset alone, and the report says so — a warning `--strict` fails.
    let opaque = dir.path("opaque.tif");
    write_hdri_unexposed(&opaque, 1_081, [655, 655, 655]);
    let (code, stdout, _err) = run(&["measure-base", "--strict", opaque.to_str().unwrap()]);
    assert_eq!(code, 1);
    let report = json(&stdout);
    assert!(report["effective_area"]["holder"].is_null(), "{report}");
    assert!(
        report["warnings"].as_array().unwrap().iter().any(|w| {
            let w = w.as_str().unwrap();
            w.contains("the film holder was not measured")
                && w.contains("cannot separate the holder")
                && w.contains("--measure-inset")
        }),
        "{report}"
    );
}

/// No conversion reads the IR plane: a stated base reads no holder, and the
/// effective area reaches no pixel. So `--strict` fails on the carried plane unless
/// `--export-ir` takes it.
#[test]
fn a_conversion_never_consumes_the_ir_plane() {
    let dir = TempDir::new("ir-convert");
    let scan = dir.path("scan.tif");
    write_hdri_unexposed(&scan, 41_000, [655, 655, 655]);
    let convert = |extra: &[&str]| {
        run(&[
            &[
                "convert",
                "--film-base",
                "0.53,0.26,0.16",
                "--strict",
                // The synthetic fixture carries no SilverFast XMP, so state the input
                // semantics the provenance gate would otherwise resolve from it.
                "--input-transfer",
                "linear",
                "--input-meaning",
                "scanner-device",
                scan.to_str().unwrap(),
                "-o",
                dir.path("out.tif").to_str().unwrap(),
            ][..],
            &MEASURED,
            extra,
        ]
        .concat())
    };
    let (code, stdout, _err) = convert(&[]);
    assert_eq!(code, 1, "an unconsumed IR plane must fail --strict");
    let report = json(&stdout);
    assert_eq!(report["effective_area"]["holder_applied"], true, "{report}");
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("preserved but not used")),
        "{report}"
    );

    let (code, _stdout, err) = convert(&["--export-ir", dir.path("ir.tif").to_str().unwrap()]);
    assert_eq!(
        code, 0,
        "--strict --export-ir must stay usable on an HDRi scan:\n{err}"
    );
}

/// The removed film-base paths exit 2 with a remedy the command accepts, never
/// clap's or serde's generic error.
#[test]
fn the_retired_film_base_paths_name_the_measurement_that_replaced_them() {
    let dir = TempDir::new("retired-base");
    let fix = fixture("hdr-48bit.tif");
    let out = dir.path("out.tif");

    let (code, _o, err) = run(&[
        "convert",
        "--auto-base",
        fix.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--auto-base was removed"), "{err}");
    assert!(
        err.contains("hanten measure-base <unexposed-frame>"),
        "{err}"
    );
    assert!(err.contains("--film-base R,G,B"), "{err}");

    for flag in ["--auto-base", "--grid"] {
        let (code, _o, err) = run(&["measure-base", flag, fix.to_str().unwrap()]);
        assert_eq!(code, 2, "{flag}: {err}");
        assert!(err.contains(&format!("{flag} was removed")), "{err}");
        assert!(err.contains("Drop the flag"), "{err}");
    }

    // A recipe reaches `roll` too, which has no flags: the remedy is a recipe value.
    let recipe = dir.path("auto.json");
    std::fs::write(
        &recipe,
        r#"{"recipe_version": 3, "calibration": {"film_base": "auto"}}"#,
    )
    .unwrap();
    for argv in [
        vec![
            "convert",
            "--params",
            recipe.to_str().unwrap(),
            fix.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ],
        vec![
            "roll",
            "--params",
            recipe.to_str().unwrap(),
            "-o",
            dir.path("roll").to_str().unwrap(),
            fix.to_str().unwrap(),
        ],
    ] {
        let (code, _o, err) = run(&argv);
        assert_eq!(code, 2, "{argv:?}: {err}");
        assert!(err.contains("`calibration.film_base` \"auto\""), "{err}");
        assert!(err.contains(r#"{"explicit": [r, g, b]}"#), "{err}");
        assert!(!err.contains("unknown variant"), "{err}");
    }
}

/// `inspect` measures the holder ring even where every along-edge segment reads
/// holder at the edge — the normal case — and a plane the march read is not
/// reported as unused.
#[test]
fn inspect_measures_a_holder_ring_and_counts_the_plane_as_read() {
    let dir = TempDir::new("ir-all-holder");
    let path = dir.path("ringed.tif");
    const W: u32 = 200;
    const H: u32 = 200;
    let mut rgb = vec![0u16; (W * H * 3) as usize];
    let mut ir = vec![41_000u16; (W * H) as usize]; // film: IR-transparent
    for y in 0..H {
        for x in 0..W {
            let i = ((y * W + x) * 3) as usize;
            let holder = x < 6 || y < 6 || x >= W - 6 || y >= H - 6;
            rgb[i..i + 3].copy_from_slice(&if holder {
                [655, 655, 655]
            } else {
                [12000, 7000, 4000]
            });
            if holder {
                ir[(y * W + x) as usize] = 1_300; // holder: IR-dark, all four edges
            }
        }
    }
    write_hdri(&path, W, H, &rgb, &ir);

    let (code, stdout, _err) = run(&["inspect", path.to_str().unwrap()]);
    assert_eq!(code, 0);
    let report = json(&stdout);
    assert_eq!(
        report["ir_separability"]["usable"], true,
        "the frame's film is IR-transparent, so the verdict must be usable: {report}"
    );
    let area = &report["effective_area"];
    assert_eq!(
        area["holder_applied"], true,
        "the march must measure the ring: {report}"
    );
    assert!(
        area["holder"]["left"].as_u64().unwrap() > 0,
        "and report a real depth for it: {report}"
    );
    assert!(
        report["warnings"].as_array().is_none_or(|ws| ws
            .iter()
            .all(|w| !w.as_str().unwrap().contains("preserved but not used"))),
        "a plane the march read must not be reported as unused: {report}"
    );
}

/// The measurement region is resolved and reported on **every** `convert`, so
/// `--measure-inset` is observable rather than accepted-and-ignored — and the flag
/// moves the reported rectangle without moving a pixel, because nothing under the
/// default anchor consumes the region (`film-base/holder-depth-mask` review).
#[test]
fn convert_always_reports_the_measurement_region_and_the_inset_flag_moves_it() {
    let dir = TempDir::new("measure-inset-convert");
    let (a, b) = (dir.path("a.tif"), dir.path("b.tif"));

    let base = ["--film-base", "0.9,0.6,0.5"];
    let mut args = vec!["convert"];
    args.extend(base);
    args.extend(["-o", a.to_str().unwrap(), "tests/fixtures/hdr-48bit.tif"]);
    let (code, stdout, _) = run(&args);
    assert_eq!(code, 0);
    let default_area = json(&stdout)["effective_area"].clone();
    assert!(
        !default_area.is_null(),
        "a bare convert must report the area it resolved: {default_area}"
    );

    let mut args = vec!["convert", "--measure-inset", "0.2"];
    args.extend(base);
    args.extend(["-o", b.to_str().unwrap(), "tests/fixtures/hdr-48bit.tif"]);
    let (code, stdout, _) = run(&args);
    assert_eq!(code, 0);
    let wider = json(&stdout)["effective_area"].clone();
    assert_ne!(
        wider["inset"], default_area["inset"],
        "the flag must reach the resolved region: {wider} vs {default_area}"
    );

    // And nothing consumed it, so the pixels are untouched — observability is not
    // a pixel change.
    assert_eq!(
        std::fs::read(&a).unwrap(),
        std::fs::read(&b).unwrap(),
        "an unconsumed region must not move a pixel"
    );
}

/// `--strict` must keep failing on the "IR preserved but not used" note when the
/// plane never reaches a pixel — even when the effective-area march *measured* a
/// holder with it. Nothing in a `convert` render reads the region (its one consumer,
/// the auto reference density, retired), so a marched holder is reported, never used.
#[test]
fn strict_still_fails_when_the_ir_marched_region_reaches_no_pixel() {
    let dir = TempDir::new("ir-region-strict");
    let path = dir.path("ringed.tif");
    const W: u32 = 200;
    const H: u32 = 200;
    let mut rgb = vec![0u16; (W * H * 3) as usize];
    let mut ir = vec![41_000u16; (W * H) as usize];
    for y in 0..H {
        for x in 0..W {
            let i = ((y * W + x) * 3) as usize;
            let holder = x < 6 || y < 6 || x >= W - 6 || y >= H - 6;
            rgb[i..i + 3].copy_from_slice(&if holder {
                [655, 655, 655]
            } else {
                [12000, 7000, 4000]
            });
            if holder {
                ir[(y * W + x) as usize] = 1_300;
            }
        }
    }
    write_hdri(&path, W, H, &rgb, &ir);

    let out = dir.path("out.tif");
    let case = |extra: &[&str]| -> (i32, String) {
        let mut args = vec![
            "convert",
            "--film-base",
            "0.9,0.6,0.5",
            "--input-transfer",
            "linear",
            "--input-meaning",
            "scanner-device",
            // Under white, so no clipping warning of its own trips `--strict`: the IR
            // note is the only candidate.
            "--exposure=-3",
        ];
        args.extend(extra);
        args.extend(["-o", out.to_str().unwrap(), path.to_str().unwrap()]);
        let (code, _, err) = run(&args);
        (code, err)
    };

    // Falsifiable control: without `--strict` the same run succeeds, so the failure
    // below is the note being promoted and nothing else.
    let (code, err) = case(&[]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("preserved but not used"), "{err}");

    let (code, err) = case(&["--strict"]);
    assert_eq!(code, 1, "the note must still fail --strict: {err}");
    assert!(
        err.contains("preserved but not used"),
        "and for the right reason: {err}"
    );
}

/// An empty measurement region is a warning on `convert`, never a refusal
/// (`film-base/holder-depth-mask` ship review, M1): nothing in a conversion measures
/// over it since the auto reference density retired, so refusing would fail a run at
/// exit 2 over a measurement no stage read — while `inspect`/`estimate` degrade the
/// identical measurement to a warning at exit 0.
#[test]
fn an_empty_measurement_region_is_a_warning_not_a_refusal() {
    let dir = TempDir::new("empty-measure-region");
    let path = dir.path("ringed.tif");
    const W: u32 = 200;
    const H: u32 = 200;
    const RING: u32 = 30;
    let mut rgb = vec![0u16; (W * H * 3) as usize];
    let mut ir = vec![41_000u16; (W * H) as usize];
    for y in 0..H {
        for x in 0..W {
            let i = ((y * W + x) * 3) as usize;
            let holder = x < RING || y < RING || x >= W - RING || y >= H - RING;
            rgb[i..i + 3].copy_from_slice(&if holder {
                [655, 655, 655]
            } else {
                [12000, 7000, 4000]
            });
            if holder {
                ir[(y * W + x) as usize] = 1_300;
            }
        }
    }
    write_hdri(&path, W, H, &rgb, &ir);

    // 30 px of measured holder plus a 40% (80 px) inset on each side of a 200 px
    // frame leaves nothing.
    let convert_with = |extra: &[&str], out: &std::path::Path| -> (i32, String, String) {
        let mut args = vec![
            "convert",
            "--film-base",
            "0.9,0.6,0.5",
            "--input-transfer",
            "linear",
            "--input-meaning",
            "scanner-device",
            "--measure-inset",
            "0.4",
        ];
        args.extend(extra);
        args.extend(["-o", out.to_str().unwrap(), path.to_str().unwrap()]);
        run(&args)
    };

    // Nothing measures over the region: a warning, a written file, and no
    // `effective_area` — there is no region to report.
    let out = dir.path("unread.tif");
    let (code, stdout, err) = convert_with(&[], &out);
    assert_eq!(code, 0, "an unread region must not fail the run: {err}");
    assert!(out.exists(), "and the output must be written");

    // The reason the refusal was wrong: the output is byte-identical to the same
    // run with a region that resolves. If this ever differs, the empty region *is*
    // reaching the render and the refusal belongs back.
    let control = dir.path("control.tif");
    let (code, _, err) = run(&[
        "convert",
        "--film-base",
        "0.9,0.6,0.5",
        "--input-transfer",
        "linear",
        "--input-meaning",
        "scanner-device",
        "-o",
        control.to_str().unwrap(),
        path.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        std::fs::read(&out).unwrap(),
        std::fs::read(&control).unwrap(),
        "an unread empty region must not move a pixel"
    );
    let report = json(&stdout);
    assert!(
        report["effective_area"].is_null(),
        "a refused region has nothing to report: {report}"
    );
    let warnings = report["warnings"].as_array().unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().unwrap().contains("measurement region is empty")),
        "the warning is the observable: {warnings:?}"
    );
    // The measured depths and the inset must be separate quantities: the message
    // used to print the post-inset totals as "holder depths", sending a reader
    // after a 110 px holder that measured 30.
    let empty = warnings
        .iter()
        .find_map(|w| {
            let w = w.as_str().unwrap();
            w.contains("measurement region is empty").then_some(w)
        })
        .unwrap();
    assert!(
        empty.contains("measured holder depths (top 30, bottom 30, left 30, right 30)")
            && empty.contains("80 px inset"),
        "{empty}"
    );
    // And the remedy must be one that exists — `effective_area` never sees a
    // user-stated region, so "state a region explicitly" could not work.
    assert!(
        empty.contains("Lower the inset fraction") && !empty.contains("state a region"),
        "{empty}"
    );

    // A conversion measures nothing over the region — its one per-frame measurement,
    // an auto white balance, retired in favour of the roll's (`measure-roll`). So the
    // empty region is a warning, whatever the white balance, and `--auto-wb` is refused
    // by name before anything is decoded.
    let convert = |extra: &[&str], out: &std::path::Path| {
        let mut args = vec![
            "convert",
            "--film-base",
            "0.9,0.6,0.5",
            "--input-transfer",
            "linear",
            "--input-meaning",
            "scanner-device",
            "--measure-inset",
            "0.4",
        ];
        args.extend(extra);
        args.extend(["-o", out.to_str().unwrap(), path.to_str().unwrap()]);
        run(&args)
    };
    let (code, stdout, err) = convert(&["--white-balance", "1.1,1,0.9"], &dir.path("nf.tiff"));
    assert_eq!(code, 0, "stated gains read no region: {err}");
    assert!(json(&stdout)["effective_area"].is_null(), "{stdout}");
    let (code, _, err) = convert(&["--auto-wb", "gray-world"], &dir.path("nf-auto.tiff"));
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("--auto-wb") && err.contains("measure-roll"),
        "{err}"
    );

    // The inset's value bound is checked before the recipe resolves, and its remedy
    // must not send the user to a flag that is refused.
    let bound_out = dir.path("nf-bound.tiff");
    let (code, _, err) = run(&[
        "convert",
        "--film-base",
        "0.9,0.6,0.5",
        "--measure-inset",
        "0.5",
        "-o",
        bound_out.to_str().unwrap(),
        path.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("beyond the supported maximum")
            && err.contains("measure-roll")
            && !err.contains("--auto-wb"),
        "{err}"
    );

    // At an inset that leaves a region, nothing renders from the holder cut, so "IR
    // preserved but not used" holds whatever the white balance.
    let out = dir.path("nf-stated.tiff");
    let (code, stdout, err) = run(&[
        "convert",
        "--film-base",
        "0.9,0.6,0.5",
        "--input-transfer",
        "linear",
        "--input-meaning",
        "scanner-device",
        "--white-balance",
        "1.1,1,0.9",
        "-o",
        out.to_str().unwrap(),
        path.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);
    assert_eq!(report["effective_area"]["holder_applied"], true, "{report}");
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("preserved but not used")),
        "{report}"
    );
}

/// A capped or unsettled holder march warns, so `--strict` can see it
/// (`film-base/holder-depth-mask` ship review, M4).
///
/// `capped` and `converged` both mean "the reported rectangle is not a
/// measurement", which a `Serialize`-only field would leave unseen.
#[test]
fn a_capped_holder_march_warns_and_strict_promotes_it() {
    let dir = TempDir::new("capped-march-warns");
    // 400x400 → march cap 100. A 120 px IR-dark top band is beyond it, which also
    // inflates left/right from their true 10 px to the cap.
    let path = dir.path("deep.tif");
    const W: u32 = 400;
    const H: u32 = 400;
    // Dense, yet under the white point at `MEASURED`'s contrast (×1.11 on the
    // fallback slope), so a sub-cap run reports no clipping.
    const HOLDER: [u16; 3] = [2000, 2000, 2000];
    let mut rgb = vec![0u16; (W * H * 3) as usize];
    let mut ir = vec![41_000u16; (W * H) as usize];
    for y in 0..H {
        for x in 0..W {
            let i = ((y * W + x) * 3) as usize;
            let holder = y < 120 || !(10..W - 10).contains(&x) || y >= H - 10;
            rgb[i..i + 3].copy_from_slice(&if holder { HOLDER } else { [12000, 7000, 4000] });
            if holder {
                ir[(y * W + x) as usize] = 1_300;
            }
        }
    }
    write_hdri(&path, W, H, &rgb, &ir);

    // `--export-ir` keeps the unrelated "IR preserved but not used" note off the
    // `--strict` run below, so the only thing that can fail it is the cap warning.
    let (out, ir_out) = (dir.path("out.tif"), dir.path("ir.tif"));
    let convert_with = |extra: &[&str]| -> (i32, String, String) {
        let mut args = vec![
            "convert",
            "--film-base",
            "0.9,0.6,0.5",
            "--input-transfer",
            "linear",
            "--input-meaning",
            "scanner-device",
            "--export-ir",
            ir_out.to_str().unwrap(),
        ];
        args.extend(MEASURED);
        args.extend(extra);
        args.extend(["-o", out.to_str().unwrap(), path.to_str().unwrap()]);
        run(&args)
    };

    let (code, stdout, err) = convert_with(&[]);
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);
    let holder = &report["effective_area"]["holder"];
    assert_eq!(
        (
            holder["top"].as_u64(),
            holder["bottom"].as_u64(),
            holder["left"].as_u64(),
            holder["right"].as_u64()
        ),
        (Some(100), Some(10), Some(100), Some(100)),
        "the beyond-cap frame the warning exists for: {holder}"
    );
    assert_eq!(
        holder["capped"],
        serde_json::json!({"top": true, "bottom": false, "left": true, "right": true}),
        "per-edge, so an inflated edge is distinguishable from a measured one"
    );
    assert!(
        holder["converged"].as_bool().unwrap(),
        "and the cap settles, which is why `converged` cannot be read alone"
    );
    let warnings = report["warnings"].as_array().unwrap();
    let capped = warnings
        .iter()
        .find_map(|w| {
            let w = w.as_str().unwrap();
            w.contains("depth march hit its cap").then_some(w)
        })
        .unwrap_or_else(|| panic!("the cap must warn: {warnings:?}"));
    assert!(
        capped.contains("top, left, right") && capped.contains("--measure-inset"),
        "naming the edges and the remedy: {capped}"
    );

    // Falsifiability, and the `--strict` half: the same frame with a sub-cap holder
    // warns about nothing, while the capped one fails.
    let (code, _, err) = convert_with(&["--strict"]);
    assert_eq!(code, 1, "--strict must promote it: {err}");
    assert!(err.contains("depth march hit its cap"), "{err}");

    let shallow = dir.path("shallow.tif");
    for y in 0..120u32 {
        for x in 0..W {
            let i = ((y * W + x) * 3) as usize;
            let holder = y < 10 || !(10..W - 10).contains(&x);
            rgb[i..i + 3].copy_from_slice(&if holder { HOLDER } else { [12000, 7000, 4000] });
            ir[(y * W + x) as usize] = if holder { 1_300 } else { 41_000 };
        }
    }
    write_hdri(&shallow, W, H, &rgb, &ir);
    let (code, stdout, err) = run(&[
        &[
            "convert",
            "--film-base",
            "0.9,0.6,0.5",
            "--input-transfer",
            "linear",
            "--input-meaning",
            "scanner-device",
            "--export-ir",
            dir.path("ir2.tif").to_str().unwrap(),
            "--strict",
            "-o",
            dir.path("out2.tif").to_str().unwrap(),
            shallow.to_str().unwrap(),
        ][..],
        &MEASURED,
    ]
    .concat());
    assert_eq!(
        code, 0,
        "falsifiability: a sub-cap march must not warn at all: {err}"
    );
    assert_eq!(
        json(&stdout)["effective_area"]["holder"]["capped"],
        serde_json::json!({"top": false, "bottom": false, "left": false, "right": false}),
        "{stdout}"
    );
}

// ---------------------------------------------------------------------------
// The rendering chain: the fixed decode, its stages and the destinations
// ---------------------------------------------------------------------------

/// The `u16` samples of a TIFF `hanten` wrote.
fn read_u16_tiff(path: &Path) -> Vec<u16> {
    use tiff::decoder::{Decoder, DecodingResult};
    let mut dec =
        Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap())).unwrap();
    match dec.read_image().unwrap() {
        DecodingResult::U16(data) => data,
        other => panic!(
            "expected u16 samples, got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

/// Per-channel means of interleaved RGB `u16` samples.
fn channel_means(samples: &[u16]) -> [f64; 3] {
    let mut sum = [0f64; 3];
    for px in samples.as_chunks::<3>().0 {
        for c in 0..3 {
            sum[c] += f64::from(px[c]);
        }
    }
    sum.map(|v| v / (samples.len() / 3) as f64)
}

#[test]
fn scene_correction_applies_the_stated_gains_and_exposure() {
    // `nf-scene-correction/stage` through the binary: each knob reaches the pixels,
    // and the report states what was applied.
    let tmp = TempDir::new("scene");
    let input = fixture("hdr-48bit.tif").display().to_string();
    let convert = |name: &str, extra: &[&str]| {
        let out = tmp.path(name);
        let mut argv = vec![
            "convert",
            input.as_str(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
        ];
        argv.extend_from_slice(extra);
        let (code, stdout, err) = run(&argv);
        assert_eq!(code, 0, "{extra:?}: {err}");
        (out, json(&stdout))
    };
    let applied = |report: &serde_json::Value| report["chain"]["stages"][0]["applied"].clone();

    let (plain, report) = convert("plain.tiff", &[]);
    let sc = &report["chain"]["scene_correction"];
    assert_eq!(
        *sc,
        serde_json::json!({"white_balance": [1.0, 1.0, 1.0], "exposure": 0.0}),
        "the gains are always stated, so there is no provenance to report"
    );
    assert_eq!(applied(&report), "identity");
    let plain_means = channel_means(&read_u16_tiff(&plain));

    // Exposure: one stop down darkens every channel.
    let (darker, report) = convert("darker.tiff", &["--exposure", "-1"]);
    assert_eq!(applied(&report), "exposure");
    assert_eq!(report["chain"]["scene_correction"]["exposure"], -1.0);
    let darker_means = channel_means(&read_u16_tiff(&darker));
    for c in 0..3 {
        assert!(
            darker_means[c] < plain_means[c],
            "channel {c}: {darker_means:?}"
        );
    }

    // Stated white balance: warms red against blue, and is reported as applied.
    let (warm, report) = convert("warm.tiff", &["--white-balance", "1.3,1,0.7"]);
    assert_eq!(applied(&report), "white-balance");
    let sc = &report["chain"]["scene_correction"];
    assert_eq!(
        sc["white_balance"],
        serde_json::json!([1.3, 1.0, 0.7]),
        "{sc}"
    );
    let warm_means = channel_means(&read_u16_tiff(&warm));
    assert!(
        warm_means[0] / warm_means[2] > plain_means[0] / plain_means[2],
        "{warm_means:?} vs {plain_means:?}"
    );

    // The recipe spelling reaches the same knobs, and a dump writes them back.
    let recipe = write_file(
        &tmp.path("scene.json"),
        r#"{ "recipe_version": 3,
             "scene_correction": { "white_balance": {"explicit": [1.3, 1.0, 0.7]} } }"#,
    );
    let dump = tmp.path("dump.json");
    let (from_recipe, _) = convert(
        "recipe.tiff",
        &[
            "--params",
            recipe.to_str().unwrap(),
            "--dump-params",
            dump.to_str().unwrap(),
        ],
    );
    assert_eq!(
        std::fs::read(&from_recipe).unwrap(),
        std::fs::read(&warm).unwrap(),
        "the recipe key and the flag are one knob"
    );
    let dumped: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&dump).unwrap()).unwrap();
    assert_eq!(
        dumped["scene_correction"],
        serde_json::json!({"white_balance": {"explicit": [1.3, 1.0, 0.7]}, "exposure": 0.0})
    );
}

#[test]
fn fit_range_fits_the_scene_range_with_the_stated_headroom() {
    // `nf-display-stages/fit-range` through the binary: the headroom flag reaches the
    // stage, the report names the operator and its arguments rather than describing
    // them, and zero headroom is the identity — which clips far more of an unbounded
    // decode at the encode than the default does.
    let tmp = TempDir::new("fit-range");
    let input = fixture("hdr-48bit.tif").display().to_string();
    let convert = |name: &str, extra: &[&str]| {
        let out = tmp.path(name);
        let mut argv = vec![
            "convert",
            input.as_str(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
        ];
        argv.extend_from_slice(extra);
        let (code, stdout, err) = run(&argv);
        assert_eq!(code, 0, "{extra:?}: {err}");
        (out, json(&stdout))
    };
    // A run that clipped nothing carries no `warnings` array at all.
    let clipped = |report: &serde_json::Value| {
        report["warnings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|w| w.as_str())
            .find_map(|w| w.strip_prefix("output lost "))
            .and_then(|w| w.split(' ').next())
            .map_or(0, |n| n.parse::<u64>().unwrap())
    };

    let (default, report) = convert("default.tiff", &[]);
    assert_eq!(report["chain"]["fit_range"]["headroom_stops"], 6.0);
    let default_clipped = clipped(&report);

    let (four, report) = convert("four.tiff", &["--display-tone-headroom", "4"]);
    let fr = &report["chain"]["fit_range"];
    assert_eq!(fr["operator"], "reinhard-peak-lifted-v1", "{fr}");
    assert_eq!(fr["headroom_stops"], 4.0);
    assert_eq!(fr["white_point"], 16.0);
    assert_ne!(read_u16_tiff(&four), read_u16_tiff(&default));

    // Zero headroom leaves reinhard out; display black still runs unless it is off.
    let (_, report) = convert("zero-black.tiff", &["--display-tone-headroom", "0"]);
    assert_eq!(report["chain"]["fit_range"]["operator"], "identity");
    assert_eq!(
        report["chain"]["stages"][2]["applied"],
        "log-shift-to-mid-grey-v1"
    );
    let (_, report) = convert(
        "zero.tiff",
        &["--display-tone-headroom", "0", "--display-black", "off"],
    );
    assert_eq!(report["chain"]["fit_range"]["operator"], "identity");
    assert_eq!(report["chain"]["stages"][2]["applied"], "identity");
    assert!(
        clipped(&report) > default_clipped,
        "the identity must clip more than reinhard: {} vs {default_clipped}",
        clipped(&report)
    );

    // Display black: a deeper setting, and off, each render differently.
    let (deeper, report) = convert("deeper.tiff", &["--display-black", "7"]);
    assert_eq!(
        report["chain"]["fit_range"]["display_black"]["setting"],
        7.0
    );
    assert_ne!(read_u16_tiff(&deeper), read_u16_tiff(&default));
    let (off, report) = convert("off.tiff", &["--display-black", "off"]);
    let black = &report["chain"]["fit_range"]["display_black"];
    assert_eq!(black["setting"], "off", "{black}");
    assert_eq!(black["curve"], "identity");
    assert_eq!(black["shift_stops"], 0.0);
    assert_ne!(read_u16_tiff(&off), read_u16_tiff(&default));

    // A bad headroom is refused by its recipe key as well as its flag.
    let (code, _out, err) = run(&[
        "convert",
        input.as_str(),
        "-o",
        tmp.path("bad.tiff").to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--display-tone-headroom",
        "-1",
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("`fit_range.headroom_stops`"), "{err}");

    // A bad display black is refused naming its flag and key.
    let refused = |extra: &[&str]| {
        let out = tmp.path("refused.tiff");
        let mut argv = vec![
            "convert",
            input.as_str(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
        ];
        argv.extend_from_slice(extra);
        let (code, _out, err) = run(&argv);
        assert_eq!(code, 2, "{extra:?}: {err}");
        err
    };
    let err = refused(&["--display-black", "0"]);
    assert!(
        err.contains("--display-black (recipe `fit_range.display_black`)"),
        "{err}"
    );
}

#[test]
fn the_default_destination_renders_a_display_p3_tiff() {
    // The fixed decode → the chain → the default destination. Both fixtures, so the
    // IR-carrying path is covered too. The pass bar
    // is "a file that decodes and is not obviously broken" — whether it *looks* right
    // is `nf-calibration`'s question.
    let tmp = TempDir::new("render");
    for name in ["hdr-48bit.tif", "hdri-64bit.tif"] {
        // A bare stem: the path is completed from the destination.
        let stem = tmp.path(name.trim_end_matches(".tif"));
        let (code, stdout, err) = run(&[
            "convert",
            fixture(name).to_str().unwrap(),
            "-o",
            stem.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
        ]);
        assert_eq!(code, 0, "{name}: {err}");
        let out = PathBuf::from(format!("{}.tiff", stem.display()));
        assert!(is_tiff(&out), "{name}: {} must be a TIFF", out.display());
        assert_eq!(read_tiff_bits(&out), 16, "{name}");

        let report = json(&stdout);
        assert_eq!(report["output"], out.to_str().unwrap());
        let nf = &report["chain"];
        // Every axis resolved, as the recipe `output` that replays it.
        assert_eq!(
            nf["destination"],
            serde_json::json!({"display": {
                "range": "sdr", "transfer": "native", "gamut": "display-p3", "container": "tiff"
            }}),
            "{stdout}"
        );
        assert_eq!(nf["decode"]["anchor_rule"], "mid-at-base-offset");
        assert_eq!(nf["decode"]["reads_reference"], false);
        let applied: Vec<&str> = nf["stages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["applied"].as_str().unwrap())
            .collect();
        assert_eq!(
            applied,
            [
                "identity",
                "contrast+highlight-desaturation",
                "reinhard-peak-lifted-v1+log-shift-to-mid-grey-v1",
                "acescg-to-display-p3-matrix+neutral-axis-radial-boundary-v2"
            ],
            "the report states what each stage did, identities included"
        );
        let fr = &nf["fit_range"];
        assert_eq!(fr["operator"], "reinhard-peak-lifted-v1", "{stdout}");
        assert_eq!(fr["headroom_stops"], 6.0);
        assert_eq!(fr["white_point"], 64.0);
        assert_eq!(fr["display_peak"], 1.0);
        // At the fallback slope (a white 1.75 stops up) the base renders ≈ 5.0 stops
        // under mid-grey, so display black shifts it to the default 6.
        let black = &fr["display_black"];
        assert_eq!(black["setting"], 6.0, "{stdout}");
        assert_eq!(black["curve"], "log-shift-to-mid-grey-v1");
        let base = black["film_base_stops"].as_f64().unwrap();
        let shift = black["shift_stops"].as_f64().unwrap();
        assert!((base - 4.96).abs() < 0.05, "{black}");
        assert!((base - shift - 6.0).abs() < 1e-3, "{black}");
        // No section of the removed chain's report survives to claim an operation this
        // run did not perform.
        for absent in [
            "reconstruction_result",
            "output_render",
            "dmax",
            "white_balance",
        ] {
            assert!(
                report.get(absent).is_none(),
                "{absent} must be absent: {stdout}"
            );
        }
        assert!(report["identity"]["params_hash"].is_string(), "{stdout}");
        assert!(nf.get("sidecar_written").is_none(), "{stdout}");
        assert!(!sidecar_of(&out).exists(), "no sidecar is written");
    }
}

#[test]
fn memory_gate_admits_at_the_modelled_peak_and_rejects_below_it() {
    // A budget one byte under the estimate the preflight reports must exit 6 before
    // anything is written, and the estimate itself must pass.
    let tmp = TempDir::new("memory-gate");
    let run_with = |out: &Path, budget: Option<&str>| {
        let mut argv = vec![
            "convert".to_string(),
            fixture("hdri-64bit.tif").display().to_string(),
            "-o".into(),
            out.display().to_string(),
            "--film-base".into(),
            "0.9,0.55,0.42".into(),
        ];
        if let Some(b) = budget {
            argv.extend(["--max-memory".into(), b.to_string()]);
        }
        run(&argv.iter().map(String::as_str).collect::<Vec<_>>())
    };
    let (code, stdout, err) = run_with(&tmp.path("probe.tiff"), None);
    assert_eq!(code, 0, "{err}");
    let peak = json(&stdout)["memory"]["estimated_peak_bytes"]
        .as_u64()
        .unwrap();

    let under = tmp.path("under.tiff");
    let (code, _o, err) = run_with(&under, Some(&(peak - 1).to_string()));
    assert_eq!(code, 6, "{err}");
    assert!(!under.exists(), "a rejected run writes nothing");

    let (code, _o, err) = run_with(&tmp.path("at.tiff"), Some(&peak.to_string()));
    assert_eq!(code, 0, "the modelled peak itself must be admitted: {err}");
}

/// The reference density and the three anchor placements that read it (or pinned
/// black) retired together (`nf-retire/dmax-machinery`): every flag is a usage error
/// naming the one placement left, and that remedy renders. (A recipe's
/// `calibration.dmax` is refused at load:
/// `convert_refuses_a_recipe_calibration_dmax`.)
#[test]
fn the_reference_density_and_retired_placements_are_migration_errors() {
    let tmp = TempDir::new("dmax-retired");
    let scan = fixture("hdr-48bit.tif");
    let out = tmp.path("out.tif");

    // (a) Each removed flag. The remedy must itself be accepted.
    for flags in [
        vec!["--d-max", "1.6"],
        vec!["--fixed-d-max"],
        vec!["--auto-d-max"],
        vec!["--no-d-max"],
        vec!["--anchor-white-at-reference"],
        vec!["--anchor-mid-fraction", "0.5"],
        vec!["--anchor-black-floor", "0.005"],
    ] {
        let mut argv = vec![
            "convert",
            scan.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
        ];
        argv.extend_from_slice(&flags);
        let (code, _, err) = run(&argv);
        assert_eq!(code, 2, "{flags:?}: {err}");
        assert!(
            err.contains(flags[0]) && err.contains("was removed"),
            "{flags:?}: {err}"
        );
        assert!(err.contains("--anchor-mid-offset"), "{flags:?}: {err}");
        assert!(!out.exists(), "{flags:?}: nothing may be written");
    }
    let (code, _, err) = run(&[
        "convert",
        scan.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--anchor-mid-offset",
        "0.5",
        "--report",
        "none",
    ]);
    assert_eq!(code, 0, "the named remedy must work: {err}");

    // (b) `estimate`'s reference half.
    let (code, _, err) = run(&[
        "measure-base",
        scan.to_str().unwrap(),
        "--d-max-region",
        "0,0,1,1",
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("--d-max-region") && err.contains("was removed"),
        "{err}"
    );
}

/// `nf-retire/characteristic`: `--density-curve`, `--film-stock` and `--preset` are
/// migration errors at every value, diagnosed before anything coarser.
#[test]
fn the_characteristic_curve_is_a_migration_error() {
    let tmp = TempDir::new("characteristic-retired");
    let scan = fixture("hdr-48bit.tif");
    let out = tmp.path("out.tif");

    // Each flag with no film base: the removed flag is the more specific diagnosis and
    // must win over "no film base selected".
    for flags in [
        vec!["--density-curve", "characteristic"],
        vec!["--density-curve", "exponential"],
        vec!["--density-curve"],
        vec!["--film-stock", "portra-400"],
        vec!["--preset", "characteristic-generic"],
        vec!["--preset", "sigmoid-knees"],
    ] {
        let mut argv = vec![
            "convert",
            scan.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--report",
            "none",
        ];
        argv.extend_from_slice(&flags);
        let (code, _, err) = run(&argv);
        assert_eq!(code, 2, "{flags:?}: {err}");
        assert!(
            err.contains(&format!("{} was removed", flags[0])),
            "{flags:?}: {err}"
        );
        assert!(!err.contains("no film base selected"), "{flags:?}: {err}");
        assert!(!out.exists(), "no output on a usage error");
    }
}

/// `nf-retire/regional-balance`: the four flags are migration errors at every value,
/// naming the grade that succeeded them, and nothing the tool writes carries a balance.
#[test]
fn the_regional_balance_is_a_migration_error() {
    let tmp = TempDir::new("balance-retired");
    let scan = fixture("hdr-48bit.tif");
    let out = tmp.path("out.tif");
    let convert = |extra: &[&str]| {
        let mut argv = vec![
            "convert",
            scan.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
            "--report",
            "none",
        ];
        argv.extend_from_slice(extra);
        let r = run(&argv);
        std::fs::remove_file(&out).ok();
        (r.0, r.2)
    };

    // (a) Each removed flag at every value — the identity `0,0,0`, a negative value in
    // both spellings, a bare flag — reaches the migration message, not clap's.
    for flags in [
        vec!["--shadow-balance", "0.1,0,0"],
        vec!["--shadow-balance", "-0.05,0,0"],
        vec!["--shadow-balance=-0.05,0,0"],
        vec!["--highlight-balance", "0,0,0"],
        vec!["--balance-range", "0.2,1.2"],
        vec!["--balance-range"],
        vec!["--auto-balance-range"],
    ] {
        let flag = flags[0].split('=').next().unwrap();
        let (code, err) = convert(&flags);
        assert_eq!(code, 2, "{flags:?}: {err}");
        assert!(
            err.contains(&format!("{flag} was removed with the regional balance")),
            "{flags:?}: {err}"
        );
        assert!(
            err.contains("`--channel-grade R,B` (recipe `look.channel_grade`)")
                && err.contains("Drop the flag"),
            "{flags:?}: {err}"
        );
        // It says why the grade is not a rename: the measurement is gone.
        assert!(err.contains("measures nothing"), "{flags:?}: {err}");
    }
    // Both remedies are accepted.
    let (code, err) = convert(&["--channel-grade", "0.95,1.05"]);
    assert_eq!(code, 0, "the grade: {err}");
    let (code, err) = convert(&["--density-offset", "0.05,0,-0.02"]);
    assert_eq!(code, 0, "--density-offset: {err}");

    // (b) Neither the report nor the resolved parameters carry a balance.
    let (code, stdout, err) = run(&[
        "convert",
        scan.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ]);
    assert_eq!(code, 0, "{err}");
    let (code, params, err) = run(&["params"]);
    assert_eq!(code, 0, "{err}");
    for (what, text) in [("report", &stdout), ("params", &params)] {
        for key in ["balance_range", "shadow_balance", "highlight_balance"] {
            assert!(!text.contains(key), "{what} still carries {key}: {text}");
        }
    }
}

#[test]
fn the_fixed_decodes_own_knobs_reach_the_decode() {
    // Everything the fixed decode reads must be accepted **and must arrive** — an
    // accepted flag the decode never saw is the accepted-and-ignored defect, so the
    // report's resolved `chain.decode` block is the witness, not the exit code.
    let tmp = TempDir::new("decode-knobs");
    let decode_of = |extra: &[&str], name: &str| -> (i32, serde_json::Value, String) {
        let out = tmp.path(name);
        let mut argv: Vec<String> = vec![
            "convert".into(),
            fixture("hdr-48bit.tif").display().to_string(),
            "-o".into(),
            out.display().to_string(),
            "--film-base".into(),
            "0.9,0.55,0.42".into(),
        ];
        argv.extend(extra.iter().map(|s| (*s).to_string()));
        let (code, stdout, err) = run(&argv.iter().map(String::as_str).collect::<Vec<_>>());
        let decode = if code == 0 {
            json(&stdout)["chain"]["decode"].clone()
        } else {
            serde_json::Value::Null
        };
        (code, decode, err)
    };
    let close = |v: &serde_json::Value, want: f64| (v.as_f64().unwrap() - want).abs() < 1e-5;

    // The defaults: the decode's own constants.
    let (code, d, err) = decode_of(&[], "default.tiff");
    assert_eq!(code, 0, "{err}");
    assert!(close(&d["linearization"], 1.8), "{d}");
    assert_eq!(d["scale"], serde_json::json!([1.0, 0.84, 0.73]), "{d}");
    // mid-grey 0.62 above base at the linearization 1.8 ⇒ anchor 0.62 + 0.745/1.8.
    assert!(close(&d["anchor"], 0.62 + 0.744_727_5 / 1.8), "{d}");

    let (code, d, err) = decode_of(&["--density-scale", "1,0.9,0.8"], "scale.tiff");
    assert_eq!(code, 0, "{err}");
    assert_eq!(d["scale"], serde_json::json!([1.0, 0.9, 0.8]), "{d}");

    let (code, d, err) = decode_of(&["--density-offset", "0,-0.03,-0.05"], "offset.tiff");
    assert_eq!(code, 0, "{err}");
    assert!(close(&d["offset"][2], -0.05), "{d}");

    let (code, d, err) = decode_of(&["--anchor-mid-offset", "0.7"], "anchor.tiff");
    assert_eq!(code, 0, "{err}");
    assert!(close(&d["anchor"], 0.7 + 0.744_727_5 / 1.8), "{d}");

    // `--density-gamma` sets `reconstruction.linearization` directly.
    let (code, d, err) = decode_of(&["--density-gamma", "1.7"], "bare-gamma.tiff");
    assert_eq!(code, 0, "{err}");
    assert!(close(&d["linearization"], 1.7), "{d}");

    // The look's contrast is not the decode's: it leaves the decode block untouched.
    let (code, d, err) = decode_of(&["--contrast", "1.5"], "look-contrast.tiff");
    assert_eq!(code, 0, "{err}");
    assert!(close(&d["linearization"], 1.8), "{d}");
    assert!(close(&d["anchor"], 0.62 + 0.744_727_5 / 1.8), "{d}");
}

/// A recipe-stated `calibration.dmax` is refused — and `calibration.film_base` still
/// accepted.
///
/// The `calibration` section holds the film base alone, since the fixed decode's anchor
/// rule reads no reference density. A recipe stating `dmax` there is refused at load by
/// name (`crate::recipe::check_body`) — and so is a `roll` per-frame overlay, which runs
/// the same check.
#[test]
fn convert_refuses_a_recipe_calibration_dmax() {
    let tmp = TempDir::new("calibration-dmax");
    let run_with = |body: &str, name: &str| -> (i32, String) {
        let recipe = write_file(&tmp.path(name), body);
        let (code, _out, err) = run(&[
            "convert",
            fixture("hdr-48bit.tif").to_str().unwrap(),
            "-o",
            tmp.path("out.tiff").to_str().unwrap(),
            "--params",
            recipe.to_str().unwrap(),
            "--report",
            "none",
        ]);
        (code, err)
    };

    let (code, err) = run_with(
        r#"{"recipe_version":3,
            "calibration":{"film_base":{"explicit":[0.9,0.55,0.42]},
                           "dmax":{"explicit":1.45}}}"#,
        "with-dmax.json",
    );
    assert_eq!(code, 2, "a recipe-stated reference must be refused: {err}");
    assert!(err.contains("`calibration.dmax`"), "{err}");
    assert!(err.contains("reference-free"), "{err}");

    // Falsifiable both ways. The base half is still read, so it renders…
    let (code, err) = run_with(
        r#"{"recipe_version":3,"calibration":{"film_base":{"explicit":[0.9,0.55,0.42]}}}"#,
        "base-only.json",
    );
    assert_eq!(code, 0, "the base half must still be accepted: {err}");
    // A `roll` **per-frame** override states it too, and is refused by the same load
    // check, run on the overlay.
    let shared = write_file(
        &tmp.path("shared.json"),
        r#"{"recipe_version":3,
            "calibration":{"film_base":{"explicit":[0.9,0.55,0.42]}},
            "measure":{"inset":0.05}}"#,
    );
    let manifest = write_file(
        &tmp.path("frames.json"),
        &format!(
            r#"{{ "frames": [ {{ "input": {scan:?},
                    "params": {{ "calibration": {{ "dmax": {{ "explicit": 2.4 }} }} }} }} ] }}"#,
            scan = fixture("hdr-48bit.tif").to_str().unwrap()
        ),
    );
    let (code, _out, err) = run(&[
        "roll",
        "--frames",
        manifest.to_str().unwrap(),
        "--out-dir",
        tmp.path("roll-out").to_str().unwrap(),
        "--params",
        shared.to_str().unwrap(),
        "--report",
        "none",
    ]);
    assert_eq!(code, 2, "a per-frame override must be refused too: {err}");
    assert!(err.contains("`calibration.dmax`"), "{err}");
}

#[test]
fn convert_refuses_a_pre_flip_recipe_and_reads_a_current_one() {
    // A recipe written for the removed chain would otherwise parse and be read by
    // nothing — `deny_unknown_fields` catches an unknown key and is blind to a known
    // but meaningless one. The document version is what makes it visible.
    let tmp = TempDir::new("recipe-provenance");
    let run_with = |body: &str, name: &str| -> (i32, String) {
        let recipe = write_file(&tmp.path(name), body);
        let (code, _out, err) = run(&[
            "convert",
            fixture("hdr-48bit.tif").to_str().unwrap(),
            "-o",
            tmp.path(&format!("{name}.tif")).to_str().unwrap(),
            "--params",
            recipe.to_str().unwrap(),
            "--report",
            "none",
        ]);
        (code, err)
    };

    // (1) No version: the removed chain's document, refused as a whole with the way
    // forward.
    let (code, err) = run_with(
        r#"{
  "reconstruction": { "type": "density" },
  "calibration": { "film_base": { "explicit": [0.9, 0.55, 0.42] } },
  "output": { "preset": "display-p3" }
}"#,
        "pre-flip.json",
    );
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("\"recipe_version\": 3"), "{err}");
    assert!(err.contains("hanten params"), "{err}");

    // (2) Versioned, but still carrying the removed chain's reconstruction keys:
    // refused by key, with where each one went.
    let (code, err) = run_with(
        r#"{"recipe_version": 3,
            "reconstruction": {"density": {"scale": [1, 0.9, 0.8]}},
            "calibration": {"film_base": {"explicit": [0.9, 0.55, 0.42]}}}"#,
        "old-keys.json",
    );
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("`reconstruction.density`"), "{err}");
    assert!(err.contains("`reconstruction.scale`"), "{err}");

    // (3) A recipe stating the decode renders, with its values.
    let (code, err) = run_with(
        r#"{"recipe_version": 3,
            "reconstruction": {"scale": [1, 0.9, 0.8], "linearization": 1.7,
                               "anchor": {"mid-at-base-offset": 0.6}},
            "calibration": {"film_base": {"explicit": [0.9, 0.55, 0.42]}},
            "measure": {"inset": 0.05},
            "look": {}}"#,
        "new.json",
    );
    assert_eq!(code, 0, "{err}");
    let (code, stdout, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        tmp.path("new-report.tif").to_str().unwrap(),
        "--params",
        tmp.path("new.json").to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    let decode = &json(&stdout)["chain"]["decode"];
    let close = |v: &serde_json::Value, want: f64| (v.as_f64().unwrap() - want).abs() < 1e-5;
    assert!(close(&decode["linearization"], 1.7), "{decode}");
    assert!(close(&decode["scale"][1], 0.9), "{decode}");

    // (4) Its values are checked, whichever provenance set them.
    let (code, err) = run_with(
        r#"{"recipe_version": 3, "reconstruction": {"linearization": 0},
            "calibration": {"film_base": {"explicit": [0.9, 0.55, 0.42]}}}"#,
        "bad.json",
    );
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("`reconstruction.linearization`"), "{err}");

    // (5) A stage refuses a key it does not have.
    let (code, err) = run_with(
        r#"{"recipe_version": 3, "look": {"saturation": 1.1},
            "calibration": {"film_base": {"explicit": [0.9, 0.55, 0.42]}}}"#,
        "look.json",
    );
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("saturation"), "{err}");

    // (6) The decode's slope before the split is refused by name, with the split's
    // remedy — never read as the linearization it no longer is.
    let (code, err) = run_with(
        r#"{"recipe_version": 3, "reconstruction": {"contrast": 2.0},
            "calibration": {"film_base": {"explicit": [0.9, 0.55, 0.42]}}}"#,
        "old-contrast.json",
    );
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("`reconstruction.contrast` split in two") && err.contains("look.contrast"),
        "{err}"
    );
}

#[test]
fn hanten_params_writes_the_recipe_convert_reads() {
    let (code, params, err) = run(&["params"]);
    assert_eq!(code, 0, "{err}");
    let doc: serde_json::Value = serde_json::from_str(&params).unwrap();
    assert_eq!(doc["recipe_version"], 3);
    assert!(doc.get("print").is_none(), "{doc}");

    // The selector it once took is a removed flag.
    let (code, _out, err) = run(&["params", "--new-flow"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--new-flow was removed"), "{err}");

    // What it writes is what `convert` reads, once a film base is stated.
    let tmp = TempDir::new("params-roundtrip");
    let mut doc = doc.clone();
    doc["calibration"]["film_base"] = serde_json::json!({"explicit": [0.9, 0.55, 0.42]});
    let recipe = write_file(&tmp.path("r.json"), &doc.to_string());
    let (code, _out, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        tmp.path("out").to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
        "--report",
        "none",
    ]);
    assert_eq!(code, 0, "{err}");
}

#[test]
fn the_removed_print_controls_are_refused_with_where_they_went() {
    // The print family, driven through the binary one flag at a time. Each names where
    // its knob went, so the refusal tells a user more than that it is gone. One of these
    // is the documented **default** (`--linear-range 0,1`) and is still refused: there is
    // no `print` section left for a flag to reset.
    let tmp = TempDir::new("removed-print");
    let fixture_path = fixture("hdr-48bit.tif").display().to_string();
    let cases: &[(&[&str], &str)] = &[
        (&["--print-exposure", "1"], "`--exposure`"),
        (&["--black-point", "0.01"], "`--display-black`"),
        (&["--linear-range", "0,1"], "`--display-black`"),
        (&["--auto-wb", "percentile"], "hanten measure-roll"),
    ];
    for (i, (extra, expect)) in cases.iter().enumerate() {
        let out = tmp.path(&format!("out{i}.tif"));
        let mut argv: Vec<&str> = vec![
            "convert",
            &fixture_path,
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
            "--report",
            "none",
        ];
        argv.extend_from_slice(extra);
        let (code, _out, err) = run(&argv);
        assert_eq!(code, 2, "{extra:?} must be refused: {err}");
        assert!(err.contains(extra[0]), "{extra:?} must be named: {err}");
        assert!(
            err.contains(expect),
            "{extra:?} must say where it went: {err}"
        );
    }
}

#[test]
fn the_removed_output_policy_flags_are_refused() {
    // A destination is separate knobs, not a preset name, so the preset flag is refused
    // and the message names the destination flags instead; the depth, profile and
    // BigTIFF selectors are refused with nothing to pick.
    let tmp = TempDir::new("removed-output");
    let fixture_path = fixture("hdr-48bit.tif").display().to_string();
    for (i, extra) in [
        vec!["--output-preset", "display-p3"],
        vec!["--out-depth", "u16"],
        vec!["--output-profile", "srgb"],
        vec!["--bigtiff", "auto"],
    ]
    .into_iter()
    .enumerate()
    {
        let out = tmp.path(&format!("out{i}.tif"));
        let mut argv: Vec<&str> = vec![
            "convert",
            &fixture_path,
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
            "--report",
            "none",
        ];
        argv.extend_from_slice(&extra);
        let (code, _out, err) = run(&argv);
        assert_eq!(code, 2, "{extra:?} must be refused: {err}");
        assert!(err.contains(extra[0]), "{extra:?} must be named: {err}");
        assert!(err.contains("was removed"), "{extra:?}: {err}");
        if extra[0] != "--bigtiff" {
            assert!(
                err.contains("--range") && err.contains("--film-master"),
                "{extra:?} must name the destination flags: {err}"
            );
        }
    }
}

#[test]
fn a_print_section_or_an_output_preset_is_refused_by_name() {
    // The recipe's spelling of the flags above: there is no `print` section, and
    // `output` is the destination's axes, so each is named with where its knobs went.
    let tmp = TempDir::new("removed-sections");
    for (name, body, went) in [
        (
            "print",
            r#"{ "recipe_version": 3, "print": { "print_exposure": 1.0 } }"#,
            "scene_correction",
        ),
        (
            "output",
            r#"{ "recipe_version": 3, "output": { "preset": "display-p3" } }"#,
            "output.display",
        ),
    ] {
        let recipe = write_file(&tmp.path(&format!("{name}.json")), body);
        let out = tmp.path(&format!("{name}.tif"));
        let input = fixture("hdr-48bit.tif");
        let (code, _out, err) = run(&[
            "convert",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
            "--params",
            recipe.to_str().unwrap(),
            "--report",
            "none",
        ]);
        assert_eq!(code, 2, "a recipe `{name}` section must be refused: {err}");
        assert!(err.contains(&format!("`{name}")), "{err}");
        assert!(err.contains(went), "{err}");
    }
}

#[test]
fn the_anchor_guard_recommends_only_a_slope() {
    // `nf-core/knob-availability-audit`, finding #2. The remedy used to end "or
    // --anchor-white-at-reference, which needs no such division" — a placement the
    // rule never checked was available, and one that has since retired.
    //
    // The losing wording is asserted **absent**, not merely the new one present:
    // both sentences name the same flag (the explanation still does, as a fact about
    // the arithmetic), so a `contains` on the flag alone cannot tell them apart.
    let tmp = TempDir::new("anchor-guard-remedy");
    let (code, _out, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        // A bare stem, completed, so the suffix rule — which runs before this one —
        // cannot be what answers.
        tmp.path("out").to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--density-gamma",
        "2e-39",
        "--anchor-mid-offset",
        "0.62",
        "--report",
        "none",
    ]);
    assert_eq!(code, 2, "{err}");
    // The guard checks the decode's recipe (`crate::recipe::validate`), and its remedy
    // names only the slope and the offset, which is what this command line can change.
    assert!(err.contains("the decode's anchor is not usable"), "{err}");
    assert!(err.contains("Use a larger --density-gamma"), "{err}");
    assert!(
        !err.contains("which needs no such division"),
        "the remedy must not recommend a placement it never checked: {err}"
    );
}

#[test]
fn convert_writes_no_sidecar_and_guards_no_phantom_one() {
    // No sidecar is written (the report's `recipe` is the record), so none is a write
    // target either: `-o out --report-file out.tiff.json` must not be refused for
    // colliding with a file that is never written.
    let tmp = TempDir::new("no-sidecar");
    let stem = tmp.path("out");
    let report = tmp.path("out.tiff.json");
    let (code, _out, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        stem.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--report-file",
        report.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(!err.contains("collides with the sidecar"), "{err}");
    assert!(json(&std::fs::read_to_string(&report).unwrap())["chain"].is_object());

    // The guard's *real* checks still run, so a report file aimed at the input scan
    // is still refused.
    let victim = tmp.path("victim.tif");
    std::fs::copy(fixture("hdr-48bit.tif"), &victim).unwrap();
    let before = std::fs::metadata(&victim).unwrap().len();
    let (code, _out, err) = run(&[
        "convert",
        victim.to_str().unwrap(),
        "-o",
        tmp.path("o.tif").to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--report-file",
        victim.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("overwrite the input scan"), "{err}");
    assert_eq!(
        std::fs::metadata(&victim).unwrap().len(),
        before,
        "the input must be untouched"
    );
}

#[test]
fn convert_removes_a_stale_sidecar_and_nothing_else() {
    // A pre-flip run left `out.tiff.json` beside `out.tiff`; a run now replaces the
    // image and writes no sidecar of its own, so the old one would sit there
    // describing a picture that no longer exists. It is removed, and the report says
    // so — but only a file that *is* one of nc's `{meta, params}` sidecars.
    let tmp = TempDir::new("stale-sidecar");
    let out = tmp.path("out.tiff");
    let convert = || {
        let (code, stdout, err) = run(&[
            "convert",
            fixture("hdr-48bit.tif").to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
        ]);
        assert_eq!(code, 0, "{err}");
        json(&stdout)
    };
    std::fs::write(&out, b"the old image").unwrap();
    write_stale_sidecar(
        &out,
        serde_json::json!({ "output": { "preset": "display-p3" } }),
    );

    let report = convert();
    assert!(
        !sidecar_of(&out).exists(),
        "the stale sidecar must be removed"
    );
    assert_eq!(
        report["chain"]["removed_sidecar"],
        sidecar_of(&out).to_str().unwrap(),
        "and the removal is reported"
    );

    // A file sharing the name that is not an nc sidecar is not ours to remove —
    // including one that merely has the envelope's two key names.
    for body in [r#"{"notes": "mine"}"#, r#"{"meta": null, "params": null}"#] {
        std::fs::write(sidecar_of(&out), body).unwrap();
        let report = convert();
        assert!(
            sidecar_of(&out).exists(),
            "a user's own file must survive: {body}"
        );
        assert!(report["chain"].get("removed_sidecar").is_none());
    }
}

#[test]
fn convert_never_removes_the_recipe_it_read() {
    // Found in review: an enveloped recipe carries a sidecar's identity fields and can
    // sit exactly where the stale sidecar would — here, a pre-flip sidecar whose
    // `params` were rewritten to a current recipe, which `--params` loads. Reading it
    // and then deleting it as "stale" would destroy the run's own input. It stays, and
    // the run says why.
    let tmp = TempDir::new("recipe-is-the-sidecar");
    let out = tmp.path("out.tiff");
    let sidecar = sidecar_of(&out);
    write_stale_sidecar(
        &out,
        serde_json::json!({
            "recipe_version": 3,
            "calibration": { "film_base": { "explicit": [0.9, 0.55, 0.42] } }
        }),
    );

    let (code, stdout, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--params",
        sidecar.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(sidecar.exists(), "the run's own recipe must survive");
    let report = json(&stdout);
    assert!(report["chain"].get("removed_sidecar").is_none());
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("because this run read it")),
        "{stdout}"
    );
}

#[test]
fn roll_judges_a_manifest_suffix_against_the_destination() {
    // A roll manifest's explicit output goes through the same rule `convert` uses, so
    // it is judged against the default TIFF destination — and the refusal names the
    // frame.
    let tmp = TempDir::new("roll-suffix");
    let recipe = write_file(
        &tmp.path("roll.json"),
        r#"{ "recipe_version": 3,
             "calibration": { "film_base": { "explicit": [0.9, 0.55, 0.42] } } }"#,
    );
    let roll_to = |output: &str, dir: &str| {
        let manifest = write_file(
            &tmp.path(&format!("{dir}.json")),
            &format!(
                r#"{{ "frames": [ {{ "input": {:?}, "output": {output:?} }} ] }}"#,
                fixture("hdr-48bit.tif").display().to_string()
            ),
        );
        run(&[
            "roll",
            "--frames",
            manifest.to_str().unwrap(),
            "--out-dir",
            tmp.path(dir).to_str().unwrap(),
            "--params",
            recipe.to_str().unwrap(),
            "--report",
            "none",
        ])
    };
    let (code, _out, err) = roll_to("one.tif", "ok");
    assert_eq!(code, 0, "{err}");
    assert!(tmp.path("ok").join("one.tif").exists());

    let (code, _out, err) = roll_to("one.jpg", "bad");
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("frame "), "the refusal names the frame: {err}");
    assert!(err.contains("does not end in .tif or .tiff"), "{err}");
    assert!(!err.contains("does not match output preset"), "{err}");
}

#[test]
fn roll_renders_every_frame() {
    // `roll` runs every frame through the same frame function `convert` uses: derived
    // names take the destination's `.tiff`, no sidecar is written, and the roll report
    // echoes no recipe.
    let tmp = TempDir::new("roll-renders");
    let recipe = write_file(
        &tmp.path("roll.json"),
        r#"{
  "recipe_version": 3,
  "calibration": { "film_base": { "explicit": [0.9, 0.55, 0.42] } },
  "measure": { "inset": 0.05 }
}"#,
    );
    let out_dir = tmp.path("out");
    let (code, stdout, err) = run(&[
        "roll",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        fixture("hdri-64bit.tif").to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    for stem in ["hdr-48bit", "hdri-64bit"] {
        let out = out_dir.join(format!("{stem}_positive.tiff"));
        assert!(is_tiff(&out), "{}", out.display());
        assert!(!sidecar_of(&out).exists(), "no sidecar is written");
    }
    let report = json(&stdout);
    assert!(report.get("recipe").is_none(), "{stdout}");
    assert!(report["identity"].get("params_hash").is_none(), "{stdout}");
    for frame in report["frames"].as_array().unwrap() {
        assert_eq!(frame["status"], "ok", "{frame}");
        assert_eq!(
            frame["chain"]["destination"]["display"]["gamut"],
            "display-p3"
        );
    }
}

#[test]
fn a_roll_frame_override_reaches_scene_correction() {
    // Exposure is per frame by nature (a bracket, a frame shot a stop over), so a
    // per-frame `scene_correction` overlay must reach that frame and only that one.
    let tmp = TempDir::new("roll-scene");
    let shared = write_file(
        &tmp.path("roll.json"),
        r#"{ "recipe_version": 3,
             "calibration": { "film_base": { "explicit": [0.9, 0.55, 0.42] } } }"#,
    );
    let (a, b) = (fixture("hdr-48bit.tif"), fixture("hdri-64bit.tif"));
    let manifest = write_file(
        &tmp.path("frames.json"),
        &format!(
            r#"{{ "frames": [
                 {{ "input": {a:?}, "params": {{ "scene_correction": {{ "exposure": -1.5 }} }} }},
                 {{ "input": {b:?} }} ] }}"#
        ),
    );
    let out_dir = tmp.path("out");
    let (code, stdout, err) = run(&[
        "roll",
        "--frames",
        manifest.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        shared.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);
    let exposures: Vec<f64> = report["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["chain"]["scene_correction"]["exposure"].as_f64().unwrap())
        .collect();
    assert_eq!(exposures, [-1.5, 0.0], "{stdout}");
    // Each frame's identity hashes the recipe that frame ran, override included.
    let hashes: Vec<&str> = report["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["identity"]["params_hash"].as_str().unwrap())
        .collect();
    assert_ne!(hashes[0], hashes[1], "{stdout}");
    // A frame without an override runs the shared recipe, as `convert` would.
    let (code, stdout, err) = run(&[
        "convert",
        b.to_str().unwrap(),
        "-o",
        tmp.path("single").to_str().unwrap(),
        "--params",
        shared.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        json(&stdout)["identity"]["params_hash"].as_str().unwrap(),
        hashes[1]
    );

    // A value the stage refuses is refused per frame, naming the key — roll takes no
    // conversion flags, so a flag spelling would be a remedy the user cannot type.
    let bad = write_file(
        &tmp.path("bad.json"),
        &format!(
            r#"{{ "frames": [ {{ "input": {a:?},
                 "params": {{ "scene_correction": {{ "white_balance": {{ "explicit": [1, 0, 1] }} }} }} }} ] }}"#
        ),
    );
    let (code, _, err) = run(&[
        "roll",
        "--frames",
        bad.to_str().unwrap(),
        "--out-dir",
        tmp.path("bad-out").to_str().unwrap(),
        "--params",
        shared.to_str().unwrap(),
    ]);
    assert_ne!(code, 0, "{err}");
    assert!(
        err.contains("`scene_correction.white_balance`") && !err.contains("--white-balance"),
        "{err}"
    );
}

#[test]
fn roll_refuses_an_unread_section_in_a_frame_override() {
    // A per-frame overlay can state a section the chain never reads; accepting it
    // would be accepted-and-ignored. Refused, naming the frame, before anything is
    // written.
    let tmp = TempDir::new("roll-overlay");
    let recipe = write_file(
        &tmp.path("roll.json"),
        r#"{ "recipe_version": 3,
             "calibration": { "film_base": { "explicit": [0.9, 0.55, 0.42] } } }"#,
    );
    let manifest = write_file(
        &tmp.path("frames.json"),
        &format!(
            r#"{{ "frames": [ {{ "input": {:?},
                    "params": {{ "print": {{ "print_exposure": 0.5 }} }} }} ] }}"#,
            fixture("hdr-48bit.tif").display().to_string()
        ),
    );
    let out_dir = tmp.path("out");
    let (code, _out, err) = run(&[
        "roll",
        "--frames",
        manifest.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
        "--report",
        "none",
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("frame "), "{err}");
    assert!(err.contains("`print` is a section"), "{err}");
    assert!(!out_dir.exists(), "refused before anything is created");
}

#[test]
fn roll_refuses_a_pre_flip_shared_recipe() {
    // `roll` accepts no conversion flags, so its shared recipe is the *only* way it
    // can state a reconstruction — and therefore the only place the
    // accepted-and-ignored hole could open for it. A recipe without the document
    // version was written for the removed chain, refused at exit 2 before anything is
    // created.
    let tmp = TempDir::new("roll-pre-flip-recipe");
    let recipe = write_file(
        &tmp.path("roll.json"),
        r#"{
  "calibration": { "film_base": { "explicit": [0.9, 0.55, 0.42] } },
  "reconstruction": { "type": "density", "curve": { "type": "exponential" } },
  "output": { "preset": "display-p3" }
}"#,
    );
    let out_dir = tmp.path("out");
    let (code, _stdout, err) = run(&[
        "roll",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
        "--report",
        "none",
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("\"recipe_version\": 3"), "{err}");
    assert!(!out_dir.exists(), "refused before anything is created");
}

#[test]
fn roll_refuses_the_removed_chains_keys_from_either_recipe_site() {
    // Roll accepts no conversion flags, so what it can reach arrives in a recipe —
    // and it reads recipes in **two** places. Both are pinned, because a check composed
    // at one site and forgotten at the other is exactly how a rule loses a call site.
    let tmp = TempDir::new("roll-removed-keys");
    let out_dir = tmp.path("out");
    let roll = |manifest_or_input: &[&str], shared: &Path, out: &Path| {
        let mut v: Vec<String> = vec!["roll".into()];
        v.extend(manifest_or_input.iter().map(|s| (*s).to_string()));
        v.extend([
            "--out-dir".to_string(),
            out.display().to_string(),
            "--params".into(),
            shared.display().to_string(),
            "--report".into(),
            "none".into(),
        ]);
        let borrowed: Vec<&str> = v.iter().map(String::as_str).collect();
        let (code, _out, err) = run(&borrowed);
        (code, err)
    };
    let input = fixture("hdr-48bit.tif").display().to_string();

    // (1) the shared recipe.
    let shared_typed = write_file(
        &tmp.path("shared.json"),
        r#"{
             "recipe_version": 3,
             "reconstruction": { "type": "density" },
             "calibration": { "film_base": { "explicit": [0.9, 0.55, 0.42] } }
           }"#,
    );
    let (code, err) = roll(&[&input], &shared_typed, &out_dir);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("`reconstruction.type`"), "{err}");

    // (2) a per-frame override, on a shared recipe the gate accepts.
    let shared = write_file(
        &tmp.path("roll-new.json"),
        r#"{ "recipe_version": 3,
             "calibration": { "film_base": { "explicit": [0.9, 0.55, 0.42] } } }"#,
    );
    let manifest_with = |name: &str, params: &str| {
        write_file(
            &tmp.path(name),
            &format!(r#"{{ "frames": [ {{ "input": {input:?}, "params": {params} }} ] }}"#),
        )
    };
    // The removed chain's retired selector, at the value every earlier sidecar wrote.
    let typed = manifest_with(
        "typed.json",
        r#"{ "reconstruction": { "type": "density" } }"#,
    );
    let (code, err) = roll(
        &["--frames", typed.to_str().unwrap()],
        &shared,
        &tmp.path("out2"),
    );
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("per-frame `params` override"), "{err}");
    assert!(err.contains("`reconstruction.type`"), "{err}");

    // The overlay lands on the shared document: a decode key it states is read and
    // checked there, not refused as unknown…
    let bad = manifest_with(
        "bad.json",
        r#"{ "reconstruction": { "linearization": -1 } }"#,
    );
    let (code, err) = roll(
        &["--frames", bad.to_str().unwrap()],
        &shared,
        &tmp.path("out2"),
    );
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("`reconstruction.linearization`"), "{err}");
    // It names the frame, and only the recipe key: `roll` accepts no `--density-gamma`.
    assert!(err.contains("per-frame `params` override"), "{err}");
    assert!(!err.contains("--density-gamma"), "{err}");
    // …and a valid one resolves and reaches the frame's render — the frame's own
    // recipe, not the shared one, which is what the decode reads.
    let good = manifest_with(
        "good.json",
        r#"{ "reconstruction": { "linearization": 1.7 } }"#,
    );
    let (code, stdout, err) = run(&[
        "roll",
        "--frames",
        good.to_str().unwrap(),
        "--out-dir",
        tmp.path("out5").to_str().unwrap(),
        "--params",
        shared.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    let frame = &json(&stdout)["frames"][0];
    let linearization = frame["chain"]["decode"]["linearization"].as_f64().unwrap();
    assert!((linearization - 1.7).abs() < 1e-5, "{frame}");
}

// ---------------------------------------------------------------------------
// measure-roll (`nf-scene-correction/roll-white-balance`)
// ---------------------------------------------------------------------------

/// A recipe for synthetic scans: they carry no SilverFast metadata, so the
/// input's transfer and meaning are stated, as `convert` would take them by flag.
fn roll_white_recipe(dir: &TempDir, base: &str) -> PathBuf {
    write_file(
        &dir.path("roll.json"),
        &format!(
            r#"{{ "recipe_version": 3,
                 "input": {{ "transfer": "linear", "meaning": "scanner-device" }},
                 "calibration": {{ "film_base": {{ "explicit": [{base}] }} }} }}"#
        ),
    )
}

#[test]
fn measure_roll_gains_reach_convert_unchanged_by_flag_and_by_recipe() {
    // The contract the command exists for: measure once, state the gains and the
    // white, and every frame renders under exactly them — by the reported flag and by
    // the reported recipe fragment alike, and as the same values stated as style knobs
    // would (`nf-calibration/roll-section` moved them, and moved no pixel).
    let tmp = TempDir::new("measure-roll-reuse");
    let frame = fixture("hdr-48bit.tif").display().to_string();
    let second = tmp.path("second.tif");
    std::fs::copy(&frame, &second).unwrap();
    let recipe = tmp.path("wb.json");
    let (code, stdout, err) = run(&[
        "measure-roll",
        &frame,
        second.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
        "--out",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);
    assert_eq!(report["command"], "measure-roll");
    let gains = report["white_balance"]["gains"].as_array().unwrap().clone();
    assert_eq!(gains[1], 1.0, "green-anchored: {report}");
    assert_ne!(
        report["white_balance"]["gains"],
        serde_json::json!([1.0, 1.0, 1.0]),
        "not vacuous: the fixture's roll white is off neutral"
    );
    assert_eq!(report["frames"].as_array().unwrap().len(), 2);
    assert_eq!(
        report["frames"][0]["sampled"], report["frames"][0]["kept"],
        "nothing guarded without a leader: {report}"
    );
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("no --leader")),
        "an unguarded run says so: {report}"
    );
    // Measured at the decode's output, which runs at the linearization alone.
    assert!(
        (report["decode"]["linearization"].as_f64().unwrap() - 1.8).abs() < 1e-6,
        "{report}"
    );

    // The fixture is low-key, so the frame is lifted and takes its own flags: the roll's,
    // plus its lift.
    let lift = report["frames"][0]["lift_ev"].as_f64().unwrap();
    assert!(lift > 0.0, "{report}");
    let by_flag = tmp.path("flag.tiff");
    let flag: Vec<&str> = report["frames"][0]["flag"]
        .as_str()
        .unwrap()
        .split_whitespace()
        .collect();
    assert_eq!(flag[0], "--roll-white-balance", "{report}");
    assert_eq!(flag[2], "--roll-white", "{report}");
    assert_eq!(flag[4], "--roll-exposure", "{report}");
    assert_eq!(flag[6], "--roll-frame-exposure", "{report}");
    assert!(
        report["reuse"]["flag"]
            .as_str()
            .unwrap()
            .ends_with(&flag[..6].join(" ")),
        "the roll's flag is the frame's without its lift: {report}"
    );
    let (code, stdout, err) = run(&[
        &[
            "convert",
            &frame,
            "--film-base",
            "0.9,0.55,0.42",
            "-o",
            by_flag.to_str().unwrap(),
        ][..],
        &flag,
    ]
    .concat());
    assert_eq!(code, 0, "{err}");
    let converted = json(&stdout);
    assert_eq!(
        converted["chain"]["scene_correction"]["white_balance"], report["white_balance"]["gains"],
        "the flag's text round-trips the gains exactly"
    );
    assert_eq!(
        converted["chain"]["look"]["slope"], report["white"]["slope"],
        "and the slope"
    );
    let roll = &converted["chain"]["roll"];
    assert_eq!(roll["white_stops"], report["white"]["stops"], "{converted}");
    assert_eq!(roll["slope"], report["white"]["slope"], "{converted}");
    assert_eq!(roll["white_balance_applied"], true, "{converted}");
    assert_eq!(roll["slope_applied"], true, "{converted}");
    assert_eq!(roll["exposure"], report["exposure"]["ev"], "{converted}");
    assert_eq!(roll["exposure_applied"], true, "{converted}");
    assert_eq!(
        roll["frame_exposure"], report["frames"][0]["lift_ev"],
        "the flag's text round-trips the lift exactly: {converted}"
    );
    assert_eq!(roll["frame_exposure_applied"], true, "{converted}");
    let summed = (report["exposure"]["ev"].as_f64().unwrap() as f32) + lift as f32;
    assert_eq!(
        converted["chain"]["scene_correction"]["exposure"]
            .as_f64()
            .unwrap() as f32,
        summed,
        "the roll's exposure plus the frame's: {converted}"
    );

    // The gains as a style knob: the section is where they live, not what they do. The
    // stated exposure starts the sum where the roll's would, then the lift adds.
    let as_style = tmp.path("style.tiff");
    let lift_text = report["frames"][0]["lift_ev"].to_string();
    let gains_text = gains
        .iter()
        .map(|g| g.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let stops_text = report["white"]["stops"].to_string();
    let exposure_text = report["exposure"]["ev"].to_string();
    let (code, _, err) = run(&[
        "convert",
        &frame,
        "--film-base",
        "0.9,0.55,0.42",
        "--white-balance",
        &gains_text,
        "--exposure",
        &exposure_text,
        "--roll-frame-exposure",
        &lift_text,
        "--roll-white",
        &stops_text,
        "-o",
        as_style.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        std::fs::read(&by_flag).unwrap(),
        std::fs::read(&as_style).unwrap(),
        "the roll section renders what the same gains as a style knob render"
    );

    // The written recipe states the base it measured under too, so it renders alone.
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&recipe).unwrap()).unwrap();
    assert_eq!(
        written["calibration"]["film_base"]["explicit"],
        serde_json::json!([0.9, 0.55, 0.42]),
        "{written}"
    );
    assert_eq!(
        written["roll"]["white_balance"],
        report["white_balance"]["gains"]
    );
    assert_eq!(written["roll"]["white_stops"], report["white"]["stops"]);
    assert_eq!(written["roll"]["exposure"], report["exposure"]["ev"]);
    assert_eq!(
        written["roll"]["frames"]["hdr-48bit.tif"]["exposure"], report["frames"][0]["lift_ev"],
        "{written}"
    );
    assert!(
        report["reuse"].get("recipe").is_none(),
        "the recipe is the file, not a report field: {report}"
    );
    let by_recipe = tmp.path("recipe.tiff");
    let (code, _, err) = run(&[
        "convert",
        &frame,
        "--params",
        recipe.to_str().unwrap(),
        "-o",
        by_recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        std::fs::read(&by_flag).unwrap(),
        std::fs::read(&by_recipe).unwrap(),
        "the reported flag and the written recipe are one knob"
    );
}

#[test]
fn the_roll_exposure_adds_to_the_stated_one_and_direct_leaves_it_out() {
    // `nf-calibration/roll-exposure`: the roll's exposure is a neutral gain the stated
    // exposure adjusts, as the stated white balance adjusts the roll's gains.
    let tmp = TempDir::new("roll-exposure");
    let (code, stdout, err) = convert_48bit(
        &tmp.path("sum.tiff"),
        &["--roll-exposure", "0.5", "--exposure", "0.25"],
    );
    assert_eq!(code, 0, "{err}");
    let summed = json(&stdout);
    assert_eq!(
        summed["chain"]["scene_correction"]["exposure"], 0.75,
        "{summed}"
    );
    assert_eq!(summed["chain"]["roll"]["exposure"], 0.5, "{summed}");
    assert_eq!(
        summed["chain"]["roll"]["exposure_applied"], true,
        "{summed}"
    );
    let (code, _, err) = convert_48bit(&tmp.path("stated.tiff"), &["--exposure", "0.75"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        std::fs::read(tmp.path("sum.tiff")).unwrap(),
        std::fs::read(tmp.path("stated.tiff")).unwrap(),
        "the roll's exposure renders what the same sum stated renders"
    );

    // From a recipe: a stated exposure beside the roll's warns, since it may be one chosen
    // by hand before the roll's was measured; typed over it, it does not.
    let recipe = write_file(
        &tmp.path("recipe.json"),
        r#"{"recipe_version": 3, "roll": {"exposure": 0.5},
            "scene_correction": {"exposure": 0.25}}"#,
    );
    let (code, stdout, err) = convert_48bit(
        &tmp.path("replay.tiff"),
        &["--params", recipe.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{err}");
    let replay = json(&stdout);
    assert_eq!(
        replay["chain"]["scene_correction"]["exposure"], 0.75,
        "{replay}"
    );
    let warned = |report: &serde_json::Value| {
        report.get("warnings").is_some_and(|w| {
            w.as_array().unwrap().iter().any(|w| {
                w.as_str()
                    .unwrap()
                    .starts_with("the recipe's `scene_correction.exposure`")
            })
        })
    };
    assert!(warned(&replay), "{replay}");
    let (code, stdout, err) = convert_48bit(
        &tmp.path("typed.tiff"),
        &["--params", recipe.to_str().unwrap(), "--exposure", "0.25"],
    );
    assert_eq!(code, 0, "{err}");
    assert!(!warned(&json(&stdout)), "{stdout}");

    // `direct` leaves the recipe's roll exposure out, and refuses a typed one.
    let (code, stdout, err) = convert_48bit(
        &tmp.path("direct.tiff"),
        &[
            "--params",
            recipe.to_str().unwrap(),
            "--rendering",
            "direct",
        ],
    );
    assert_eq!(code, 0, "{err}");
    let direct = json(&stdout);
    assert_eq!(
        direct["chain"]["scene_correction"]["exposure"], 0.25,
        "{direct}"
    );
    assert_eq!(
        direct["chain"]["roll"]["exposure_applied"], false,
        "{direct}"
    );
    let (code, _, err) = convert_48bit(
        &tmp.path("direct-typed.tiff"),
        &["--rendering", "direct", "--roll-exposure", "0.5"],
    );
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains(
            "--roll-exposure applies the roll's measurements, but the rendering is `direct`"
        ),
        "{err}"
    );
}

#[test]
fn a_recipe_style_value_beside_the_roll_replays_as_stated_and_warns() {
    // Nothing is read as unset by its value: a `--dump-params` recipe replays exactly
    // what it rendered. A recipe white balance beside the roll's gains may be a leftover
    // an earlier `measure-roll` wrote there, so the run warns; a typed flag is a choice
    // made now, and never does. A contrast beside the roll's white is not a leftover —
    // it multiplies the roll's slope, and no earlier build wrote a multiplier — while a
    // version 2 recipe's contrast, which was the slope itself, is refused with its
    // conversion. `hdr-48bit.tif` is the IR-free fixture, so a `--strict` exit 1 is a
    // warning's.
    let tmp = TempDir::new("roll-overlap");
    let overlap = |warnings: &serde_json::Value, key: &str| {
        let prefix = format!("the recipe's `{key}`");
        warnings.as_array().map_or(0, |w| {
            w.iter()
                .filter(|w| w.as_str().unwrap().starts_with(&prefix))
                .count()
        })
    };
    let slope = |report: &serde_json::Value| report["chain"]["look"]["slope"].as_f64().unwrap();

    // Typed flags beside the roll's: no warning, and `--strict` passes. The contrast
    // multiplies the roll's slope.
    let dumped = tmp.path("dumped.json");
    let (code, stdout, err) = convert_48bit(
        &tmp.path("a.tiff"),
        &[
            "--roll-white-balance",
            "1.25,1,0.8",
            "--white-balance",
            "1.05,1,1",
            "--roll-white",
            "1.7",
            "--roll-exposure",
            "0",
            "--contrast",
            "1.2",
            "--dump-params",
            dumped.to_str().unwrap(),
            "--strict",
        ],
    );
    assert_eq!(code, 0, "a typed flag never warns: {err}");
    let first = json(&stdout);
    let look = &first["chain"]["look"];
    assert_eq!(look["contrast"], 1.2, "{first}");
    assert_eq!(look["base_from"], "roll", "{first}");
    let roll_slope = first["chain"]["roll"]["slope"].as_f64().unwrap();
    assert_eq!(look["base_slope"].as_f64().unwrap(), roll_slope, "{first}");
    assert!((slope(&first) - roll_slope * 1.2).abs() < 1e-6, "{first}");
    assert!(
        first
            .get("warnings")
            .is_none_or(|w| w.as_array().unwrap().is_empty()),
        "{first}"
    );

    // The dump, read back: the same render, and now both values are the recipe's. Only
    // the white balance warns; the roll's slope still applies under the contrast.
    let (code, stdout, err) =
        convert_48bit(&tmp.path("b.tiff"), &["--params", dumped.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    let replay = json(&stdout);
    assert_eq!(slope(&replay), slope(&first), "{replay}");
    assert_eq!(replay["chain"]["roll"]["slope_applied"], true);
    assert_eq!(overlap(&replay["warnings"], "look.contrast"), 0, "{replay}");
    assert_eq!(
        overlap(&replay["warnings"], "scene_correction.white_balance"),
        1,
        "{replay}"
    );
    assert_eq!(
        std::fs::read(tmp.path("a.tiff")).unwrap(),
        std::fs::read(tmp.path("b.tiff")).unwrap(),
        "the dump replays what it rendered"
    );
    // The same recipe with the white balance typed over it: the flag is the choice.
    let (code, _, err) = convert_48bit(
        &tmp.path("b2.tiff"),
        &[
            "--params",
            dumped.to_str().unwrap(),
            "--white-balance",
            "1.05,1,1",
            "--strict",
        ],
    );
    assert_eq!(code, 0, "{err}");

    let recipe = |version: u32, look: &str, scene: &str| {
        let path = tmp.path(&format!("r{version}-{}.json", look.len() + scene.len()));
        write_file(
            &path,
            &format!(
                r#"{{"recipe_version": {version},
                     "calibration": {{"film_base": {{"explicit": [0.9, 0.55, 0.42]}}}},
                     "roll": {{"white_balance": [1.25, 1.0, 0.8], "white_stops": 1.7,
                               "exposure": 0.0}},
                     "scene_correction": {{"white_balance": {{"explicit": {scene}}}}},
                     "look": {{"contrast": {look}}}}}"#
            ),
        )
    };
    // A version 2 recipe with a `roll` section merged in: its contrast was the slope,
    // so it is refused rather than multiplied, and the conversion it names renders it.
    let old = recipe(2, "1.1111112", "[1, 1, 1]");
    let (code, _, err) = convert_48bit(&tmp.path("c.tiff"), &["--params", old.to_str().unwrap()]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("`look.contrast` 1.1111112 in a `recipe_version` 2 recipe is the slope")
            && err.contains("from `roll.white_stops` 1.7"),
        "{err}"
    );
    let k = err
        .split("state `look.contrast` ")
        .nth(1)
        .and_then(|s| s.split(' ').next())
        .unwrap();
    let converted = recipe(3, k, "[1, 1, 1]");
    let (code, stdout, err) = convert_48bit(
        &tmp.path("c2.tiff"),
        &["--params", converted.to_str().unwrap(), "--strict"],
    );
    assert_eq!(code, 0, "{err}");
    assert!(
        (slope(&json(&stdout)) - 1.111_111_2).abs() < 1e-6,
        "{stdout}"
    );

    // Old gains still in `scene_correction.white_balance`: they multiply the roll's.
    let squared = recipe(3, "null", "[1.25, 1.0, 0.8]");
    let (code, _, err) = convert_48bit(
        &tmp.path("e.tiff"),
        &["--params", squared.to_str().unwrap(), "--strict"],
    );
    assert_eq!(code, 1, "--strict must promote the warning: {err}");
    assert!(
        err.contains("the recipe's `scene_correction.white_balance` [1.25, 1.0, 0.8] multiplies"),
        "{err}"
    );

    // The control: the migrated recipe — gains dropped — is quiet.
    let migrated = recipe(3, "null", "[1, 1, 1]");
    let (code, stdout, err) = convert_48bit(
        &tmp.path("f.tiff"),
        &["--params", migrated.to_str().unwrap(), "--strict"],
    );
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);
    assert_eq!(report["chain"]["roll"]["slope_applied"], true, "{report}");

    // `roll`: a shared-recipe fact, so it warns once for the roll, never per frame.
    let second = tmp.path("second.tif");
    std::fs::copy(fixture("hdr-48bit.tif"), &second).unwrap();
    let (code, stdout, err) = run(&[
        "roll",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        second.to_str().unwrap(),
        "--out-dir",
        tmp.path("roll").to_str().unwrap(),
        "--params",
        squared.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);
    let key = "scene_correction.white_balance";
    assert_eq!(overlap(&report["warnings"], key), 1, "{report}");
    let frames = report["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 2, "{report}");
    for frame in frames {
        assert_eq!(frame["status"], "ok", "{frame}");
        assert_eq!(overlap(&frame["warnings"], key), 0, "{frame}");
    }
}

#[test]
fn a_direct_dump_of_a_deliberate_adjustment_replays_under_strict() {
    // A file cannot say who chose a value, so `direct` warns only for what an earlier
    // build wrote unchosen (desaturation 0.8, a white balance beside a roll section): a
    // dump of a deliberate adjustment must replay under `--strict`, to the same bytes. `hdr-48bit.tif` is the IR-free fixture, so an exit 1 is a warning.
    let tmp = TempDir::new("direct-dump-replay");
    let dumped = tmp.path("d.json");
    let (code, _, err) = convert_48bit(
        &tmp.path("a.tiff"),
        &[
            "--rendering",
            "direct",
            "--highlight-desaturation",
            "0.5",
            "--strict",
            "--dump-params",
            dumped.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{err}");
    let (code, _, err) = convert_48bit(
        &tmp.path("b.tiff"),
        &["--params", dumped.to_str().unwrap(), "--strict"],
    );
    assert_eq!(code, 0, "the dump must replay under --strict: {err}");
    assert_eq!(
        std::fs::read(tmp.path("a.tiff")).unwrap(),
        std::fs::read(tmp.path("b.tiff")).unwrap()
    );
    // The old serialized default still warns, and `--strict` refuses it.
    let old = write_file(
        &tmp.path("old.json"),
        r#"{"recipe_version": 3, "rendering": "direct",
            "look": {"highlight_desaturation": {"strength": 0.8}}}"#,
    );
    let (code, _, err) = convert_48bit(
        &tmp.path("c.tiff"),
        &["--params", old.to_str().unwrap(), "--strict"],
    );
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("`look.highlight_desaturation.strength` 0.8"),
        "{err}"
    );

    // The carve-out: beside a recipe's `roll` section, a white balance typed now is
    // dumped into the recipe, and a file cannot say who chose it — so the replay warns
    // (as `default`'s overlap rule does). Typed again on replay, it is quiet. A contrast
    // is spared: no earlier build wrote the multiplier.
    let roll = write_file(
        &tmp.path("roll.json"),
        r#"{"recipe_version": 3, "roll": {"white_balance": [1.25, 1.0, 0.8], "white_stops": 1.7}}"#,
    );
    {
        let flag = &["--white-balance", "1.05,1,1"][..];
        let dump = tmp.path("roll-dump.json");
        let (code, _, err) = convert_48bit(
            &tmp.path("r1.tiff"),
            &[
                &[
                    "--params",
                    roll.to_str().unwrap(),
                    "--rendering",
                    "direct",
                    "--strict",
                    "--dump-params",
                    dump.to_str().unwrap(),
                ][..],
                flag,
            ]
            .concat(),
        );
        assert_eq!(code, 0, "{flag:?}: typed, it never warns: {err}");
        let (code, _, err) = convert_48bit(
            &tmp.path("r2.tiff"),
            &["--params", dump.to_str().unwrap(), "--strict"],
        );
        assert_eq!(code, 1, "{flag:?}: the replay warns: {err}");
        assert!(
            err.contains("beside a `roll` section")
                && err.contains("type it as a flag to keep it without this warning"),
            "{flag:?}: {err}"
        );
        let (code, _, err) = convert_48bit(
            &tmp.path("r3.tiff"),
            &[&["--params", dump.to_str().unwrap(), "--strict"][..], flag].concat(),
        );
        assert_eq!(code, 0, "{flag:?}: typed on replay, it is quiet: {err}");
    }
    let dump = tmp.path("contrast-dump.json");
    let (code, _, err) = convert_48bit(
        &tmp.path("k1.tiff"),
        &[
            "--params",
            roll.to_str().unwrap(),
            "--rendering",
            "direct",
            "--contrast",
            "1.3",
            "--dump-params",
            dump.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{err}");
    let (code, _, err) = convert_48bit(
        &tmp.path("k2.tiff"),
        &["--params", dump.to_str().unwrap(), "--strict"],
    );
    assert_eq!(code, 0, "a dumped contrast replays quiet: {err}");
    assert_eq!(
        std::fs::read(tmp.path("k1.tiff")).unwrap(),
        std::fs::read(tmp.path("k2.tiff")).unwrap()
    );
}

#[test]
fn a_roll_white_without_an_exposure_warns_until_one_is_stated() {
    // Recipe or flag, a roll white or gains without `roll.exposure` render at exposure 0
    // and warn; the fallback warning's remedy (state `roll.white_stops`) names the
    // exposure too, so following it is quiet. The IR-free fixture, so `--strict` sees
    // only these warnings.
    let tmp = TempDir::new("roll-no-exposure");
    let white_only = write_file(
        &tmp.path("white.json"),
        r#"{"recipe_version": 3, "roll": {"white_stops": 1.7},
            "scene_correction": {"white_balance": {"explicit": [1.1, 1, 0.9]}}}"#,
    );
    let recipe = ["--params", white_only.to_str().unwrap()];
    let typed = ["--roll-white", "1.7", "--white-balance", "1.1,1,0.9"];
    for source in [&recipe[..], &typed[..]] {
        let (code, _, err) = convert_48bit(&tmp.path("w.tiff"), &[source, &["--strict"]].concat());
        assert_eq!(code, 1, "{source:?}: {err}");
        assert!(
            err.contains("the roll section has no `roll.exposure`")
                && err.contains("`--roll-exposure` (0 keeps this render)")
                && !err.contains("measured before")
                && !err.contains("no roll measurement"),
            "{source:?}: {err}"
        );
        // The remedy, followed: a stated exposure of 0 is quiet.
        let (code, _, err) = convert_48bit(
            &tmp.path("e.tiff"),
            &[source, &["--roll-exposure", "0", "--strict"]].concat(),
        );
        assert_eq!(code, 0, "{source:?}: {err}");
    }
}

#[test]
fn the_roll_flags_are_refused_under_the_direct_rendering() {
    // `direct` leaves the roll out, so a typed roll flag would be silently ignored —
    // refused whether a flag or the recipe chose `direct`, before any value rule, and
    // with a remedy that works. A recipe's `roll` section is spared.
    let tmp = TempDir::new("roll-flag-direct");
    let direct = write_file(
        &tmp.path("direct.json"),
        r#"{"recipe_version": 3, "rendering": "direct",
            "roll": {"white_balance": [1.25, 1.0, 0.8], "white_stops": 1.7}}"#,
    );
    let params = ["--params", direct.to_str().unwrap()];
    for (source, extra) in [
        (
            &["--rendering", "direct"][..],
            &["--roll-white", "1.7", "--strict"][..],
        ),
        (&params[..], &["--roll-white-balance", "1.3,1,0.8"][..]),
        // A bad value still gets the presence refusal, not the value rule's remedy.
        (&params[..], &["--roll-white", "1e-45"][..]),
    ] {
        let (code, _, err) = convert_48bit(&tmp.path("d.tiff"), &[source, extra].concat());
        assert_eq!(code, 2, "{extra:?}: {err}");
        assert!(
            err.contains(&format!("{} applies the roll's measurements", extra[0]))
                && err.contains("the rendering is `direct`")
                && err.contains(&format!("drop {}", extra[0]))
                && err.contains("pass --rendering default"),
            "{extra:?}: {err}"
        );
        assert!(
            !err.contains("larger white") && !err.contains("must give a finite"),
            "{extra:?}: {err}"
        );
    }
    // The remedy works over the recipe's `direct`, and applies the roll.
    let (code, stdout, err) = convert_48bit(
        &tmp.path("default.tiff"),
        &[
            &params[..],
            &["--roll-white", "1.7", "--rendering", "default"],
        ]
        .concat(),
    );
    assert_eq!(code, 0, "{err}");
    assert_eq!(json(&stdout)["chain"]["roll"]["slope_applied"], true);
    // The recipe's own section alone is spared.
    let (code, _, err) = convert_48bit(&tmp.path("spared.tiff"), &params);
    assert_eq!(code, 0, "{err}");
    // Film master and `direct` both: the film master is named, with a remedy that
    // clears `direct` too.
    let both = write_file(
        &tmp.path("both.json"),
        r#"{"recipe_version": 3, "rendering": "direct", "output": "film-master"}"#,
    );
    let (code, _, err) = convert_48bit(
        &tmp.path("both.tiff"),
        &["--params", both.to_str().unwrap(), "--roll-white", "1.7"],
    );
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("the recipe's `output` is \"film-master\"")
            && err.contains("Either drop --roll-white and pass --rendering default, or")
            && err.contains("with --rendering default"),
        "{err}"
    );
    assert!(!err.contains("the rendering is `direct`"), "{err}");
    // Each offered remedy, followed as written, converts: the first writes the film
    // master under `default`, the second a rendered destination applying the roll.
    let (code, _, err) = convert_48bit(
        &tmp.path("both-dropped.tiff"),
        &["--params", both.to_str().unwrap(), "--rendering", "default"],
    );
    assert_eq!(code, 0, "{err}");
    let (code, _, err) = convert_48bit(
        &tmp.path("both-fixed.tiff"),
        &[
            "--params",
            both.to_str().unwrap(),
            "--roll-white",
            "1.7",
            "--range",
            "sdr",
            "--rendering",
            "default",
        ],
    );
    assert_eq!(code, 0, "{err}");
}

#[test]
fn the_roll_flags_are_refused_under_the_film_master() {
    // A roll flag asks a rendering to apply a measurement, which the film master never
    // does. A typed `--film-master` conflicts at the parser, like the destination axes;
    // a recipe's film master is refused by the presence rule, with a remedy the command
    // accepts. A recipe's `roll` section is spared
    // (`the_film_master_leaves_the_roll_section_unapplied_and_says_so`).
    let tmp = TempDir::new("roll-flag-film-master");
    let master = write_file(
        &tmp.path("master.json"),
        r#"{"recipe_version": 3, "output": "film-master",
            "roll": {"white_balance": [1.25, 1.0, 0.8], "white_stops": 1.7}}"#,
    );
    for flag in [
        &["--roll-white-balance", "1.3,1,0.8"][..],
        &["--roll-white", "1.7"][..],
    ] {
        let (code, _, err) = convert_48bit(
            &tmp.path("m.tiff"),
            &[&["--film-master"][..], flag].concat(),
        );
        assert_eq!(code, 2, "{flag:?}: {err}");
        assert!(
            err.contains("--film-master")
                && err.contains(flag[0])
                && err.contains("cannot be used"),
            "{flag:?}: {err}"
        );

        // `.jpg` also breaks the film master's suffix rule, a coarser diagnosis.
        let (code, _, err) = convert_48bit(
            &tmp.path("m.jpg"),
            &[&["--params", master.to_str().unwrap()][..], flag].concat(),
        );
        assert_eq!(code, 2, "{flag:?}: {err}");
        assert!(
            err.contains(&format!("{} applies the roll's measurements", flag[0]))
                && err.contains("the recipe's `output` is \"film-master\"")
                && err.contains(&format!("drop {}", flag[0]))
                && err.contains("choose a rendered destination"),
            "{flag:?}: {err}"
        );
        // The more specific diagnosis wins over the suffix rule.
        assert!(!err.contains("does not end in"), "{flag:?}: {err}");
        // The remedy works: a rendered destination applies the flag.
        let (code, stdout, err) = convert_48bit(
            &tmp.path("rendered.tiff"),
            &[
                &["--params", master.to_str().unwrap(), "--range", "sdr"][..],
                flag,
            ]
            .concat(),
        );
        assert_eq!(code, 0, "{flag:?}: {err}");
        assert!(json(&stdout)["chain"]["roll"].is_object());
    }
    // The presence rule runs before every value rule that could refuse first: a bad
    // roll value, or a typed look knob the film master refuses, would each be fixed
    // only to meet this refusal.
    for (extra, losing) in [
        (
            &["--roll-white", "1e-45"][..],
            &["larger white", "must give a finite"][..],
        ),
        (
            &["--roll-white-balance", "0,1,1"][..],
            &["must be finite and positive"][..],
        ),
        (
            &["--roll-white", "1.7", "--contrast", "1.3"][..],
            &["cannot apply", "--contrast"][..],
        ),
    ] {
        let (code, _, err) = convert_48bit(
            &tmp.path("m.tiff"),
            &[&["--params", master.to_str().unwrap()][..], extra].concat(),
        );
        assert_eq!(code, 2, "{extra:?}: {err}");
        assert!(
            err.contains("the recipe's `output` is \"film-master\"") && err.contains(extra[0]),
            "{extra:?}: {err}"
        );
        for wording in losing {
            assert!(
                !err.contains(wording),
                "{extra:?} lost to {wording:?}: {err}"
            );
        }
    }
    // The recipe's own section alone is spared.
    let (code, _, err) = convert_48bit(
        &tmp.path("spared.tiff"),
        &["--params", master.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{err}");
}

#[test]
fn measure_roll_keeps_a_fully_exposed_frame_out_of_the_white() {
    // A roll of the picture fixture plus one fully exposed frame — a copy of the
    // leader, mixed in (the leader itself as an input is refused below). Guarded by
    // `--leader`, the gains are the picture's alone; unguarded, the exposed frame
    // becomes the roll's white.
    let tmp = TempDir::new("measure-roll-guard");
    let recipe = roll_white_recipe(&tmp, "0.9,0.55,0.42");
    let leader = tmp.path("leader.tif");
    // Dense, cast, and IR-transparent (so no holder is measured).
    write_hdri_with_uniform_ir(&leader, 64, 64, [900, 700, 400], 40_000);
    let blown = tmp.path("blown.tif");
    std::fs::copy(&leader, &blown).unwrap();
    let (frame, other) = (tmp.path("frame.tif"), tmp.path("other.tif"));
    std::fs::copy(fixture("hdr-48bit.tif"), &frame).unwrap();
    std::fs::copy(fixture("hdr-48bit.tif"), &other).unwrap();
    let (frame, other, leader, blown) = (
        frame.to_str().unwrap(),
        other.to_str().unwrap(),
        leader.to_str().unwrap(),
        blown.to_str().unwrap(),
    );
    let params = recipe.to_str().unwrap();
    let gains = |args: &[&str]| {
        let (code, stdout, err) = run(&[&["measure-roll", "--params", params][..], args].concat());
        assert_eq!(code, 0, "{args:?}: {err}");
        json(&stdout)
    };

    let clean = gains(&[frame, other, "--leader", leader]);
    let guarded = gains(&[frame, other, blown, "--leader", leader]);
    let unguarded = gains(&[frame, other, blown]);
    assert_eq!(
        guarded["white_balance"]["gains"], clean["white_balance"]["gains"],
        "{guarded}"
    );
    assert_eq!(guarded["frames"][2]["kept"], 0, "{guarded}");
    assert!(
        guarded["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("contributed no pixel")),
        "{guarded}"
    );
    assert_ne!(
        unguarded["white_balance"]["gains"], clean["white_balance"]["gains"],
        "the guard must be what keeps it out: {unguarded}"
    );
    let ceiling = &clean["leader"]["ceiling"];
    assert!(
        ceiling.is_array() && clean["leader"]["guard_density"] == 0.1,
        "{clean}"
    );

    // The leader itself among the frames is refused: its unguarded edges would pool.
    let (code, _, err) = run(&[
        "measure-roll",
        "--params",
        params,
        frame,
        leader,
        "--leader",
        leader,
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("is both the --leader and an input frame"),
        "{err}"
    );
}

/// A uniform frame whose film density is `d` on every channel over `base`. Red carries
/// the largest density scale, so above mid-grey it is the brightest channel, and the
/// frame's white is red's `(d − 0.62) · 1.8 / log10 2` scene stops above mid-grey.
fn write_uniform_density(path: &Path, base: [f32; 3], d: f32) {
    let raw = base.map(|b| (b * 10f32.powf(-d) * 65535.0).round() as u16);
    write_hdri_with_uniform_ir(path, 64, 64, raw, 40_000);
}

#[test]
fn measure_roll_places_the_white_and_clamps_a_frame_above_the_cap() {
    // A dim frame (~+1.7 stops) and a bright one (~+3.0): the roll's white is the dim
    // frame's, and the bright one is clamped to the cap — disclosed with both contrasts,
    // and handed to `roll` as a per-frame contrast. It warns only when its leader is
    // near: a frame merely above the cap is an ordinary bright scene.
    let tmp = TempDir::new("measure-roll-white");
    let base = [0.9f32, 0.55, 0.42];
    let recipe = roll_white_recipe(&tmp, "0.9,0.55,0.42");
    let (dim, bright) = (tmp.path("dim.tif"), tmp.path("bright.tif"));
    write_uniform_density(&dim, base, 0.904);
    write_uniform_density(&bright, base, 1.122);
    let (near, far) = (tmp.path("near.tif"), tmp.path("far.tif"));
    // The near leader is 0.1 stop over the bright frame — inside the guard, which would
    // leave the frame no pixel if its white were measured after it.
    write_uniform_density(&near, base, 1.139);
    write_uniform_density(&far, base, 1.5);
    let measure = |leader: &Path| {
        let (code, stdout, err) = run(&[
            "measure-roll",
            "--params",
            recipe.to_str().unwrap(),
            dim.to_str().unwrap(),
            bright.to_str().unwrap(),
            "--leader",
            leader.to_str().unwrap(),
        ]);
        assert_eq!(code, 0, "{err}");
        json(&stdout)
    };
    let saturation = |r: &serde_json::Value| {
        r["warnings"]
            .as_array()
            .map(|w| {
                w.iter()
                    .filter(|w| w.as_str().unwrap().contains("near film saturation"))
                    .map(|w| w.as_str().unwrap().to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };

    let report = measure(&far);
    let white = &report["white"];
    let stops = |i: usize| report["frames"][i]["white_stops"].as_f64().unwrap();
    assert!((1.5..2.0).contains(&stops(0)), "{report}");
    assert!(stops(1) > 2.0, "{report}");
    assert_eq!(white["bound"], "none", "{report}");
    assert_eq!(white["stops"], report["frames"][0]["white_stops"]);
    assert_eq!(white["from"], dim.to_str().unwrap());
    assert_eq!(report["frames"][0]["white_role"], "sets_roll");
    assert_eq!(report["frames"][1]["white_role"], "clamped");
    let roll_slope = white["slope"].as_f64().unwrap();
    let clamped = &white["clamped"][0];
    assert_eq!(clamped["input"], bright.to_str().unwrap());
    let cap_slope = clamped["slope"].as_f64().unwrap();
    assert!(
        clamped["flag"].as_str().unwrap().contains(&format!(
            "--roll-white {} ",
            white["rule"]["cap_stops"].as_f64().unwrap() as f32
        )),
        "a clamped frame's own flag carries the cap as its white: {report}"
    );
    assert!(
        (cap_slope * 1.8 - 2.23).abs() < 0.01 && roll_slope > cap_slope,
        "{report}"
    );
    assert!(
        saturation(&report).is_empty(),
        "above the cap is not a warning: {report}"
    );
    assert!(
        report["frames"][1]["leader_distance_stops"]
            .as_f64()
            .unwrap()
            > 0.5,
        "{report}"
    );

    let report_near = measure(&near);
    let warned = saturation(&report_near);
    assert_eq!(warned.len(), 1, "{report_near}");
    assert!(
        warned[0].starts_with(bright.to_str().unwrap()),
        "{warned:?}"
    );
    assert_eq!(
        report_near["frames"][1]["kept"], 0,
        "the guard took every pixel, and the frame still warned: {report_near}"
    );
    assert_eq!(
        report_near["frames"][1]["white_stops"],
        report["frames"][1]["white_stops"]
    );
    // An emptied frame is not picture, so it has no say in the roll's exposure.
    assert!(
        report["frames"][1].get("level_stops").is_some()
            && report_near["frames"][1].get("level_stops").is_none(),
        "{report_near}"
    );
    assert_eq!(
        report_near["exposure"]["level_stops"], report_near["frames"][0]["level_stops"],
        "{report_near}"
    );

    // The reuse forms: the roll's white by flag, and the whole measurement — the
    // clamped frame's own white (the cap) included — as the `--out` recipe, which
    // `roll` renders with no manifest. Without lifts, so the table holds the clamp alone
    // (`measure_roll_lifts_a_low_key_frame_…` covers them).
    assert!(
        report["reuse"]["flag"]
            .as_str()
            .unwrap()
            .ends_with(&format!(
                "--roll-white {} --roll-exposure {}",
                white["stops"].as_f64().unwrap() as f32,
                report["exposure"]["ev"].as_f64().unwrap() as f32
            )),
        "{report}"
    );
    assert!(report["reuse"].get("frames").is_none(), "{report}");
    let measured = tmp.path("measured.json");
    let (code, _, err) = run(&[
        "measure-roll",
        "--params",
        recipe.to_str().unwrap(),
        dim.to_str().unwrap(),
        bright.to_str().unwrap(),
        "--leader",
        far.to_str().unwrap(),
        "--out",
        measured.to_str().unwrap(),
        "--no-frame-lift",
    ]);
    assert_eq!(code, 0, "{err}");
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&measured).unwrap()).unwrap();
    assert_eq!(written["roll"]["white_stops"], white["stops"], "{written}");
    assert_eq!(
        written["roll"]["frames"],
        serde_json::json!({"bright.tif": {"white_stops": white["rule"]["cap_stops"],
            "exposure": null}}),
        "keyed by file name, the clamped frame only (`--no-frame-lift`): {written}"
    );
    // The input it was decoded under travels too: these scans state no transfer.
    assert_eq!(written["input"]["transfer"], "linear", "{written}");
    let out = tmp.path("out");
    let (code, stdout, err) = run(&[
        "roll",
        dim.to_str().unwrap(),
        bright.to_str().unwrap(),
        "--params",
        measured.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    let rolled = json(&stdout);
    // `roll` sorts positional inputs, so find each frame by name.
    let rendered = |path: &Path| {
        rolled["frames"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["input"] == path.to_str().unwrap())
            .map(|f| f["chain"]["look"]["slope"].clone())
    };
    assert_eq!(rendered(&dim).as_ref(), Some(&white["slope"]), "{rolled}");
    assert_eq!(
        rendered(&bright).as_ref(),
        Some(&clamped["slope"]),
        "{rolled}"
    );

    // The same pixels as the route it replaces: the base and the roll section merged
    // by hand, and the clamp handed over as a `--frames` manifest.
    let mut shared: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&recipe).unwrap()).unwrap();
    shared["roll"] = serde_json::json!({
        "white_balance": written["roll"]["white_balance"],
        "white_stops": written["roll"]["white_stops"],
        "exposure": written["roll"]["exposure"],
    });
    let shared = write_file(&tmp.path("shared.json"), &shared.to_string());
    let manifest = write_file(
        &tmp.path("frames.json"),
        &serde_json::json!({"frames": [
            {"input": dim},
            {"input": bright, "params": {"roll": {"white_stops": white["rule"]["cap_stops"]}}},
        ]})
        .to_string(),
    );
    let by_manifest = tmp.path("by-manifest");
    let (code, _, err) = run(&[
        "roll",
        "--params",
        shared.to_str().unwrap(),
        "--frames",
        manifest.to_str().unwrap(),
        "-o",
        by_manifest.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    for name in ["dim_positive.tiff", "bright_positive.tiff"] {
        assert_eq!(
            std::fs::read(out.join(name)).unwrap(),
            std::fs::read(by_manifest.join(name)).unwrap(),
            "{name}: the recipe's table renders what the manifest did"
        );
    }

    // A single `convert` of the clamped frame takes its entry too, by file name.
    let single = tmp.path("single.tiff");
    let (code, _, err) = run(&[
        "convert",
        bright.to_str().unwrap(),
        "--params",
        measured.to_str().unwrap(),
        "-o",
        single.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        std::fs::read(&single).unwrap(),
        std::fs::read(out.join("bright_positive.tiff")).unwrap(),
        "a roll frame is byte-identical to a single convert of it"
    );

    // A manifest's `params` beat the table; a table in a manifest is refused.
    let beats = write_file(
        &tmp.path("beats.json"),
        &serde_json::json!({"frames": [
            {"input": bright, "params": {"roll": {"white_stops": white["stops"]}}},
        ]})
        .to_string(),
    );
    let (code, stdout, err) = run(&[
        "roll",
        "--params",
        measured.to_str().unwrap(),
        "--frames",
        beats.to_str().unwrap(),
        "-o",
        tmp.path("beats").to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    let beaten = json(&stdout);
    assert_eq!(
        beaten["frames"][0]["chain"]["look"]["slope"],
        white["slope"]
    );
    // A clamp is how a manifest states a frame's own white, not a roll-wide break.
    assert!(
        !beaten.to_string().contains("override resolves"),
        "{beaten}"
    );
    let nested = write_file(
        &tmp.path("nested.json"),
        &serde_json::json!({"frames": [
            {"input": bright, "params": {"roll": {"frames": {"bright.tif": {"white_stops": 2.0}}}}},
        ]})
        .to_string(),
    );
    let (code, _, err) = run(&[
        "roll",
        "--params",
        measured.to_str().unwrap(),
        "--frames",
        nested.to_str().unwrap(),
        "-o",
        tmp.path("nested").to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("belongs in the shared recipe"), "{err}");

    // A roll whose every frame is above the cap lands on the cap: its frames are still
    // disclosed as clamped, but they render at the roll's own contrast, so the table
    // names no frame (without lifts: the roll's exposure binds at -2 EV, so this one
    // frame would be lifted).
    let capped_recipe = tmp.path("capped.json");
    let (code, stdout, err) = run(&[
        "measure-roll",
        "--params",
        recipe.to_str().unwrap(),
        bright.to_str().unwrap(),
        "--leader",
        far.to_str().unwrap(),
        "--out",
        capped_recipe.to_str().unwrap(),
        "--no-frame-lift",
    ]);
    assert_eq!(code, 0, "{err}");
    let capped = json(&stdout);
    assert_eq!(capped["white"]["bound"], "cap", "{capped}");
    assert_eq!(capped["white"]["slope"], clamped["slope"], "{capped}");
    assert_eq!(capped["frames"][0]["white_role"], "clamped", "{capped}");
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&capped_recipe).unwrap()).unwrap();
    assert_eq!(
        written["roll"]["frames"],
        serde_json::json!({}),
        "{written}"
    );
}

#[test]
fn measure_roll_lifts_a_low_key_frame_and_either_opt_out_drops_it() {
    // `nf-calibration/frame-level-trim`: a night frame (~-1 stop), an ordinary one (~0)
    // and a bright one (~+2.9) on one roll. The lift follows where each white renders
    // after the roll's exposure: the night frame is held at the bound, the bright one is
    // not lifted, and the report names each. Off at measurement (`--no-frame-lift`) and
    // off at render (`--frame-lift off`, `roll.frame_lift`) render alike.
    let tmp = TempDir::new("measure-roll-lift");
    let base = [0.9f32, 0.55, 0.42];
    let recipe = roll_white_recipe(&tmp, "0.9,0.55,0.42");
    let (night, mid, bright, leader) = (
        tmp.path("night.tif"),
        tmp.path("mid.tif"),
        tmp.path("bright.tif"),
        tmp.path("leader.tif"),
    );
    write_uniform_density(&night, base, 0.45);
    write_uniform_density(&mid, base, 0.62);
    write_uniform_density(&bright, base, 1.1);
    write_uniform_density(&leader, base, 1.5);
    let s = |p: &Path| p.to_str().unwrap().to_owned();
    let measure = |out: &Path, extra: &[&str]| {
        let mut argv = vec![
            "measure-roll".to_owned(),
            "--params".to_owned(),
            s(&recipe),
            s(&night),
            s(&mid),
            s(&bright),
            "--leader".to_owned(),
            s(&leader),
            "--out".to_owned(),
            s(out),
        ];
        argv.extend(extra.iter().map(|a| a.to_string()));
        let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
        let (code, stdout, err) = run(&argv);
        assert_eq!(code, 0, "{err}");
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(out).unwrap()).unwrap();
        (json(&stdout), written)
    };

    let lifted = tmp.path("lifted.json");
    let (report, written) = measure(&lifted, &[]);
    let lift = |i: usize| report["frames"][i]["lift_ev"].as_f64().unwrap();
    let bound = report["frame_lift"]["bound_ev"].as_f64().unwrap();
    assert!(
        (lift(0) - bound).abs() < 1e-6,
        "held at the bound: {report}"
    );
    assert!(lift(1) > 0.0 && lift(1) <= bound, "{report}");
    assert_eq!(lift(2), 0.0, "a bright frame is not lifted: {report}");
    assert_eq!(report["frame_lift"]["written"], true, "{report}");
    assert_eq!(report["frame_lift"]["lifted"], 2, "{report}");
    let frames = &written["roll"]["frames"];
    assert_eq!(
        frames["night.tif"]["exposure"],
        report["frames"][0]["lift_ev"]
    );
    assert_eq!(
        frames["mid.tif"]["exposure"],
        report["frames"][1]["lift_ev"]
    );
    assert!(
        frames["bright.tif"]["exposure"].is_null(),
        "only its clamp: {written}"
    );
    assert!(
        report["frames"][0]["flag"]
            .as_str()
            .unwrap()
            .ends_with(&format!("--roll-frame-exposure {}", lift(0) as f32))
    );

    // `roll` reports each frame's lift as applied.
    let out = tmp.path("out");
    let (code, stdout, err) = run(&[
        "roll",
        &s(&night),
        &s(&mid),
        &s(&bright),
        "--params",
        &s(&lifted),
        "-o",
        &s(&out),
    ]);
    assert_eq!(code, 0, "{err}");
    let rolled = json(&stdout);
    let chain_roll = |path: &Path| {
        rolled["frames"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["input"] == path.to_str().unwrap())
            .map(|f| f["chain"]["roll"].clone())
            .unwrap()
    };
    assert_eq!(
        chain_roll(&night)["frame_exposure"],
        report["frames"][0]["lift_ev"],
        "{rolled}"
    );
    assert_eq!(chain_roll(&night)["frame_exposure_applied"], true);
    assert!(chain_roll(&bright)["frame_exposure"].is_null(), "{rolled}");

    // Off three ways, one picture: measured without lifts, turned off by flag, and
    // turned off by a recipe layer.
    let unlifted = tmp.path("unlifted.json");
    let (report_off, written_off) = measure(&unlifted, &["--no-frame-lift"]);
    assert_eq!(report_off["frame_lift"]["written"], false, "{report_off}");
    assert_eq!(
        report_off["frames"][0]["lift_ev"], report["frames"][0]["lift_ev"],
        "still reported: {report_off}"
    );
    assert!(
        written_off["roll"]["frames"].get("night.tif").is_none(),
        "{written_off}"
    );
    let off_layer = write_file(
        &tmp.path("off.json"),
        r#"{"recipe_version": 3, "roll": {"frame_lift": "off"}}"#,
    );
    let convert = |name: &str, extra: &[&str]| {
        let output = tmp.path(name);
        let mut argv = vec![
            "convert",
            night.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ];
        argv.extend_from_slice(extra);
        let (code, stdout, err) = run(&argv);
        assert_eq!(code, 0, "{err}");
        (std::fs::read(&output).unwrap(), json(&stdout))
    };
    let (on, on_report) = convert("on.tiff", &["--params", &s(&lifted)]);
    let (never, _) = convert("never.tiff", &["--params", &s(&unlifted)]);
    let (by_flag, flag_report) = convert(
        "flag.tiff",
        &["--params", &s(&lifted), "--frame-lift", "off"],
    );
    let (by_key, _) = convert(
        "key.tiff",
        &["--params", &s(&lifted), "--params", &s(&off_layer)],
    );
    assert_ne!(on, never, "not vacuous: the lift moved the picture");
    assert_eq!(
        by_flag, never,
        "`--frame-lift off` renders the unlifted recipe"
    );
    assert_eq!(
        by_key, never,
        "`roll.frame_lift` off renders the unlifted recipe"
    );
    // The measured file states no `frame_lift`, so layered last it keeps an earlier off.
    assert!(written["roll"]["frame_lift"].is_null(), "{written}");
    let (key_first, _) = convert(
        "key-first.tiff",
        &["--params", &s(&off_layer), "--params", &s(&lifted)],
    );
    assert_eq!(
        key_first, never,
        "an off layer before the measured file holds"
    );
    assert_eq!(
        on_report["chain"]["roll"]["frame_exposure_applied"], true,
        "{on_report}"
    );
    assert_eq!(
        flag_report["chain"]["roll"]["frame_exposure"], report["frames"][0]["lift_ev"],
        "kept in the recipe: {flag_report}"
    );
    assert_eq!(
        flag_report["chain"]["roll"]["frame_exposure_applied"], false,
        "{flag_report}"
    );
}

#[test]
fn frame_lift_off_is_spared_where_nothing_lifts_and_roll_refuses_a_shared_frame_exposure() {
    let tmp = TempDir::new("frame-lift-rules");
    // `off` asks for nothing, so neither `direct` nor the film master refuses it.
    for (name, extra) in [
        ("direct.tiff", &["--rendering", "direct"][..]),
        ("master.tiff", &["--film-master"][..]),
    ] {
        let (code, _, err) =
            convert_48bit(&tmp.path(name), &[extra, &["--frame-lift", "off"]].concat());
        assert_eq!(code, 0, "{name}: {err}");
    }
    // `on` asks for a lift the film master would ignore, and is named as typed.
    let (code, _, err) = convert_48bit(
        &tmp.path("on.tiff"),
        &["--film-master", "--frame-lift", "on"],
    );
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains(
            "--frame-lift on applies the roll's measurements through the rendering \
                      stages, but --film-master writes"
        ) && err.contains("drop --film-master"),
        "{err}"
    );
    assert!(!err.contains("recipe's `output`"), "{err}");

    // On `roll`, one frame's exposure stated for all lifts every frame alike.
    let input = fixture("hdr-48bit.tif");
    let input = input.to_str().unwrap();
    let out = tmp.path("out");
    let (code, _, err) = run(&[
        "roll",
        input,
        "--film-base",
        "0.9,0.55,0.42",
        "--roll-frame-exposure",
        "0.2",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("--roll-frame-exposure is one frame's own exposure, and on `roll`"),
        "{err}"
    );
    let dumped = write_file(
        &tmp.path("dumped.json"),
        r#"{"recipe_version": 3, "roll": {"exposure": 0.5, "frame_exposure": 0.2}}"#,
    );
    let (code, _, err) = run(&[
        "roll",
        input,
        "--film-base",
        "0.9,0.55,0.42",
        "--params",
        dumped.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("the recipe's `roll.frame_exposure` is one frame's own exposure"),
        "{err}"
    );
}

#[test]
fn measure_roll_unexposed_measures_the_base_and_writes_the_whole_roll() {
    // `measure-roll --unexposed` then `roll --params` renders what the route it replaces
    // rendered: `measure-base` on the same frame, `measure-roll` over that base,
    // the parts merged by hand and the clamp handed over as a manifest.
    let tmp = TempDir::new("measure-roll-unexposed");
    let base = [0.9f32, 0.55, 0.42];
    // Synthetic scans state their input; the base is what is being measured.
    let input = write_file(
        &tmp.path("input.json"),
        r#"{ "recipe_version": 3,
             "input": { "transfer": "linear", "meaning": "scanner-device" } }"#,
    );
    let (blank, dim, bright, leader) = (
        tmp.path("blank.tif"),
        tmp.path("dim.tif"),
        tmp.path("bright.tif"),
        tmp.path("leader.tif"),
    );
    write_uniform_density(&blank, base, 0.0);
    write_uniform_density(&dim, base, 0.904);
    write_uniform_density(&bright, base, 1.122);
    write_uniform_density(&leader, base, 1.5);
    let s = |p: &Path| p.to_str().unwrap().to_owned();
    let frames = [s(&dim), s(&bright)];

    let measured = tmp.path("roll.json");
    let (blank_s, leader_s, input_s, measured_s) = (s(&blank), s(&leader), s(&input), s(&measured));
    let (code, stdout, err) = run(&[
        "measure-roll",
        &frames[0],
        &frames[1],
        "--unexposed",
        &blank_s,
        "--leader",
        &leader_s,
        "--params",
        &input_s,
        "--out",
        &measured_s,
    ]);
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);

    // One base measurement, one report shape: the `unexposed` object is exactly
    // `measure-base`'s evidence for the same frame.
    let (code, stdout, err) = run(&["measure-base", &s(&blank)]);
    assert_eq!(code, 0, "{err}");
    let alone = json(&stdout);
    let unexposed = report["unexposed"]
        .as_object()
        .expect("an unexposed object");
    for key in [
        "film_base",
        "film_base_source",
        "film_base_percentile",
        "film_base_flag",
        "effective_area",
    ] {
        assert!(unexposed.contains_key(key), "{key}: {report}");
    }
    for (key, value) in unexposed {
        assert_eq!(&alone[key], value, "`unexposed.{key}` matches measure-base");
    }
    assert_eq!(
        report["film_base"], alone["film_base"],
        "every frame decodes with it"
    );

    let out = tmp.path("out");
    let (code, _, err) = run(&[
        "roll",
        &frames[0],
        &frames[1],
        "--params",
        &s(&measured),
        "-o",
        &s(&out),
    ]);
    assert_eq!(code, 0, "{err}");

    // The replaced route.
    let flag = alone["film_base_flag"].as_str().unwrap();
    let flag_value = flag.strip_prefix("--film-base ").unwrap();
    let (code, stdout, err) = run(&[
        "measure-roll",
        &frames[0],
        &frames[1],
        "--leader",
        &s(&leader),
        "--params",
        &s(&input),
        "--film-base",
        flag_value,
    ]);
    assert_eq!(code, 0, "{err}");
    let by_hand = json(&stdout);
    assert_eq!(by_hand["white_balance"], report["white_balance"]);
    let explicit = alone["film_base"].clone();
    let shared = write_file(
        &tmp.path("shared.json"),
        &serde_json::json!({
            "recipe_version": 3,
            "input": { "transfer": "linear", "meaning": "scanner-device" },
            "calibration": {"film_base": {"explicit": [explicit["r"], explicit["g"], explicit["b"]]}},
            "roll": {
                "white_balance": by_hand["white_balance"]["gains"],
                "white_stops": by_hand["white"]["stops"],
                "exposure": by_hand["exposure"]["ev"],
            },
        })
        .to_string(),
    );
    let clamped = by_hand["white"]["clamped"].as_array().unwrap();
    assert_eq!(
        clamped.len(),
        1,
        "not vacuous: one frame is clamped: {by_hand}"
    );
    // Each frame's lift as its own `roll.frame_exposure`; both frames are lifted here.
    let lift = |i: usize| by_hand["frames"][i]["lift_ev"].clone();
    assert!(
        (0..2).all(|i| lift(i).as_f64().unwrap() > 0.0),
        "not vacuous: {by_hand}"
    );
    let manifest = write_file(
        &tmp.path("frames.json"),
        &serde_json::json!({"frames": [
            {"input": dim, "params": {"roll": {"frame_exposure": lift(0)}}},
            {"input": bright, "params": {"roll": {
                "white_stops": by_hand["white"]["rule"]["cap_stops"],
                "frame_exposure": lift(1),
            }}},
        ]})
        .to_string(),
    );
    let old = tmp.path("old");
    let (code, _, err) = run(&[
        "roll",
        "--params",
        &s(&shared),
        "--frames",
        &s(&manifest),
        "-o",
        &s(&old),
    ]);
    assert_eq!(code, 0, "{err}");
    for name in ["dim_positive.tiff", "bright_positive.tiff"] {
        assert_eq!(
            std::fs::read(out.join(name)).unwrap(),
            std::fs::read(old.join(name)).unwrap(),
            "{name}: byte-identical to the replaced route"
        );
    }
}

#[test]
fn measure_roll_refuses_a_second_statement_of_the_base_and_misplaced_frames() {
    let tmp = TempDir::new("measure-roll-unexposed-refusals");
    let frame = fixture("hdr-48bit.tif").display().to_string();
    let blank = tmp.path("blank.tif");
    std::fs::copy(&frame, &blank).unwrap();
    let blank = blank.display().to_string();
    let stated = write_file(
        &tmp.path("stated.json"),
        r#"{"recipe_version": 3, "calibration": {"film_base": {"region": [0, 0, 8, 8]}}}"#,
    );
    let copy = tmp.path("sub");
    std::fs::create_dir_all(&copy).unwrap();
    let twin = copy.join("hdr-48bit.tif");
    std::fs::copy(&frame, &twin).unwrap();
    let out = tmp.path("roll.json").display().to_string();
    for (args, expect, absent) in [
        (
            vec![
                frame.as_str(),
                "--unexposed",
                &blank,
                "--film-base",
                "0.9,0.55,0.42",
            ],
            "--film-base states one",
            "stated explicitly",
        ),
        (
            vec![
                frame.as_str(),
                "--unexposed",
                &blank,
                "--params",
                stated.to_str().unwrap(),
            ],
            "the recipe's `calibration.film_base` states one",
            "stated explicitly",
        ),
        (
            vec![frame.as_str(), "--unexposed", &frame],
            "is both the --unexposed and an input frame",
            "film base",
        ),
        (
            vec![frame.as_str(), "--unexposed", &blank, "--leader", &blank],
            "is both the --leader and the --unexposed frame",
            "input frame",
        ),
        (
            vec![
                frame.as_str(),
                twin.to_str().unwrap(),
                "--unexposed",
                &blank,
                "--out",
                &out,
            ],
            "share the file name",
            "named twice",
        ),
    ] {
        let (code, stdout, err) = run(&[&["measure-roll"][..], &args].concat());
        assert_eq!(code, 2, "{args:?}: {err}");
        assert!(stdout.is_empty(), "{args:?}");
        assert!(err.contains(expect), "{args:?}: {err}");
        assert!(
            !err.contains(absent),
            "{args:?}: the coarser rule lost: {err}"
        );
        assert!(
            !err.contains("decoded"),
            "{args:?}: refused before decoding: {err}"
        );
    }
    assert!(!Path::new(&out).exists());
}

#[test]
fn measure_roll_out_diagnoses_the_specific_fault_and_writes_a_file_roll_accepts() {
    let tmp = TempDir::new("measure-roll-out-review");
    let base = [0.9f32, 0.55, 0.42];
    let frame = tmp.path("f.tif");
    write_uniform_density(&frame, base, 0.904);
    let f = frame.to_str().unwrap();
    let out = tmp.path("roll.json");
    let o = out.to_str().unwrap();
    // A frame named twice is that fault, not a file-name clash to rename away.
    let (code, _, err) = run(&[
        "measure-roll",
        f,
        f,
        "--film-base",
        "0.9,0.55,0.42",
        "--out",
        o,
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("is named twice"), "{err}");
    assert!(!err.contains("share the file name"), "{err}");

    // An IR export path is one frame's output: it does not travel into the roll's
    // recipe, which `roll` would then refuse.
    let recipe = write_file(
        &tmp.path("in.json"),
        r#"{ "recipe_version": 3,
             "input": { "transfer": "linear", "meaning": "scanner-device",
                        "export_ir": "ir.tif" },
             "calibration": { "film_base": { "explicit": [0.9, 0.55, 0.42] } } }"#,
    );
    let r = recipe.to_str().unwrap();
    // `--out` over the recipe it reads says so, not "the input scan".
    let (code, _, err) = run(&["measure-roll", f, "--params", r, "--out", r, "--force"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("would overwrite the --params recipe"), "{err}");
    assert!(!err.contains("input scan"), "{err}");

    let (code, _, err) = run(&["measure-roll", f, "--params", r, "--out", o]);
    assert_eq!(code, 0, "{err}");
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(written["input"]["transfer"], "linear", "{written}");
    assert!(written["input"]["export_ir"].is_null(), "{written}");
    let (code, _, err) = run(&[
        "roll",
        f,
        "--params",
        o,
        "-o",
        tmp.path("out").to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
}

#[test]
fn convert_diagnoses_a_flag_its_branch_cannot_apply_before_a_bad_roll_table() {
    let tmp = TempDir::new("roll-frames-order");
    let recipe = write_file(
        &tmp.path("r.json"),
        r#"{"recipe_version": 3, "calibration": {"film_base": {"explicit": [0.9, 0.55, 0.42]}},
            "roll": {"frames": {"other.tif": {"white_stops": -1.0}}}}"#,
    );
    let (code, _, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "--film-master",
        "--roll-white",
        "2",
        "--params",
        recipe.to_str().unwrap(),
        "-o",
        tmp.path("o.tiff").to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--roll-white"), "{err}");
    assert!(
        !err.contains("roll.frames"),
        "the presence rule runs first: {err}"
    );

    // A bad decode is named as itself, not as another frame's entry.
    let sound = write_file(
        &tmp.path("sound.json"),
        r#"{"recipe_version": 3, "calibration": {"film_base": {"explicit": [0.9, 0.55, 0.42]}},
            "roll": {"frames": {"other.tif": {"white_stops": 2.0}}}}"#,
    );
    let (code, _, err) = run(&[
        "convert",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "--density-gamma",
        "0",
        "--params",
        sound.to_str().unwrap(),
        "-o",
        tmp.path("g.tiff").to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("must be finite and positive, got 0"), "{err}");
    assert!(!err.contains("roll.frames"), "{err}");
}

#[test]
fn a_flag_over_a_frames_own_white_replays_from_dump_params() {
    // `--roll-white` beats the frame's `roll.frames` entry; the dumped recipe must
    // replay what rendered, not re-apply the entry the flag overrode.
    let tmp = TempDir::new("roll-frames-replay");
    let frame = fixture("hdr-48bit.tif");
    let recipe = write_file(
        &tmp.path("r.json"),
        r#"{"recipe_version": 3, "calibration": {"film_base": {"explicit": [0.9, 0.55, 0.42]}},
            "roll": {"white_stops": 1.6,
                     "frames": {"hdr-48bit.tif": {"white_stops": 2.0}, "other.tif": {"white_stops": 2.0}}}}"#,
    );
    let (dump, first, replay) = (tmp.path("d.json"), tmp.path("a.tiff"), tmp.path("b.tiff"));
    let (code, stdout, err) = run(&[
        "convert",
        frame.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
        "--roll-white",
        "1.7",
        "--dump-params",
        dump.to_str().unwrap(),
        "-o",
        first.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(json(&stdout)["chain"]["roll"]["white_stops"], 1.7);
    let dumped: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&dump).unwrap()).unwrap();
    assert!(
        dumped["roll"]["frames"].get("hdr-48bit.tif").is_none()
            && dumped["roll"]["frames"].get("other.tif").is_some(),
        "the frame's own entry is resolved away, the others kept: {dumped}"
    );
    let (code, _, err) = run(&[
        "convert",
        frame.to_str().unwrap(),
        "--params",
        dump.to_str().unwrap(),
        "-o",
        replay.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        std::fs::read(&first).unwrap(),
        std::fs::read(&replay).unwrap(),
        "the dump replays byte-identically"
    );
}

#[test]
fn the_removed_estimate_command_names_measure_base() {
    for args in [
        vec!["estimate", "in.tif", "--grid"],
        vec!["estimate", "--help"],
        vec!["estimate"],
    ] {
        let (code, stdout, err) = run(&args);
        assert_eq!(code, 2, "{args:?}: {err}");
        assert!(stdout.is_empty(), "{args:?}");
        assert!(
            err.contains("renamed `hanten measure-base`"),
            "{args:?}: {err}"
        );
    }
}

#[test]
fn a_roll_frames_table_is_validated_by_key_and_value() {
    let tmp = TempDir::new("roll-frames-table");
    let frame = fixture("hdr-48bit.tif").display().to_string();
    for (table, expect) in [
        (
            r#"{"sub/f.tif": {"white_stops": 2.0}}"#,
            "file names, not paths",
        ),
        (
            r#"{"hdr-48bit.tif": {"white_stops": -1.0}}"#,
            "must be finite and positive",
        ),
        (
            r#"{"hdr-48bit.tif": {"white_stops": 2.0, "contrast": 1}}"#,
            "unknown field",
        ),
        // Finite and positive, but its whole contrast overflows: named as the entry,
        // not as the `roll.white_stops` it resolves into.
        (
            r#"{"hdr-48bit.tif": {"white_stops": 1e-38}}"#,
            "`roll.frames.\"hdr-48bit.tif\".white_stops`",
        ),
    ] {
        let recipe = write_file(
            &tmp.path("r.json"),
            &format!(
                r#"{{"recipe_version": 3, "calibration": {{"film_base": {{"explicit": [0.9, 0.55, 0.42]}}}},
                    "roll": {{"white_stops": 1.6, "frames": {table}}}}}"#
            ),
        );
        for command in ["convert", "roll"] {
            let out = tmp.path(&format!("o-{command}.tiff"));
            let (code, _, err) = run(&[
                command,
                &frame,
                "--params",
                recipe.to_str().unwrap(),
                "-o",
                out.to_str().unwrap(),
            ]);
            assert_eq!(code, 2, "{command} {table}: {err}");
            assert!(err.contains(expect), "{command} {table}: {err}");
            assert!(
                !err.contains("recipe `roll.white_stops`"),
                "{command} {table}: the stated 1.6 is not at fault: {err}"
            );
        }
    }
}

#[test]
fn measure_roll_leader_errors_keep_their_exit_code() {
    let tmp = TempDir::new("measure-roll-leader-errors");
    let recipe = roll_white_recipe(&tmp, "0.9,0.55,0.42");
    let frame = fixture("hdr-48bit.tif").display().to_string();
    let params = recipe.to_str().unwrap();

    // An unreadable leader is a decode error (exit 3), not a generic one.
    let missing = tmp.path("missing-leader.tif");
    let (code, _, err) = run(&[
        "measure-roll",
        "--params",
        params,
        &frame,
        "--leader",
        missing.to_str().unwrap(),
    ]);
    assert_eq!(code, 3, "{err}");

    // A leader over the memory budget is a resource refusal (exit 6, the one an agent
    // retries with `--max-memory`).
    let leader = tmp.path("leader.tif");
    write_hdri_with_uniform_ir(&leader, 64, 64, [900, 700, 400], 40_000);
    let (code, _, err) = run(&[
        "measure-roll",
        "--params",
        params,
        &frame,
        "--leader",
        leader.to_str().unwrap(),
        "--max-memory",
        "1024",
    ]);
    assert_eq!(code, 6, "{err}");
}

#[test]
fn measure_roll_reads_a_roll_recipe_whatever_rendering_it_states() {
    // `measure-roll` renders nothing, so a recipe's rendering is never read — not even
    // `direct` beside a film master, a pair `convert` refuses.
    let tmp = TempDir::new("measure-roll-direct-master");
    let recipe = write_file(
        &tmp.path("roll.json"),
        r#"{"recipe_version": 3, "rendering": "direct", "output": "film-master",
            "calibration": {"film_base": {"explicit": [0.9, 0.55, 0.42]}}}"#,
    );
    let (code, stdout, err) = run(&[
        "measure-roll",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(!err.contains("--rendering"), "{err}");
    assert_eq!(json(&stdout)["command"], "measure-roll");
}

#[test]
fn measure_roll_refuses_what_it_cannot_measure_under() {
    let tmp = TempDir::new("measure-roll-refusals");
    let frame = fixture("hdr-48bit.tif").display().to_string();

    // No stated base: a per-frame estimate would decode each frame differently.
    let (code, stdout, err) = run(&["measure-roll", &frame]);
    assert_eq!(code, 2, "{err}");
    assert!(stdout.is_empty());
    assert!(
        err.contains("--unexposed <unexposed.tif>") && err.contains("measure-base --out"),
        "{err}"
    );

    // A recipe written before the flip is refused.
    let legacy = write_file(
        &tmp.path("pre-flip.json"),
        r#"{ "calibration": { "film_base": { "explicit": [0.9, 0.55, 0.42] } } }"#,
    );
    let (code, _, err) = run(&["measure-roll", &frame, "--params", legacy.to_str().unwrap()]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("recipe_version"), "{err}");

    // Refused before any decode, so the whole roll is not read for an answer known
    // up front: `--strict` without a leader, a frame named twice, a bad inset — the
    // last blamed on the flag, not on the first frame that would have measured it.
    let base = ["--film-base", "0.9,0.55,0.42"];
    for (extra, expect) in [
        (
            vec![frame.as_str(), "--strict"],
            "--strict refuses an unguarded measurement",
        ),
        (vec![frame.as_str(), frame.as_str()], "is named twice"),
        (
            vec![frame.as_str(), "--measure-inset", "0.6"],
            "beyond the supported maximum",
        ),
    ] {
        let (code, stdout, err) = run(&[&["measure-roll"][..], &base, &extra].concat());
        assert_eq!(code, 2, "{extra:?}: {err}");
        assert!(stdout.is_empty(), "{extra:?}");
        assert!(err.contains(expect), "{extra:?}: {err}");
        assert!(
            !err.contains("decoded"),
            "{extra:?} must refuse before decoding: {err}"
        );
    }
    let (_, _, err) = run(&[
        &["measure-roll"][..],
        &base,
        &[&frame, "--measure-inset", "0.6"],
    ]
    .concat());
    assert!(
        !err.contains("hdr-48bit.tif:"),
        "the flag is at fault, not a frame: {err}"
    );

    // The recipe's scene correction is what this command measures, and its look is
    // never applied, so neither is read: a value `convert` would refuse there does not
    // refuse the measurement.
    for (name, section) in [
        ("scene.json", r#""scene_correction": { "exposure": 500 }"#),
        (
            "look.json",
            r#""look": { "highlight_desaturation": { "strength": 1.5 } }"#,
        ),
    ] {
        let recipe = write_file(
            &tmp.path(name),
            &format!(
                r#"{{ "recipe_version": 3,
                     "calibration": {{ "film_base": {{ "explicit": [0.9, 0.55, 0.42] }} }},
                     {section} }}"#
            ),
        );
        let (code, _, err) = run(&["measure-roll", &frame, "--params", recipe.to_str().unwrap()]);
        assert_eq!(code, 0, "{name}: {err}");
    }

    // A retired per-frame mode is still refused at load, and the remedy works from
    // here too: drop it, then state the gains this command reports.
    let retired = write_file(
        &tmp.path("retired.json"),
        r#"{ "recipe_version": 3, "scene_correction": { "white_balance": "percentile" } }"#,
    );
    let (code, _, err) = run(&[
        "measure-roll",
        &frame,
        "--film-base",
        "0.9,0.55,0.42",
        "--params",
        retired.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("Drop it, then state the gains"), "{err}");
}

#[test]
fn measure_roll_warns_when_a_frames_region_is_not_a_measurement() {
    // The capped-march frame (`a_capped_holder_march_warns_and_strict_promotes_it`):
    // its region keeps holder strips, which the pool would take as picture. The
    // warning `convert` gives must reach this report too, so `--strict` can see it.
    let dir = TempDir::new("measure-roll-capped");
    let path = dir.path("deep.tif");
    const W: u32 = 400;
    const H: u32 = 400;
    let mut rgb = vec![0u16; (W * H * 3) as usize];
    let mut ir = vec![41_000u16; (W * H) as usize];
    for y in 0..H {
        for x in 0..W {
            let i = ((y * W + x) * 3) as usize;
            let holder = y < 120 || !(10..W - 10).contains(&x) || y >= H - 10;
            rgb[i..i + 3].copy_from_slice(&if holder {
                [655, 655, 655]
            } else {
                [12000, 7000, 4000]
            });
            if holder {
                ir[(y * W + x) as usize] = 1_300;
            }
        }
    }
    write_hdri(&path, W, H, &rgb, &ir);
    let recipe = roll_white_recipe(&dir, "0.9,0.6,0.5");
    let (code, stdout, err) = run(&[
        "measure-roll",
        path.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    let warnings = json(&stdout)["warnings"].clone();
    assert!(
        warnings
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("deep.tif: ")
                && w.as_str().unwrap().contains("cap")),
        "{warnings}"
    );
}

// ---------------------------------------------------------------------------
// The look: highlight desaturation (`nf-look/path-to-white`)
// ---------------------------------------------------------------------------

#[test]
fn highlight_desaturation_reaches_the_pixels_by_flag_and_by_recipe() {
    let tmp = TempDir::new("look-desat");
    let input = fixture("hdr-48bit.tif").display().to_string();
    let convert = |name: &str, extra: &[&str]| {
        let out = tmp.path(name);
        let mut argv = vec![
            "convert",
            input.as_str(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
            // Push the fixture's highlights past diffuse white, where the operator acts.
            "--exposure",
            "2",
        ];
        argv.extend_from_slice(extra);
        let (code, stdout, err) = run(&argv);
        assert_eq!(code, 0, "{extra:?}: {err}");
        (std::fs::read(&out).unwrap(), json(&stdout))
    };
    let look = |r: &serde_json::Value| r["chain"]["stages"][1].clone();

    // On by default at 0.8, after the look's default contrast, and the report says so.
    let (plain, report) = convert("plain.tiff", &[]);
    assert_eq!(
        look(&report)["applied"],
        "contrast+highlight-desaturation",
        "{report}"
    );
    assert_eq!(
        report["chain"]["look"]["highlight_desaturation"],
        serde_json::json!({"strength": 0.8, "start_stops": -1.0, "band": [0.015, 0.025]})
    );
    // Strength 0 is off: the look then runs its contrast alone, different from the
    // default, and with the other two knobs inert — a moved band or start changes
    // nothing when off.
    let (off, report) = convert("off.tiff", &["--highlight-desaturation", "0"]);
    assert_eq!(look(&report)["applied"], "contrast", "{report}");
    assert_ne!(plain, off, "the default must move the fixture's highlights");
    let (off_moved, _) = convert(
        "off-moved.tiff",
        &[
            "--highlight-desaturation",
            "0",
            "--highlight-desaturation-start",
            "-3",
            "--highlight-desaturation-band",
            "0.001,0.3",
        ],
    );
    assert_eq!(off, off_moved, "off is off whatever the band and start");

    // A stronger pull moves further.
    let (on, report) = convert("on.tiff", &["--highlight-desaturation", "1"]);
    assert_eq!(look(&report)["stage"], "look");
    assert_ne!(plain, on, "strength must change the pixels");

    // The recipe key is the same knob, and a dump writes it back.
    let recipe = write_file(
        &tmp.path("look.json"),
        r#"{ "recipe_version": 3,
             "look": { "highlight_desaturation": { "strength": 1 } } }"#,
    );
    let dump = tmp.path("dump.json");
    let (from_recipe, _) = convert(
        "recipe.tiff",
        &[
            "--params",
            recipe.to_str().unwrap(),
            "--dump-params",
            dump.to_str().unwrap(),
        ],
    );
    assert_eq!(on, from_recipe, "the recipe key and the flag are one knob");
    let dumped: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&dump).unwrap()).unwrap();
    assert_eq!(dumped["look"]["highlight_desaturation"]["strength"], 1.0);

    // A flag wins over the recipe, down to the identity: the recipe's pull is live
    // (so the comparison can fail), and the flag resets it to off.
    assert_ne!(
        from_recipe, off,
        "the recipe's strength must move the pixels"
    );
    let (reset, report) = convert(
        "reset.tiff",
        &[
            "--params",
            recipe.to_str().unwrap(),
            "--highlight-desaturation",
            "0",
        ],
    );
    assert_eq!(
        report["chain"]["look"]["highlight_desaturation"]["strength"], 0.0,
        "{report}"
    );
    assert_eq!(reset, off, "the flag's 0 must win over the recipe's 1");

    // A narrower band moves fewer pixels: the band is live.
    let (narrow, _) = convert(
        "narrow.tiff",
        &[
            "--highlight-desaturation",
            "1",
            "--highlight-desaturation-band",
            "0.001,0.002",
        ],
    );
    assert_ne!(narrow, on, "the band must change which pixels are pulled");
}

/// The roll white whose slope is exactly 1 (`log2(1/0.18)`, the binary's own value
/// printed so it parses back to the same `f32`): the look's contrast is then the
/// identity, for a test that wants the look to do nothing else.
fn scene_contrast_white() -> String {
    (1.0f32 / 0.18).log2().to_string()
}

/// The look's contrast (`nf-look/contrast-definition`): a multiplier on the base slope,
/// reachable by flag and by recipe, one knob, the flag winning; `1` keeps the base; the
/// report states the knob, the base and where it came from, and the slope; and the
/// decode is untouched by it.
#[test]
fn the_look_contrast_reaches_the_pixels_by_flag_and_by_recipe() {
    let tmp = TempDir::new("look-contrast");
    let input = fixture("hdr-48bit.tif").display().to_string();
    let convert = |name: &str, extra: &[&str]| {
        let out = tmp.path(name);
        let mut argv = vec![
            "convert",
            input.as_str(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
            // Off, so the look's `applied` reads the contrast alone.
            "--highlight-desaturation",
            "0",
        ];
        argv.extend_from_slice(extra);
        let (code, stdout, err) = run(&argv);
        assert_eq!(code, 0, "{extra:?}: {err}");
        (std::fs::read(&out).unwrap(), json(&stdout))
    };
    let applied = |r: &serde_json::Value| r["chain"]["stages"][1]["applied"].clone();
    let look = |r: &serde_json::Value| {
        let l = &r["chain"]["look"];
        (
            l["contrast"].as_f64().unwrap(),
            l["base_slope"].as_f64().unwrap(),
            l["base_from"].as_str().unwrap().to_owned(),
            l["slope"].as_f64().unwrap(),
        )
    };
    // Placed as if the roll's white were 1.75 stops above mid-grey.
    let fallback = (1.0_f64 / 0.18).log2() / 1.75;

    let (default, report) = convert("default.tiff", &[]);
    assert_eq!(applied(&report), "contrast", "{report}");
    let (k, base, from, slope) = look(&report);
    assert!(
        k == 1.0 && (base - fallback).abs() < 1e-6 && from == "fallback",
        "{report}"
    );
    assert_eq!(slope, base, "{report}");
    let default_decode = report["chain"]["decode"].clone();

    // 1 keeps the base: the same bytes as leaving it unstated.
    let (kept, _) = convert("kept.tiff", &["--contrast", "1"]);
    assert_eq!(kept, default, "--contrast 1 keeps the base");

    let (steep, report) = convert("steep.tiff", &["--contrast", "1.5"]);
    assert_ne!(steep, default);
    let (k, base, _, slope) = look(&report);
    assert_eq!(k, 1.5, "{report}");
    assert!((slope - base * 1.5).abs() < 1e-6, "{report}");
    // The decode block is the same at every contrast.
    assert_eq!(
        report["chain"]["decode"], default_decode,
        "the look contrast reached the decode"
    );

    // On a roll white it builds on the roll's slope, not the fallback.
    let (_, report) = convert("roll.tiff", &["--roll-white", "2", "--contrast", "1.2"]);
    let (_, base, from, slope) = look(&report);
    assert_eq!(from, "roll", "{report}");
    assert_eq!(base, report["chain"]["roll"]["slope"].as_f64().unwrap());
    assert!((slope - base * 1.2).abs() < 1e-6, "{report}");
    // A slope of exactly 1 is the identity, reported as such.
    let white = scene_contrast_white();
    let (unity, report) = convert("unity.tiff", &["--roll-white", &white]);
    assert_eq!(applied(&report), "identity", "{report}");
    assert_ne!(unity, default);

    // The recipe key is the same knob, and a flag wins over it.
    let recipe = write_file(
        &tmp.path("look.json"),
        r#"{ "recipe_version": 3, "look": { "contrast": 1.5 } }"#,
    );
    let (from_recipe, _) = convert("recipe.tiff", &["--params", recipe.to_str().unwrap()]);
    assert_eq!(
        steep, from_recipe,
        "the recipe key and the flag are one knob"
    );
    let (reset, _) = convert(
        "reset.tiff",
        &["--params", recipe.to_str().unwrap(), "--contrast", "1"],
    );
    assert_eq!(
        reset, default,
        "the flag's 1 must win over the recipe's 1.5"
    );
}

#[test]
fn the_look_contrast_refuses_a_value_it_cannot_apply() {
    let input = fixture("hdr-48bit.tif").display().to_string();
    let tmp = TempDir::new("look-contrast-refused");
    let out = tmp.path("x.tiff");
    let base = [
        "convert",
        input.as_str(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ];
    // A non-positive value is refused naming the flag and the key.
    for value in ["0", "-1"] {
        let (code, _, err) = run(&[&base[..], &["--contrast", value]].concat());
        assert_eq!(code, 2, "{value}: {err}");
        assert!(err.contains("--contrast (recipe `look.contrast`)"), "{err}");
    }
    // So is a pair each usable alone whose product leaves f32 — a usage error naming
    // both knobs, not an internal one from highlight desaturation.
    for (gamma, contrast) in [("1e30", "1e10"), ("1e-30", "1e-20")] {
        let (code, _, err) = run(&[
            &base[..],
            &["--density-gamma", gamma, "--contrast", contrast],
        ]
        .concat());
        assert_eq!(code, 2, "{gamma} × {contrast}: {err}");
        assert!(
            err.contains("--density-gamma (recipe `reconstruction.linearization`)")
                && err.contains("--contrast (recipe `look.contrast`)"),
            "{err}"
        );
    }
}

/// The look's per-channel grade (`nf-look/per-channel-grade`): reachable by flag and by
/// recipe, one knob, the flag winning down to the identity, reported as it ran.
#[test]
fn the_channel_grade_reaches_the_pixels_by_flag_and_by_recipe() {
    let tmp = TempDir::new("look-channel-grade");
    let input = fixture("hdr-48bit.tif").display().to_string();
    let white = scene_contrast_white();
    let convert = |name: &str, extra: &[&str]| {
        let out = tmp.path(name);
        let mut argv = vec![
            "convert",
            input.as_str(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
            // Contrast and desaturation off, so `applied` reads the grade alone.
            "--roll-white",
            &white,
            "--highlight-desaturation",
            "0",
        ];
        argv.extend_from_slice(extra);
        let (code, stdout, err) = run(&argv);
        assert_eq!(code, 0, "{extra:?}: {err}");
        (std::fs::read(&out).unwrap(), json(&stdout))
    };
    let applied = |r: &serde_json::Value| r["chain"]["stages"][1]["applied"].clone();

    let (identity, report) = convert("identity.tiff", &[]);
    assert_eq!(applied(&report), "identity", "{report}");
    assert_eq!(
        report["chain"]["look"]["channel_grade"],
        serde_json::json!([1.0, 1.0]),
        "{report}"
    );

    let (graded, report) = convert("graded.tiff", &["--channel-grade", "1.2,0.85"]);
    assert_eq!(applied(&report), "channel-grade", "{report}");
    assert_ne!(graded, identity, "the grade must move the pixels");

    let recipe = write_file(
        &tmp.path("look.json"),
        r#"{ "recipe_version": 3, "look": { "channel_grade": [1.2, 0.85] } }"#,
    );
    let (from_recipe, _) = convert("recipe.tiff", &["--params", recipe.to_str().unwrap()]);
    assert_eq!(
        graded, from_recipe,
        "the recipe key and the flag are one knob"
    );
    let (reset, report) = convert(
        "reset.tiff",
        &[
            "--params",
            recipe.to_str().unwrap(),
            "--channel-grade",
            "1,1",
        ],
    );
    assert_eq!(applied(&report), "identity", "{report}");
    assert_eq!(
        reset, identity,
        "the flag's 1,1 must win over the recipe's grade"
    );
}

#[test]
fn the_channel_grade_refuses_a_value_it_cannot_apply() {
    let input = fixture("hdr-48bit.tif").display().to_string();
    let tmp = TempDir::new("look-channel-grade-refused");
    let out = tmp.path("x.tiff");
    let base = [
        "convert",
        input.as_str(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ];
    // A non-positive exponent, and a spread that would fold the tone scale.
    for value in ["0,1", "1.1,-0.5", "1.6,0.5"] {
        let (code, _, err) = run(&[&base[..], &["--channel-grade", value]].concat());
        assert_eq!(code, 2, "{value}: {err}");
        assert!(
            err.contains("--channel-grade (recipe `look.channel_grade`)"),
            "{err}"
        );
    }
}

#[test]
fn highlight_desaturation_refuses_a_value_it_cannot_apply() {
    let input = fixture("hdr-48bit.tif").display().to_string();
    let tmp = TempDir::new("look-desat-refused");
    let out = tmp.path("x.tiff");
    let base = [
        "convert",
        input.as_str(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ];
    // Out-of-range values are refused naming the flag and the key.
    for (flag, expect) in [
        (["--highlight-desaturation", "1.5"], "within [0, 1]"),
        // A negative value reaches the value rule rather than clap's parser.
        (["--highlight-desaturation", "-0.5"], "within [0, 1]"),
        (
            ["--highlight-desaturation-band", "-0.01,0.02"],
            "0 <= s0 < s1",
        ),
        (["--highlight-desaturation-start", "0"], "must be negative"),
        (
            ["--highlight-desaturation-band", "0.03,0.02"],
            "0 <= s0 < s1",
        ),
    ] {
        let (code, _, err) = run(&[&base[..], &flag].concat());
        assert_eq!(code, 2, "{flag:?}: {err}");
        assert!(
            err.contains(flag[0])
                && err.contains("look.highlight_desaturation")
                && err.contains(expect),
            "{flag:?}: {err}"
        );
    }
}

/// `convert` on the 48-bit fixture with `extra`, writing to `out`.
fn convert_48bit(out: &Path, extra: &[&str]) -> (i32, String, String) {
    let input = fixture("hdr-48bit.tif");
    let mut argv = vec![
        "convert",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--film-base",
        "0.9,0.55,0.42",
    ];
    argv.extend_from_slice(extra);
    let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
    run(&argv)
}

#[test]
fn every_destination_renders_end_to_end() {
    // Each ready row of the destination table, from the flags that name it: the path is
    // completed from its container, the bytes are that container, the report names every
    // resolved axis, and the encoder's own block is there. The look is named on each.
    let tmp = TempDir::new("destinations");
    for (i, (extra, container, suffix, block)) in [
        (vec![], "tiff", "tiff", None),
        (vec!["--gamut", "adobe-rgb"], "tiff", "tiff", None),
        (vec!["--gamut", "srgb"], "tiff", "tiff", None),
        (
            vec!["--transfer", "linear", "--gamut", "bt2020"],
            "tiff",
            "tiff",
            Some("hdr_linear_tiff"),
        ),
        (
            vec!["--transfer", "linear", "--gamut", "display-p3"],
            "tiff",
            "tiff",
            Some("hdr_linear_tiff"),
        ),
        (
            vec!["--transfer", "linear", "--gamut", "adobe-rgb"],
            "tiff",
            "tiff",
            Some("hdr_linear_tiff"),
        ),
        (
            vec!["--transfer", "linear", "--gamut", "srgb"],
            "tiff",
            "tiff",
            Some("hdr_linear_tiff"),
        ),
        (
            vec!["--transfer", "pq"],
            "tiff",
            "tiff",
            Some("hdr_coded_tiff"),
        ),
        (
            vec!["--transfer", "hlg"],
            "tiff",
            "tiff",
            Some("hdr_coded_tiff"),
        ),
        (vec!["--range", "hdr"], "jpeg", "jpg", None),
        (
            vec!["--range", "hdr", "--gamut", "srgb"],
            "jpeg",
            "jpg",
            None,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let stem = tmp.path(&format!("d{i}"));
        let (code, stdout, err) = convert_48bit(&stem, &extra);
        assert_eq!(code, 0, "{extra:?}: {err}");
        let out = PathBuf::from(format!("{}.{suffix}", stem.display()));
        assert_eq!(sniff_container(&out), container, "{extra:?}");
        let report = json(&stdout);
        assert_eq!(report["output"], out.to_str().unwrap(), "{extra:?}");
        let nf = &report["chain"];
        let axes = &nf["destination"]["display"];
        for key in ["range", "transfer", "gamut", "container"] {
            assert!(axes[key].is_string(), "{extra:?}: {key} unresolved: {nf}");
        }
        for pair in extra.chunks(2) {
            assert_eq!(axes[pair[0].trim_start_matches("--")], pair[1], "{extra:?}");
        }
        assert_eq!(nf["stages"][1]["stage"], "look", "{extra:?}");
        let hdr = axes["range"] == "hdr";
        assert_eq!(nf["peak_clamp"].is_object(), hdr, "{extra:?}: {nf}");
        let gain_map = axes["container"] == "jpeg";
        assert_eq!(nf["gain_map"].is_object(), gain_map, "{extra:?}: {nf}");
        if hdr && axes["transfer"] != "linear" && !gain_map {
            assert_eq!(axes["gamut"], "bt2020", "{extra:?}: coded HDR is BT.2020");
        }
        if let Some(block) = block {
            assert!(report[block].is_object(), "{extra:?}: no `{block}` block");
        }
        if extra.is_empty() || extra == ["--gamut", "adobe-rgb"] || extra == ["--gamut", "srgb"] {
            assert_eq!(read_tiff_bits(&out), 16, "{extra:?}");
        }
    }
}

#[test]
fn a_linear_tiff_names_its_gamut_and_keeps_values_above_white() {
    use tiff::decoder::{Decoder, DecodingResult};
    let tmp = TempDir::new("linear-gamuts");
    for (gamut, description) in [
        ("display-p3", "NC Display-Linear Display P3 (D65)"),
        ("adobe-rgb", "NC Display-Linear Adobe RGB (D65)"),
        ("srgb", "NC Display-Linear sRGB (D65)"),
        ("bt2020", "NC Display-Linear BT.2020 (D65)"),
    ] {
        let stem = tmp.path(gamut);
        let (code, stdout, err) = convert_48bit(
            &stem,
            &["--transfer", "linear", "--gamut", gamut, "--exposure", "3"],
        );
        assert_eq!(code, 0, "{gamut}: {err}");
        let out = PathBuf::from(format!("{}.tiff", stem.display()));
        // The profile's description is UTF-16BE inside its `mluc` record.
        let wanted: Vec<u8> = description
            .encode_utf16()
            .flat_map(|u| u.to_be_bytes())
            .collect();
        let icc = read_icc_tag(&out);
        assert!(
            icc.windows(wanted.len()).any(|w| w == wanted),
            "{gamut}: the profile is not named {description:?}"
        );
        let block = &json(&stdout)["hdr_linear_tiff"];
        let domain = block["linear_domain"].as_str().unwrap();
        assert!(domain.starts_with(&format!("{gamut}-linear-")), "{block}");
        let contract = block["pixel_contract"].as_str().unwrap();
        assert!(
            contract.contains(&format!("-linear-{gamut}-d65-")),
            "{block}"
        );
        let mut dec = Decoder::new(std::io::BufReader::new(std::fs::File::open(&out).unwrap()))
            .unwrap()
            .with_limits(tiff::decoder::Limits::unlimited());
        let DecodingResult::F32(samples) = dec.read_image().unwrap() else {
            panic!("{gamut}: not a float TIFF");
        };
        let max = samples.iter().copied().fold(f32::MIN, f32::max);
        assert!(max > 1.0, "{gamut}: nothing above reference white ({max})");
    }
}

#[test]
fn the_direct_rendering_writes_the_decode_with_only_what_the_container_needs() {
    // `hdr-48bit.tif` is IR-free, so `--strict` sees only this run's warnings.
    let tmp = TempDir::new("direct");
    let recipe = write_file(
        &tmp.path("roll.json"),
        r#"{"recipe_version": 3,
            "calibration": {"film_base": {"explicit": [0.9, 0.55, 0.42]}},
            "roll": {"white_balance": [0.8, 1.0, 1.25], "white_stops": 1.6}}"#,
    );
    let convert = |name: &str, extra: &[&str]| {
        let out = tmp.path(name);
        let (code, stdout, err) = run(&[
            &[
                "convert",
                fixture("hdr-48bit.tif").to_str().unwrap(),
                "--params",
                recipe.to_str().unwrap(),
                "-o",
                out.to_str().unwrap(),
            ][..],
            extra,
        ]
        .concat());
        (code, stdout, err, out)
    };

    // Bare: the HDR float TIFF, the roll left out and saying so, nothing to warn about.
    let (code, stdout, err, out) = convert("bare", &["--rendering", "direct", "--strict"]);
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);
    let nf = &report["chain"];
    assert_eq!(nf["rendering"], "direct", "{nf}");
    assert_eq!(
        nf["destination"]["display"],
        serde_json::json!({"range": "hdr", "transfer": "linear", "gamut": "adobe-rgb",
                           "container": "tiff"}),
        "{nf}"
    );
    assert_eq!(
        read_tiff_bits(&PathBuf::from(format!("{}.tiff", out.display()))),
        32
    );
    assert_eq!(nf["roll"]["white_balance_applied"], false, "{nf}");
    assert_eq!(nf["roll"]["slope_applied"], false, "{nf}");
    assert_eq!(
        nf["scene_correction"]["white_balance"],
        serde_json::json!([1.0, 1.0, 1.0]),
        "{nf}"
    );
    assert_eq!(
        nf["look"]["highlight_desaturation"]["strength"], 0.0,
        "{nf}"
    );
    assert!(
        report
            .get("warnings")
            .is_none_or(|w| w.as_array().unwrap().is_empty()),
        "{report}"
    );

    // Stated SDR: Adobe RGB, 16 bits — the form viewed by eye.
    let (code, stdout, err, out) = convert("sdr", &["--rendering", "direct", "--range", "sdr"]);
    assert_eq!(code, 0, "{err}");
    let nf = &json(&stdout)["chain"];
    assert_eq!(nf["destination"]["display"]["gamut"], "adobe-rgb", "{nf}");
    assert_eq!(
        read_tiff_bits(&PathBuf::from(format!("{}.tiff", out.display()))),
        16
    );

    // `default` on the same recipe applies the roll.
    let (code, stdout, err, _) = convert("default", &[]);
    assert_eq!(code, 0, "{err}");
    let nf = &json(&stdout)["chain"];
    assert_eq!(nf["rendering"], "default", "{nf}");
    assert_eq!(nf["roll"]["white_balance_applied"], true, "{nf}");

    // The film master renders nothing for `direct` to start from.
    let (code, _, err, _) = convert("fm", &["--rendering", "direct", "--film-master"]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("--rendering direct") && err.contains("--film-master"),
        "{err}"
    );
}

#[test]
fn the_gain_map_destination_writes_an_iso_only_jpeg_and_reports_its_map() {
    // `hdr-48bit.tif` is 502×462 and IR-free, and every run states a roll measurement
    // (neutral gains, exposure 0, the default contrast), so a `--strict` exit is this run's
    // own — not the `default` rendering's no-roll warnings.
    let tmp = TempDir::new("gain-map");
    let measured = MEASURED;
    let convert = |name: &str, extra: &[&str]| {
        let (code, stdout, err) = convert_48bit(
            &tmp.path(name),
            &[&["--range", "hdr"][..], &measured[..], extra].concat(),
        );
        assert_eq!(code, 0, "{extra:?}: {err}");
        let bytes = std::fs::read(tmp.path(&format!("{name}.jpg"))).unwrap();
        (json(&stdout), bytes)
    };

    // Three stops up puts highlights above diffuse white: the map is live.
    let (report, bytes) = convert("live", &["--exposure", "3"]);
    let gm = &report["chain"]["gain_map"];
    assert_eq!(gm["flat"], false, "{gm}");
    assert!(
        gm["max"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m.as_f64().unwrap() > 1.0)
    );
    assert_eq!(
        (gm["width"].as_u64(), gm["height"].as_u64()),
        (Some(251), Some(231))
    );
    assert_eq!(gm["base_fit_range"]["display_peak"], 1.0, "{gm}");
    let hdr_peak = report["chain"]["fit_range"]["display_peak"]
        .as_f64()
        .unwrap();
    assert!((hdr_peak - 1000.0 / 203.0).abs() < 1e-5, "{report}");
    // ISO 21496-1 is the only dialect: its segment label, and no Ultra HDR v1 XMP.
    let contains = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
    assert!(contains(b"urn:iso:std:iso:ts:21496:-1\0"));
    assert!(contains(b"MPF\0"));
    assert!(!contains(b"hdrgm"));
    // The file opens as the SDR base in an ordinary reader.
    assert_eq!(
        image::load_from_memory(&bytes)
            .unwrap()
            .to_rgb8()
            .dimensions(),
        (502, 462)
    );

    // Far overexposed, the SDR base clips and the HDR rendition clamps at its peak — two
    // renditions, both counted, so the loss is taken over both and stays a fraction.
    let (report, _) = convert("over", &["--exposure", "12"]);
    let loss = &report["loss"];
    let clipped = loss["clipped_high"].as_u64().unwrap() + loss["clipped_low"].as_u64().unwrap();
    let total = loss["total_samples"].as_u64().unwrap();
    assert!(clipped > 0 && clipped <= total, "{loss}");
    assert_eq!(total, 2 * 502 * 462 * 3, "{loss}");

    // Five stops down leaves nothing above white: the map is flat, and says so in the
    // report — not as a warning, so `--strict` still passes.
    let (report, _) = convert("flat", &["--exposure=-5", "--strict"]);
    let gm = &report["chain"]["gain_map"];
    assert_eq!(gm["flat"], true, "{gm}");
    assert_eq!(gm["min"], serde_json::json!([1.0, 1.0, 1.0]), "{gm}");
    assert_eq!(gm["max"], serde_json::json!([1.0, 1.0, 1.0]), "{gm}");
    assert!(
        report["warnings"].as_array().is_none_or(|w| w.is_empty()),
        "{report}"
    );
}

#[test]
fn the_film_master_runs_no_rendering_and_refuses_a_look() {
    let tmp = TempDir::new("film-master");
    let stem = tmp.path("master");
    let (code, stdout, err) = convert_48bit(&stem, &["--film-master"]);
    assert_eq!(code, 0, "{err}");
    let out = PathBuf::from(format!("{}.tiff", stem.display()));
    assert_eq!(read_tiff_bits(&out), 32);
    let nf = &json(&stdout)["chain"];
    assert_eq!(nf["destination"], "film-master");
    assert_eq!(nf["stages"], serde_json::json!([]));
    assert!(nf.get("look").is_none(), "{nf}");

    // A look the user asked for is refused, naming the look rather than a knob.
    let (code, _, err) = convert_48bit(&tmp.path("a"), &["--film-master", "--contrast", "1.3"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("the look"), "{err}");
    assert!(
        !err.contains("--contrast 1.3"),
        "one rule, not one per knob: {err}"
    );
    // The empty look renders exactly what the film master does, so it is spared: the
    // flags-win reset of a recipe's look.
    let (code, _, err) = convert_48bit(
        &tmp.path("b"),
        &[
            "--film-master",
            "--contrast",
            "1",
            "--highlight-desaturation",
            "0",
        ],
    );
    assert_eq!(code, 0, "{err}");
    // The film master and an axis are one choice, refused at the parser.
    let (code, _, err) = convert_48bit(&tmp.path("c"), &["--film-master", "--range", "hdr"]);
    assert_eq!(code, 2, "{err}");
}

#[test]
fn the_hdr_hand_off_counts_what_it_clamps_and_strict_sees_it() {
    // `hdr-48bit.tif` is the IR-free fixture, and every run states a roll measurement
    // (neutral gains, exposure 0, the default contrast), so a `--strict` exit 1 is this
    // warning's — not the `default` rendering's no-roll warnings.
    let tmp = TempDir::new("peak-clamp");
    let measured = MEASURED;
    // The control: at the defaults nothing sits above the peak, and `--strict` passes.
    let (code, stdout, err) = convert_48bit(
        &tmp.path("a"),
        &[&measured[..], &["--transfer", "pq", "--strict"]].concat(),
    );
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);
    assert_eq!(report["chain"]["peak_clamp"]["above_peak"], 0, "{report}");
    // Three stops up with fit range at its identity puts content past the 1000-nit peak:
    // clamped at the hand-off, counted there, folded into the report's clip count.
    let over = [
        &measured[..],
        &[
            "--transfer",
            "pq",
            "--exposure",
            "3",
            "--display-tone-headroom",
            "0",
        ],
    ]
    .concat();
    let (code, stdout, err) = convert_48bit(&tmp.path("b"), &over);
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);
    let above = report["chain"]["peak_clamp"]["above_peak"]
        .as_u64()
        .unwrap();
    assert!(above > 0, "{report}");
    assert_eq!(
        report["loss"]["clipped_high"].as_u64(),
        Some(above),
        "{report}"
    );
    assert!(err.contains("clipped"), "{err}");
    let (code, _, err) = convert_48bit(&tmp.path("c"), &[&over[..], &["--strict"]].concat());
    assert!(!err.contains("no roll measurement"), "{err}");
    assert_eq!(code, 1, "--strict must promote the clamp: {err}");

    // An HDR signal that never passes reference white is warned about with this chain's
    // levers, not the removed chain's.
    let (code, stdout, err) = convert_48bit(&tmp.path("d"), &["--transfer", "pq", "--exposure=-5"]);
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);
    let warning = report["warnings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|w| w.as_str())
        .find(|w| w.contains("SDR-range signal"))
        .unwrap_or_else(|| panic!("no SDR-range warning: {report}"));
    assert!(
        warning.contains("`--exposure`") && warning.contains("--range sdr"),
        "{warning}"
    );
    assert!(
        !warning.contains("--print-exposure") && !warning.contains("SDR preset"),
        "{warning}"
    );
}

#[test]
fn the_film_master_refuses_every_stage_it_does_not_run() {
    // The film master runs no rendering stage, so a request for any of them is refused
    // — one rule per stage, naming the stage — never silently ignored.
    let tmp = TempDir::new("film-master-stages");
    for (i, (extra, stage)) in [
        (&["--exposure", "2"][..], "scene correction"),
        (&["--white-balance", "1.2,1,1.1"][..], "scene correction"),
        (&["--display-tone-headroom", "3"][..], "fit range"),
        (&["--display-black", "5"][..], "fit range"),
        (&["--contrast", "1.3"][..], "the look"),
    ]
    .into_iter()
    .enumerate()
    {
        let argv = [&["--film-master"][..], extra].concat();
        let (code, _, err) = convert_48bit(&tmp.path(&format!("r{i}")), &argv);
        assert_eq!(code, 2, "{extra:?}: {err}");
        assert!(err.contains(stage), "{extra:?}: {err}");
        assert!(err.contains("choose a rendered destination"), "{err}");
    }
    // Every stage asked for is named at once, in chain order.
    let (code, _, err) = convert_48bit(
        &tmp.path("all"),
        &[
            "--film-master",
            "--exposure",
            "2",
            "--white-balance",
            "1.2,1,1.1",
            "--display-tone-headroom",
            "3",
        ],
    );
    assert_eq!(code, 2, "{err}");
    let scene = err.find("scene correction (").expect(&err);
    let fit = err.find("fit range (").expect(&err);
    assert!(scene < fit, "{err}");
    assert!(
        !err.contains("the look ("),
        "the look was not asked for: {err}"
    );
    // The defaults and the identities render exactly what the master does, so they are
    // spared — the flags-win reset of a recipe that asks for a stage.
    for (i, extra) in [
        &["--exposure", "0", "--white-balance", "1,1,1"][..],
        &["--display-tone-headroom", "0"][..],
        &["--display-tone-headroom", "6"][..],
        &["--display-black", "off"][..],
        &["--display-black", "6"][..],
    ]
    .into_iter()
    .enumerate()
    {
        let argv = [&["--film-master"][..], extra].concat();
        let (code, _, err) = convert_48bit(&tmp.path(&format!("ok{i}")), &argv);
        assert_eq!(code, 0, "{extra:?}: {err}");
    }
    let recipe = write_file(
        &tmp.path("stages.json"),
        r#"{"recipe_version": 3, "scene_correction": {"exposure": 1.5},
            "fit_range": {"headroom_stops": 4, "display_black": 5}}"#,
    );
    let r = recipe.to_str().unwrap();
    let (code, _, err) = convert_48bit(&tmp.path("rec"), &["--film-master", "--params", r]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("scene correction") && err.contains("fit range"),
        "{err}"
    );
    // Following the remedy — each stage's identity by flag — converts.
    let (code, _, err) = convert_48bit(
        &tmp.path("reset"),
        &[
            "--film-master",
            "--params",
            r,
            "--exposure",
            "0",
            "--display-tone-headroom",
            "0",
            "--display-black",
            "off",
        ],
    );
    assert_eq!(code, 0, "{err}");
}

#[test]
fn a_destination_the_table_lacks_is_refused_with_a_remedy_that_works() {
    let tmp = TempDir::new("destination-refusals");
    // A conflicting pair is named, not the bystander, and the remedy is a flag.
    let (code, _, err) = convert_48bit(
        &tmp.path("a"),
        &["--range", "sdr", "--gamut", "bt2020", "--container", "tiff"],
    );
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--range sdr and --gamut bt2020"), "{err}");
    assert!(err.contains("--gamut adobe-rgb"), "{err}");
    // Following that remedy converts.
    let (code, _, err) = convert_48bit(
        &tmp.path("a2"),
        &[
            "--range",
            "sdr",
            "--gamut",
            "adobe-rgb",
            "--container",
            "tiff",
        ],
    );
    assert_eq!(code, 0, "{err}");
    // An axis the table cannot decide is asked for, offering only values that resolve.
    let (code, _, err) = convert_48bit(&tmp.path("b"), &["--gamut", "bt2020"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--transfer linear|pq|hlg"), "{err}");
    // A linear TIFF is written in several gamuts, so the default one is not taken.
    let (code, _, err) = convert_48bit(&tmp.path("b2"), &["--transfer", "linear"]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("--gamut display-p3|adobe-rgb|srgb|bt2020"),
        "{err}"
    );
    // A planned row names its task and what is ready now, as the fewest flags to add
    // to what was stated — and following that remedy converts.
    let (code, _, err) = convert_48bit(&tmp.path("c"), &["--container", "jpeg"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("output/sdr-jpeg-preset"), "{err}");
    assert!(
        err.contains("adding to what is stated: --range hdr; --range hdr --gamut srgb "),
        "{err}"
    );
    for (i, add) in [
        &["--range", "hdr"][..],
        &["--range", "hdr", "--gamut", "srgb"],
    ]
    .iter()
    .enumerate()
    {
        let (code, _, err) = convert_48bit(
            &tmp.path(&format!("c{i}.jpg")),
            &[&["--container", "jpeg"][..], &add[..]].concat(),
        );
        assert_eq!(code, 0, "{add:?}: {err}");
    }
    // A stated suffix the destination does not write is refused, naming it.
    let (code, _, err) = convert_48bit(&tmp.path("e.tiff"), &["--range", "hdr"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains(".jpg"), "{err}");
    // `--output-preset` is refused, and the preset's counterpart named — for an AVIF
    // preset, the same signal's TIFF.
    let (code, _, err) = convert_48bit(&tmp.path("f"), &["--output-preset", "hdr-pq"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("the nearest is --transfer pq:"), "{err}");
    assert!(err.contains("no longer writes AVIF"), "{err}");
}

#[test]
fn a_suffix_refusal_offers_only_a_destination_that_writes_it() {
    let tmp = TempDir::new("suffix-offers");
    // An axis the recipe states cannot be unstated by a flag, only overridden, so the
    // offer restates it — and the printed remedy, followed as written, converts.
    let recipe = write_file(
        &tmp.path("adobe.json"),
        r#"{"recipe_version": 3, "output": {"display": {"gamut": "adobe-rgb"}}}"#,
    );
    let with_recipe = ["--params", recipe.to_str().unwrap()];
    let (code, _, err) = convert_48bit(&tmp.path("r.jpg"), &with_recipe);
    assert_eq!(code, 2, "{err}");
    let (_, offers) = err
        .split_once("state a destination that writes it: ")
        .unwrap_or_else(|| panic!("no offer: {err}"));
    for offer in offers.trim().split("; ") {
        assert!(
            offer.contains("--gamut ") && !offer.contains("adobe-rgb"),
            "{offer}: {err}"
        );
        let flags: Vec<&str> = offer.split_whitespace().collect();
        let (code, _, err) =
            convert_48bit(&tmp.path("r.jpg"), &[&with_recipe[..], &flags[..]].concat());
        assert_eq!(code, 0, "following `{offer}` must convert: {err}");
    }
    // The ready JPEGs are the gain maps, offered beside dropping the suffix, and each
    // offer converts as written. `--gamut srgb` alone would be the SDR JPEG, not written
    // yet, so the sRGB offer states its range.
    let (code, _, err) = convert_48bit(&tmp.path("b.jpg"), &[]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("drop .jpg"), "{err}");
    let (_, offers) = err
        .split_once("state a destination that writes it: ")
        .unwrap_or_else(|| panic!("no offer: {err}"));
    assert_eq!(
        offers.trim(),
        "--range hdr --container jpeg; --range hdr --gamut srgb --container jpeg",
        "{err}"
    );
    for offer in offers.trim().split("; ") {
        let flags: Vec<&str> = offer.split_whitespace().collect();
        let (code, _, err) = convert_48bit(&tmp.path("b.jpg"), &flags);
        assert_eq!(code, 0, "following `{offer}` must convert: {err}");
    }

    // A roll frame's explicit path is refused naming recipe keys: a roll takes no
    // conversion flags.
    let recipe = write_file(
        &tmp.path("roll.json"),
        r#"{"recipe_version": 3, "calibration": {"film_base": {"explicit": [0.9, 0.55, 0.42]}}}"#,
    );
    let frames = write_file(
        &tmp.path("frames.json"),
        &format!(
            r#"{{"frames": [{{"input": "{}", "output": "x.jpg"}}]}}"#,
            fixture("hdr-48bit.tif").display()
        ),
    );
    let (code, _, err) = run(&[
        "roll",
        "--frames",
        frames.to_str().unwrap(),
        "--out-dir",
        tmp.path("out").to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("`output.display.container` \"tiff\""), "{err}");
    assert!(err.contains("`output.display.container` \"jpeg\""), "{err}");
    assert!(
        !err.contains("--range") && !err.contains("--container"),
        "{err}"
    );
}

#[test]
fn a_recipe_film_master_suffix_refusal_names_no_flag_the_user_did_not_type() {
    let tmp = TempDir::new("recipe-master-suffix");
    let recipe = write_file(
        &tmp.path("master.json"),
        r#"{"recipe_version": 3, "output": "film-master"}"#,
    );
    let with_recipe = ["--params", recipe.to_str().unwrap()];
    let (code, _, err) = convert_48bit(&tmp.path("m.jpg"), &with_recipe);
    assert_eq!(code, 2, "{err}");
    assert!(
        !err.contains("drop --film-master"),
        "no such flag was typed: {err}"
    );
    assert!(
        err.contains("replace the recipe's `output` \"film-master\""),
        "{err}"
    );
    // The printed remedy, followed as written over the same recipe, converts.
    let (_, offers) = err
        .split_once("recipe's `output` \"film-master\": ")
        .unwrap_or_else(|| panic!("no offer: {err}"));
    for offer in offers.trim().split("; ") {
        let flags: Vec<&str> = offer.split_whitespace().collect();
        let (code, _, err) =
            convert_48bit(&tmp.path("m.jpg"), &[&with_recipe[..], &flags[..]].concat());
        assert_eq!(code, 0, "following `{offer}` must convert: {err}");
    }
    // With the flag typed, the remedy is to drop it.
    let (code, _, err) = convert_48bit(&tmp.path("f.jpg"), &["--film-master"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("drop --film-master"), "{err}");
}

#[test]
fn a_roll_frame_axis_joins_the_shared_recipes_axes() {
    // `output.display` states only its axes, so a shared range and a per-frame gamut
    // are two one-key objects — merged field by field, not switched.
    let tmp = TempDir::new("roll-axis-merge");
    let recipe = write_file(
        &tmp.path("roll.json"),
        r#"{"recipe_version": 3,
            "calibration": {"film_base": {"explicit": [0.9, 0.55, 0.42]}},
            "output": {"display": {"range": "hdr"}}}"#,
    );
    let frames = write_file(
        &tmp.path("frames.json"),
        &format!(
            r#"{{"frames": [
  {{"input": "{}", "params": {{"output": {{"display": {{"gamut": "srgb"}}}}}}}},
  {{"input": "{}"}}
]}}"#,
            fixture("hdr-48bit.tif").display(),
            fixture("hdri-64bit.tif").display()
        ),
    );
    let out_dir = tmp.path("out");
    let (code, stdout, err) = run(&[
        "roll",
        "--frames",
        frames.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);
    // Switched rather than merged, the frame's `gamut` alone would be an SDR TIFF.
    let axes = &report["frames"][0]["chain"]["destination"]["display"];
    assert_eq!(axes["range"], "hdr", "{report}");
    assert_eq!(axes["gamut"], "srgb", "{report}");
    // The other frame keeps the roll's destination: the Display P3 gain map.
    let axes = &report["frames"][1]["chain"]["destination"]["display"];
    assert_eq!(
        (axes["range"].as_str(), axes["gamut"].as_str()),
        (Some("hdr"), Some("display-p3")),
        "{report}"
    );
}

#[test]
fn a_roll_names_each_frame_from_its_destination() {
    // The shared recipe's `output` picks the container, so derived names follow it; a
    // per-frame override that changes the destination changes that frame's name.
    let tmp = TempDir::new("roll-destination");
    // A roll measurement is stated (neutral gains and a white), so the `--strict` runs
    // below see only the destination warning, not the `default` rendering's fallback.
    let recipe = write_file(
        &tmp.path("roll.json"),
        r#"{
  "recipe_version": 3,
  "calibration": { "film_base": { "explicit": [0.9, 0.55, 0.42] } },
  "roll": { "white_balance": [1.0, 1.0, 1.0], "white_stops": 2.0, "exposure": 0.0 },
  "output": { "display": { "transfer": "pq" } }
}"#,
    );
    // A third frame under its own stem, switched to the film master.
    let master = tmp.path("master.tif");
    std::fs::copy(fixture("hdr-48bit.tif"), &master).unwrap();
    let frames = write_file(
        &tmp.path("frames.json"),
        &format!(
            r#"{{"frames": [
  {{"input": "{}"}},
  {{"input": "{}", "params": {{"output": {{"display": {{"range": "hdr", "transfer": "native", "gamut": "display-p3", "container": "jpeg"}}}}}}}},
  {{"input": "{}", "params": {{"output": "film-master"}}}}
]}}"#,
            fixture("hdr-48bit.tif").display(),
            fixture("hdri-64bit.tif").display(),
            master.display()
        ),
    );
    let out_dir = tmp.path("out");
    let (code, stdout, err) = run(&[
        "roll",
        "--frames",
        frames.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(read_tiff_bits(&out_dir.join("hdr-48bit_positive.tiff")), 16);
    assert_eq!(
        sniff_container(&out_dir.join("hdri-64bit_positive.jpg")),
        "jpeg"
    );
    assert_eq!(read_tiff_bits(&out_dir.join("master_positive.tiff")), 32);
    let report = json(&stdout);
    assert_eq!(
        report["frames"][1]["chain"]["destination"]["display"]["container"], "jpeg",
        "{stdout}"
    );
    assert_eq!(report["frames"][2]["chain"]["destination"], "film-master");
    // A roll frame carries its encoder's block, as `convert` does; the gain map's is in
    // its chain, and the film master has none.
    assert_eq!(
        report["frames"][0]["hdr_coded_tiff"]["bits_per_sample"], 16,
        "{stdout}"
    );
    for i in [1, 2] {
        assert!(
            report["frames"][i].get("hdr_coded_tiff").is_none(),
            "{stdout}"
        );
    }
    assert!(
        report["frames"][1]["chain"]["gain_map"].is_object(),
        "{stdout}"
    );
    assert!(
        report["frames"][2]["chain"].get("gain_map").is_none(),
        "{stdout}"
    );
    // A frame switching the roll's destination is warned about, naming the frame.
    let warned: Vec<&str> = report["warnings"]
        .as_array()
        .expect("roll report must carry a warnings array")
        .iter()
        .filter_map(|w| w.as_str())
        .filter(|w| w.contains("resolves `output`"))
        .collect();
    assert_eq!(warned.len(), 2, "{report}");
    assert!(warned[0].contains("hdri-64bit.tif"), "{}", warned[0]);
    assert!(warned[1].contains("master.tif"), "{}", warned[1]);
    // `--strict` promotes it; the same roll with no per-frame `output` is the control.
    // (`hdr-48bit.tif` is IR-free; the control proves the other frame raises nothing.)
    let strict_roll = |frames: &Path, out: &str| {
        run(&[
            "roll",
            "--frames",
            frames.to_str().unwrap(),
            "--out-dir",
            tmp.path(out).to_str().unwrap(),
            "--params",
            recipe.to_str().unwrap(),
            "--strict",
        ])
    };
    let one = |params: &str| {
        format!(
            r#"{{"frames": [{{"input": "{}"{params}}}]}}"#,
            fixture("hdr-48bit.tif").display()
        )
    };
    let overridden = write_file(
        &tmp.path("overridden.json"),
        &one(r#", "params": {"output": "film-master"}"#),
    );
    let (code, _, err) = strict_roll(&overridden, "strict-a");
    assert_eq!(code, 1, "--strict must promote the warning: {err}");
    assert!(err.contains("resolves `output`"), "{err}");
    let control = write_file(&tmp.path("control.json"), &one(""));
    let (code, _, err) = strict_roll(&control, "strict-b");
    assert_eq!(code, 0, "{err}");
    // The roll's recipe names flag and key: the fault can come from either.
    let bad = write_file(
        &tmp.path("bad.json"),
        r#"{"recipe_version": 3, "calibration": {"film_base": {"explicit": [0.9, 0.55, 0.42]}},
            "output": {"display": {"gamut": "bt2020"}}}"#,
    );
    let (code, _, err) = run(&[
        "roll",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        bad.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("`.transfer`"), "{err}");
    assert!(err.contains("--transfer"), "{err}");
}

// --- layered `--params` (core/recipe-composition) ---------------------------------

/// A measured roll file: the film base and the roll section.
const MEASURED_LAYER: &str = r#"{"recipe_version": 2,
    "calibration": {"film_base": {"explicit": [0.9, 0.6, 0.5]}},
    "roll": {"white_balance": [1.05, 1.0, 0.95], "white_stops": 2.5}}"#;
/// A look: no measurement.
const LOOK_LAYER: &str = r#"{"recipe_version": 2,
    "scene_correction": {"exposure": 0.3},
    "look": {"channel_grade": [1.1, 0.95]}}"#;
/// The two, merged by hand.
const MERGED_LAYERS: &str = r#"{"recipe_version": 2,
    "calibration": {"film_base": {"explicit": [0.9, 0.6, 0.5]}},
    "roll": {"white_balance": [1.05, 1.0, 0.95], "white_stops": 2.5},
    "scene_correction": {"exposure": 0.3},
    "look": {"channel_grade": [1.1, 0.95]}}"#;

/// `convert` the IR-free fixture into `tmp/name` with `extra`, expecting success;
/// returns the report and the written bytes.
fn convert_ok(tmp: &TempDir, name: &str, extra: &[&str]) -> (serde_json::Value, Vec<u8>) {
    let out = tmp.path(name);
    let scan = fixture("hdr-48bit.tif");
    let mut args = vec![
        "convert",
        scan.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    let (code, stdout, err) = run(&args);
    assert_eq!(code, 0, "{args:?}:\n{err}");
    let report = json(&stdout);
    let written = PathBuf::from(report["output"].as_str().unwrap());
    (report, std::fs::read(written).unwrap())
}

/// A one-frame `--frames` manifest for the IR-free fixture, with `params` as its
/// override.
fn one_frame_manifest(path: &Path, params: &str) -> PathBuf {
    write_file(
        path,
        &format!(
            r#"{{"frames": [{{"input": "{}", "params": {params}}}]}}"#,
            fixture("hdr-48bit.tif").display()
        ),
    )
}

#[test]
fn two_layers_convert_as_their_merged_recipe() {
    let tmp = TempDir::new("layers-merge");
    let measured = write_file(&tmp.path("measured.json"), MEASURED_LAYER);
    let look = write_file(&tmp.path("look.json"), LOOK_LAYER);
    let merged = write_file(&tmp.path("merged.json"), MERGED_LAYERS);
    let (layered, layered_bytes) = convert_ok(
        &tmp,
        "layered",
        &[
            "--params",
            measured.to_str().unwrap(),
            "--params",
            look.to_str().unwrap(),
        ],
    );
    let (single, single_bytes) =
        convert_ok(&tmp, "single", &["--params", merged.to_str().unwrap()]);
    assert_eq!(layered["recipe"], single["recipe"]);
    assert_eq!(
        layered["identity"]["params_hash"],
        single["identity"]["params_hash"]
    );
    assert!(
        layered_bytes == single_bytes,
        "the layers render differently"
    );
    // Falsifiable: the measurement alone renders differently.
    let (_, alone) = convert_ok(&tmp, "alone", &["--params", measured.to_str().unwrap()]);
    assert!(alone != single_bytes, "the look layer changed nothing");
}

#[test]
fn a_later_layer_wins_and_a_flag_beats_every_layer_by_source() {
    let tmp = TempDir::new("layers-order");
    let measured = write_file(&tmp.path("measured.json"), MEASURED_LAYER);
    let a = write_file(
        &tmp.path("a.json"),
        r#"{"recipe_version": 2, "scene_correction":
            {"exposure": 0.3, "white_balance": {"explicit": [1.25, 1.0, 0.75]}}}"#,
    );
    let b = write_file(
        &tmp.path("b.json"),
        r#"{"recipe_version": 2, "scene_correction": {"exposure": -0.25}}"#,
    );
    let (m, a, b) = (
        measured.to_str().unwrap(),
        a.to_str().unwrap(),
        b.to_str().unwrap(),
    );
    let scene = |extra: &[&str], name: &str| {
        let (report, _) = convert_ok(&tmp, name, extra);
        report["recipe"]["scene_correction"].clone()
    };
    // Same key in both: the later one wins; a key only the earlier states survives.
    let ab = scene(&["--params", m, "--params", a, "--params", b], "ab");
    assert_eq!(ab["exposure"], -0.25, "{ab}");
    assert_eq!(
        ab["white_balance"],
        serde_json::json!({"explicit": [1.25, 1.0, 0.75]})
    );
    let ba = scene(&["--params", m, "--params", b, "--params", a], "ba");
    assert_eq!(ba["exposure"], 0.3, "{ba}");
    // An explicit neutral gain is a value, not "fall back to the layers".
    let flagged = scene(
        &[
            "--params",
            m,
            "--params",
            a,
            "--params",
            b,
            "--white-balance",
            "1,1,1",
        ],
        "flag",
    );
    assert_eq!(
        flagged["white_balance"],
        serde_json::json!({"explicit": [1.0, 1.0, 1.0]})
    );
    assert_eq!(flagged["exposure"], -0.25);
}

#[test]
fn a_complete_look_file_layered_after_the_measurement_keeps_it() {
    // `hanten params` states every key, the unset base and roll ones `null`. A `null`
    // states nothing, so either order keeps those; but its restated default
    // `reconstruction.linearization` wins wherever it lands, so the measured file
    // goes last.
    let tmp = TempDir::new("layers-null");
    let measured = write_file(
        &tmp.path("measured.json"),
        r#"{"recipe_version": 2,
            "calibration": {"film_base": {"explicit": [0.9, 0.6, 0.5]}},
            "reconstruction": {"linearization": 1.6},
            "roll": {"white_balance": [1.05, 1.0, 0.95], "white_stops": 2.5}}"#,
    );
    let (code, complete, err) = run(&["params"]);
    assert_eq!(code, 0, "{err}");
    assert!(complete.contains(r#""film_base": null"#), "{complete}");
    assert!(complete.contains(r#""linearization": 1.8"#), "{complete}");
    let look = write_file(&tmp.path("complete.json"), &complete);
    let (measured, look) = (measured.to_str().unwrap(), look.to_str().unwrap());
    for (name, layers, linearization) in [
        ("measured-last", [look, measured], 1.6),
        ("measured-first", [measured, look], 1.8),
    ] {
        let (report, _) = convert_ok(&tmp, name, &["--params", layers[0], "--params", layers[1]]);
        let recipe = &report["recipe"];
        assert_eq!(
            recipe["calibration"]["film_base"],
            serde_json::json!({"explicit": [0.9, 0.6, 0.5]}),
            "{name}"
        );
        assert_eq!(recipe["roll"]["white_stops"], 2.5, "{name}");
        let got = recipe["reconstruction"]["linearization"].as_f64().unwrap();
        assert!((got - linearization).abs() < 1e-6, "{name}: {got}");
    }
}

#[test]
fn a_dump_is_the_whole_run_and_a_look_only_once_stripped() {
    // A `--dump-params` file carries its roll's `roll.frames` table. Tables union and
    // absence states nothing, so layered under another roll's file its clamp survives;
    // with `calibration` and `roll` stripped, only the measured file's roll applies.
    let tmp = TempDir::new("layers-dump-as-look");
    let roll1 = write_file(
        &tmp.path("roll1.json"),
        r#"{"recipe_version": 2,
            "calibration": {"film_base": {"explicit": [0.9, 0.6, 0.5]}},
            "roll": {"white_balance": [1.05, 1.0, 0.95], "white_stops": 2.0,
                     "frames": {"hdr-48bit.tif": {"white_stops": 1.7}}}}"#,
    );
    let roll2 = write_file(
        &tmp.path("roll2.json"),
        r#"{"recipe_version": 2,
            "calibration": {"film_base": {"explicit": [0.8, 0.5, 0.4]}},
            "roll": {"white_balance": [1.0, 1.0, 1.0], "white_stops": 1.8}}"#,
    );
    // Dumped from another frame of roll 1, so the table stays a table.
    let other = tmp.path("other.tif");
    std::fs::copy(fixture("hdr-48bit.tif"), &other).unwrap();
    let dump = tmp.path("dump.json");
    let (code, _, err) = run(&[
        "convert",
        other.to_str().unwrap(),
        "-o",
        tmp.path("dumped").to_str().unwrap(),
        "--params",
        roll1.to_str().unwrap(),
        "--exposure",
        "0.3",
        "--dump-params",
        dump.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    let mut look: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&dump).unwrap()).unwrap();
    let obj = look.as_object_mut().unwrap();
    obj.remove("calibration");
    obj.remove("roll");
    let look = write_file(&tmp.path("look.json"), &look.to_string());
    let roll2 = roll2.to_str().unwrap();
    for (name, first, white_stops) in [
        ("unstripped", dump.to_str().unwrap(), 1.7),
        ("stripped", look.to_str().unwrap(), 1.8),
    ] {
        let (report, _) = convert_ok(&tmp, name, &["--params", first, "--params", roll2]);
        let recipe = &report["recipe"];
        assert_eq!(
            recipe["calibration"]["film_base"],
            serde_json::json!({"explicit": [0.8, 0.5, 0.4]}),
            "{name}"
        );
        assert_eq!(recipe["scene_correction"]["exposure"], 0.3, "{name}");
        let got = recipe["roll"]["white_stops"].as_f64().unwrap();
        assert!((got - white_stops).abs() < 1e-6, "{name}: {got}");
    }
}

#[test]
fn convert_refuses_a_report_file_or_a_layered_dump_over_a_params_layer() {
    let tmp = TempDir::new("layers-report-over-params");
    let measured = write_file(&tmp.path("measured.json"), MEASURED_LAYER);
    let look = write_file(&tmp.path("look.json"), LOOK_LAYER);
    let (m, l) = (measured.to_str().unwrap(), look.to_str().unwrap());
    let scan = fixture("hdr-48bit.tif");
    let out = tmp.path("out.tiff");
    let convert = |extra: &[&str]| {
        let mut args = vec![
            "convert",
            scan.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--params",
            m,
            "--params",
            l,
        ];
        args.extend_from_slice(extra);
        run(&args)
    };
    let (code, _, err) = convert(&["--report-file", l]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("--report-file") && err.contains("would overwrite the --params recipe"),
        "{err}"
    );
    assert!(!out.exists());
    assert_eq!(std::fs::read_to_string(&look).unwrap(), LOOK_LAYER);
    // A dump over one of several layers would fold the others into it.
    let (code, _, err) = convert(&["--dump-params", m]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("--params layer") && err.contains("measured.json"),
        "{err}"
    );
    assert!(err.contains("Dump to another path"), "{err}");
    assert!(!out.exists());
    assert_eq!(std::fs::read_to_string(&measured).unwrap(), MEASURED_LAYER);
    // Over the sole layer, rewriting the recipe it replays stays allowed.
    let (code, _, err) = run(&[
        "convert",
        scan.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--params",
        m,
        "--dump-params",
        m,
    ]);
    assert_eq!(code, 0, "{err}");
}

#[test]
fn params_dash_reads_stdin_once() {
    let tmp = TempDir::new("layers-stdin");
    let look = write_file(&tmp.path("look.json"), LOOK_LAYER);
    let merged = write_file(&tmp.path("merged.json"), MERGED_LAYERS);
    let scan = fixture("hdr-48bit.tif");
    let out = tmp.path("stdin.tiff");
    let (code, stdout, err) = run_stdin(
        &[
            "convert",
            scan.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--params",
            "-",
            "--params",
            look.to_str().unwrap(),
        ],
        MEASURED_LAYER,
    );
    assert_eq!(code, 0, "{err}");
    let (single, single_bytes) =
        convert_ok(&tmp, "single", &["--params", merged.to_str().unwrap()]);
    assert_eq!(json(&stdout)["recipe"], single["recipe"]);
    assert!(std::fs::read(&out).unwrap() == single_bytes);

    // Twice is refused before anything is read or written.
    let twice = tmp.path("twice.tiff");
    let (code, _, err) = run_stdin(
        &[
            "convert",
            scan.to_str().unwrap(),
            "-o",
            twice.to_str().unwrap(),
            "--params",
            "-",
            "--params",
            "-",
        ],
        MEASURED_LAYER,
    );
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("more than once"), "{err}");
    assert!(!twice.exists());
}

#[test]
fn a_layer_fault_names_its_file() {
    let tmp = TempDir::new("layers-fault");
    let measured = write_file(&tmp.path("measured.json"), MEASURED_LAYER);
    let bad = write_file(
        &tmp.path("typo.json"),
        r#"{"recipe_version": 2, "look": {"contrst": 1.2}}"#,
    );
    let scan = fixture("hdr-48bit.tif");
    let out = tmp.path("out");
    let (code, _, err) = run(&[
        "convert",
        scan.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--params",
        measured.to_str().unwrap(),
        "--params",
        bad.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("typo.json") && err.contains("contrst"),
        "{err}"
    );
}

#[test]
fn roll_runs_from_flags_alone_and_from_layers() {
    let tmp = TempDir::new("roll-flags");
    let scan = fixture("hdr-48bit.tif");
    let flags = [
        "--film-base",
        "0.9,0.6,0.5",
        "--exposure",
        "0.3",
        "--roll-white",
        "2.5",
    ];
    let (_, convert_bytes) = convert_ok(&tmp, "convert", &flags);
    let out_dir = tmp.path("flags");
    let mut args = vec![
        "roll",
        scan.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
    ];
    args.extend_from_slice(&flags);
    let (code, _, err) = run(&args);
    assert_eq!(code, 0, "{err}");
    assert!(std::fs::read(out_dir.join("hdr-48bit_positive.tiff")).unwrap() == convert_bytes);

    // Layers, as on `convert`.
    let measured = write_file(&tmp.path("measured.json"), MEASURED_LAYER);
    let look = write_file(&tmp.path("look.json"), LOOK_LAYER);
    let merged = write_file(&tmp.path("merged.json"), MERGED_LAYERS);
    let (_, merged_bytes) = convert_ok(&tmp, "merged", &["--params", merged.to_str().unwrap()]);
    let out_dir = tmp.path("layers");
    let (code, _, err) = run(&[
        "roll",
        scan.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--params",
        measured.to_str().unwrap(),
        "--params",
        look.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(std::fs::read(out_dir.join("hdr-48bit_positive.tiff")).unwrap() == merged_bytes);

    // A removed flag is refused on `roll` as on `convert`.
    let (code, _, err) = run(&[
        "roll",
        scan.to_str().unwrap(),
        "--out-dir",
        tmp.path("removed").to_str().unwrap(),
        "--film-base",
        "0.9,0.6,0.5",
        "--print-exposure",
        "1",
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--print-exposure was removed"), "{err}");
}

#[test]
fn a_frames_override_beats_a_roll_flag() {
    let tmp = TempDir::new("roll-frame-over-flag");
    let measured = write_file(&tmp.path("measured.json"), MEASURED_LAYER);
    let roll = |manifest: &Path, extra: &[&str], dir: &str| {
        let out_dir = tmp.path(dir);
        let mut args = vec![
            "roll",
            "--frames",
            manifest.to_str().unwrap(),
            "--out-dir",
            out_dir.to_str().unwrap(),
            "--params",
            measured.to_str().unwrap(),
            // The layer has no exposure; stating one keeps its warning out of these.
            "--roll-exposure",
            "0",
        ];
        args.extend_from_slice(extra);
        let (code, stdout, err) = run(&args);
        assert_eq!(code, 0, "{err}");
        json(&stdout)
    };
    // A clamp survives a roll-wide `--roll-white`, and is frame-local: no warning.
    let clamp = one_frame_manifest(&tmp.path("clamp.json"), r#"{"roll": {"white_stops": 1.5}}"#);
    let report = roll(&clamp, &["--roll-white", "3.0"], "clamp");
    assert_eq!(
        report["frames"][0]["chain"]["roll"]["white_stops"], 1.5,
        "{report}"
    );
    assert!(
        report["warnings"].as_array().is_none_or(Vec::is_empty),
        "{report}"
    );
    // A frame's value over a flag-set roll-wide value still warns, naming both…
    let gains = one_frame_manifest(
        &tmp.path("gains.json"),
        r#"{"roll": {"white_balance": [1.1, 1.0, 0.9]}}"#,
    );
    let report = roll(&gains, &["--roll-white-balance", "1,1,1"], "gains");
    let warnings = report["warnings"].to_string();
    assert!(
        warnings.contains("roll.white_balance") && warnings.contains("[1.0,1.0,1.0]"),
        "{warnings}"
    );
    // …and restating the flag's value is silent: the frame compares against the roll
    // as resolved, flags applied, not against the recipes alone.
    let same = one_frame_manifest(
        &tmp.path("same.json"),
        r#"{"roll": {"white_balance": [1, 1, 1]}}"#,
    );
    let report = roll(&same, &["--roll-white-balance", "1,1,1"], "same");
    assert!(
        report["warnings"].as_array().is_none_or(Vec::is_empty),
        "{report}"
    );
}

#[test]
fn a_frames_null_is_refused() {
    // The merge skips a `null`, so a frame's `null` (an attempt to unset) would do
    // nothing: it is refused, naming the frame and the key, before anything is written.
    let tmp = TempDir::new("roll-frame-null");
    let shared = write_file(
        &tmp.path("shared.json"),
        r#"{"recipe_version": 3, "calibration": {"film_base": {"explicit": [0.9, 0.6, 0.5]}},
            "look": {"contrast": 1.3}}"#,
    );
    let manifest = one_frame_manifest(
        &tmp.path("m.json"),
        r#"{"scene_correction": {"exposure": 0.2}, "look": {"contrast": null}}"#,
    );
    let out = tmp.path("out");
    let (code, _, err) = run(&[
        "roll",
        "--frames",
        manifest.to_str().unwrap(),
        "--out-dir",
        out.to_str().unwrap(),
        "--params",
        shared.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("frame ") && err.contains("hdr-48bit.tif"),
        "{err}"
    );
    assert!(err.contains("`look.contrast` is null"), "{err}");
    assert!(err.contains("Omit `look.contrast`"), "{err}");
    assert!(!out.exists());

    // A mistyped key is that fault, even when its value is null.
    let typo = one_frame_manifest(&tmp.path("typo.json"), r#"{"look": {"contrst": null}}"#);
    let (code, _, err) = run(&[
        "roll",
        "--frames",
        typo.to_str().unwrap(),
        "--out-dir",
        out.to_str().unwrap(),
        "--params",
        shared.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("unknown field `contrst`"), "{err}");
    assert!(!err.contains("is null"), "{err}");

    // A null inside an array is named by its index.
    let element = one_frame_manifest(
        &tmp.path("element.json"),
        r#"{"roll": {"white_balance": [1, null, 1]}}"#,
    );
    let (code, _, err) = run(&[
        "roll",
        "--frames",
        element.to_str().unwrap(),
        "--out-dir",
        out.to_str().unwrap(),
        "--params",
        shared.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("`roll.white_balance[1]` is null"), "{err}");

    // A null inside a tagged value that switches variant is refused by path too.
    let switch = one_frame_manifest(
        &tmp.path("switch.json"),
        r#"{"calibration": {"film_base": {"region": [1, 2, 3, null]}}}"#,
    );
    let (code, _, err) = run(&[
        "roll",
        "--frames",
        switch.to_str().unwrap(),
        "--out-dir",
        out.to_str().unwrap(),
        "--params",
        shared.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("`calibration.film_base.region[3]` is null"),
        "{err}"
    );
}

#[test]
fn a_frames_input_axis_is_not_credited_to_the_flag() {
    // The manifest `params` win over the flags, so an input axis they state is the
    // recipe's in the frame's provenance; the axis they leave stays the flag's.
    let tmp = TempDir::new("roll-frame-input-provenance");
    let manifest = one_frame_manifest(&tmp.path("m.json"), r#"{"input": {"transfer": "linear"}}"#);
    let (code, stdout, err) = run(&[
        "roll",
        "--frames",
        manifest.to_str().unwrap(),
        "--out-dir",
        tmp.path("out").to_str().unwrap(),
        "--film-base",
        "0.9,0.6,0.5",
        "--input-transfer",
        "linear",
        "--input-meaning",
        "scanner-device",
    ]);
    assert_eq!(code, 0, "{err}");
    let report = json(&stdout);
    let provenance = |axis: &str| {
        report["frames"][0]["input_color"]["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["axis"] == axis && e["kind"] == "user-assertion")
            .map(|e| e["provenance"].clone())
    };
    assert_eq!(
        provenance("transfer").unwrap(),
        "input.transfer (recipe)",
        "{report}"
    );
    assert_eq!(
        provenance("meaning").unwrap(),
        "--input-meaning (CLI flag)",
        "{report}"
    );
}

#[test]
fn roll_names_the_params_recipe_a_report_file_would_overwrite() {
    let tmp = TempDir::new("roll-report-over-params");
    let recipe = write_file(&tmp.path("r.json"), MEASURED_LAYER);
    let r = recipe.to_str().unwrap();
    let (code, _, err) = run(&[
        "roll",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "--out-dir",
        tmp.path("out").to_str().unwrap(),
        "--params",
        r,
        "--report-file",
        r,
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("would overwrite the --params recipe"), "{err}");
    assert!(!err.contains("input scan"), "{err}");
    assert_eq!(std::fs::read_to_string(&recipe).unwrap(), MEASURED_LAYER);
}

#[test]
fn a_bad_roll_flag_is_not_blamed_on_the_recipe() {
    // The roll's recipe includes the flags, so its fault is reported as `convert`
    // reports it: naming the flag, with no "the roll's recipe" prefix.
    let tmp = TempDir::new("roll-bad-flag");
    let (code, _, err) = run(&[
        "roll",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "--out-dir",
        tmp.path("out").to_str().unwrap(),
        "--film-base",
        "0.9,0.6,0.5",
        "--contrast",
        "0",
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--contrast"), "{err}");
    assert!(!err.contains("the roll's recipe"), "{err}");
}

#[test]
fn measure_roll_warns_on_a_layer_from_another_pipeline_version() {
    let tmp = TempDir::new("measure-roll-layer-skew");
    let base = write_file(
        &tmp.path("base.json"),
        r#"{"recipe_version": 2, "calibration": {"film_base": {"explicit": [0.9, 0.6, 0.5]}}}"#,
    );
    let stale = write_file(
        &tmp.path("stale.json"),
        r#"{"meta": {"pipeline_version": 9999}, "params": {"recipe_version": 2}}"#,
    );
    let (code, stdout, err) = run(&[
        "measure-roll",
        fixture("hdr-48bit.tif").to_str().unwrap(),
        "--params",
        base.to_str().unwrap(),
        "--params",
        stale.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    let warnings = json(&stdout)["warnings"].to_string();
    assert!(warnings.contains("pipeline_version 9999"), "{warnings}");
}

#[test]
fn measure_roll_composes_its_params_layers() {
    let tmp = TempDir::new("measure-roll-layers");
    let base = write_file(
        &tmp.path("base.json"),
        r#"{"recipe_version": 2, "calibration": {"film_base": {"explicit": [0.9, 0.6, 0.5]}}}"#,
    );
    let decode = write_file(
        &tmp.path("decode.json"),
        r#"{"recipe_version": 2, "reconstruction": {"linearization": 1.6}}"#,
    );
    let both = write_file(
        &tmp.path("both.json"),
        r#"{"recipe_version": 2, "calibration": {"film_base": {"explicit": [0.9, 0.6, 0.5]}},
            "reconstruction": {"linearization": 1.6}}"#,
    );
    let scan = fixture("hdr-48bit.tif");
    let measure = |layers: &[&Path]| {
        let mut args = vec!["measure-roll", scan.to_str().unwrap()];
        for l in layers {
            args.extend(["--params", l.to_str().unwrap()]);
        }
        let (code, stdout, err) = run(&args);
        assert_eq!(code, 0, "{err}");
        let mut report = json(&stdout);
        report.as_object_mut().unwrap().remove("elapsed_ms");
        report
    };
    let layered = measure(&[&base, &decode]);
    assert_eq!(layered, measure(&[&both]));
    // Falsifiable: the decode layer moves the measurement.
    assert_ne!(layered, measure(&[&base]));
}

#[test]
fn a_roll_flag_beats_a_frames_table_entry_as_on_convert() {
    // A `roll.frames` entry is recipe, applied before the flags: `--roll-white` beats it
    // on `roll` exactly as on `convert`, and each frame still matches its `convert`.
    let tmp = TempDir::new("roll-table-flag");
    let measured = write_file(
        &tmp.path("roll.json"),
        r#"{"recipe_version": 2,
            "calibration": {"film_base": {"explicit": [0.9, 0.6, 0.5]}},
            "roll": {"white_stops": 2.5, "frames": {"hdr-48bit.tif": {"white_stops": 1.5}}}}"#,
    );
    let scan = fixture("hdr-48bit.tif");
    let roll = |extra: &[&str], dir: &str| {
        let out_dir = tmp.path(dir);
        let mut args = vec![
            "roll",
            scan.to_str().unwrap(),
            "--out-dir",
            out_dir.to_str().unwrap(),
            "--params",
            measured.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        let (code, stdout, err) = run(&args);
        assert_eq!(code, 0, "{err}");
        let report = json(&stdout);
        let white = report["frames"][0]["chain"]["roll"]["white_stops"].clone();
        (
            white,
            std::fs::read(out_dir.join("hdr-48bit_positive.tiff")).unwrap(),
        )
    };
    let m = measured.to_str().unwrap();
    let (white, bytes) = roll(&[], "entry");
    assert_eq!(white, 1.5, "the entry applies");
    let (_, convert_bytes) = convert_ok(&tmp, "entry", &["--params", m]);
    assert!(bytes == convert_bytes);
    let (white, bytes) = roll(&["--roll-white", "3.0"], "flag");
    assert_eq!(white, 3.0, "the flag beats the entry");
    let (_, convert_bytes) = convert_ok(&tmp, "flag", &["--params", m, "--roll-white", "3.0"]);
    assert!(bytes == convert_bytes);
}

// ---------------------------------------------------------------------------
// A reader that goes away (`core/stdout-broken-pipe-safety`)
// ---------------------------------------------------------------------------

/// Which of the child's output streams [`run_reader_gone`] closes.
#[derive(Clone, Copy)]
enum Gone {
    Stdout,
    Stderr,
}

/// Run with one output stream a pipe whose reader has already exited — `hanten … |
/// head` once `head` is done, made deterministic. Returns the exit code and the other
/// stream.
fn run_reader_gone(args: &[&str], gone: Gone) -> (i32, String) {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let mut cmd = Command::new(NC);
    cmd.args(args);
    match gone {
        Gone::Stdout => cmd.stdout(writer),
        Gone::Stderr => cmd.stderr(writer),
    };
    let out = cmd.output().expect("failed to spawn nc binary");
    let other = match gone {
        Gone::Stdout => out.stderr,
        Gone::Stderr => out.stdout,
    };
    (
        out.status.code().expect("process terminated by signal"),
        String::from_utf8(other).unwrap(),
    )
}

const BASE: [&str; 2] = ["--film-base", "0.9,0.55,0.42"];

#[test]
fn a_closed_stdout_is_not_a_failure_and_the_run_finishes() {
    let tmp = TempDir::new("stdout-gone");
    let fix = fixture("hdr-48bit.tif");
    let fix = fix.to_str().unwrap();
    let out = tmp.path("out.tiff");
    let recipe = tmp.path("base.json");
    let roll_recipe = tmp.path("roll.json");
    let roll_dir = tmp.path("roll");
    let runs: [Vec<&str>; 6] = [
        vec!["params"],
        vec!["inspect", fix],
        [&["convert", fix, "-o", out.to_str().unwrap()][..], &BASE].concat(),
        // `--out` is written after the report.
        vec!["measure-base", fix, "--out", recipe.to_str().unwrap()],
        [
            &["measure-roll", fix, "--out", roll_recipe.to_str().unwrap()][..],
            &BASE,
        ]
        .concat(),
        [
            &["roll", fix, "--out-dir", roll_dir.to_str().unwrap()][..],
            &BASE,
        ]
        .concat(),
    ];
    for args in &runs {
        let (code, err) = run_reader_gone(args, Gone::Stdout);
        assert_eq!(code, 0, "{args:?}: {err}");
        assert!(!err.contains("panicked"), "{args:?}: {err}");
        assert!(
            !err.contains("reader closed"),
            "silent without -v: {args:?}: {err}"
        );
    }
    assert!(is_tiff(&out));
    assert!(recipe.exists(), "measure-base's recipe is written");
    assert!(roll_recipe.exists(), "measure-roll's recipe is written");
    assert!(is_tiff(&roll_dir.join("hdr-48bit_positive.tiff")));
}

#[test]
fn a_closed_stdout_keeps_the_runs_own_exit_code() {
    let tmp = TempDir::new("stdout-gone-exit");
    // `--strict` is gated after the report: the IR plane's warning fails it.
    let fix = fixture("hdri-64bit.tif");
    let out = tmp.path("out.tiff");
    let args = [
        &[
            "convert",
            fix.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ][..],
        &BASE,
        &["--strict"],
    ]
    .concat();
    let (code, err) = run_reader_gone(&args, Gone::Stdout);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("--strict"), "{err}");

    // A roll's failed frame is gated after the report too.
    let good = fixture("hdr-48bit.tif");
    let missing = tmp.path("does-not-exist.tif");
    let roll_dir = tmp.path("roll");
    let args = [
        &[
            "roll",
            good.to_str().unwrap(),
            missing.to_str().unwrap(),
            "--out-dir",
            roll_dir.to_str().unwrap(),
        ][..],
        &BASE,
    ]
    .concat();
    let (code, err) = run_reader_gone(&args, Gone::Stdout);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("1 of 2 frame(s) failed"), "{err}");
}

#[test]
fn a_closed_stdout_is_one_info_line_and_no_telemetry_warning() {
    let tmp = TempDir::new("stdout-gone-telemetry");
    let fix = fixture("hdr-48bit.tif");
    let out = tmp.path("out.tiff");
    let args = [
        &[
            "convert",
            fix.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ][..],
        &BASE,
        &["--telemetry-file", "-", "-v"],
    ]
    .concat();
    let (code, err) = run_reader_gone(&args, Gone::Stdout);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("the report was not read"), "{err}");
    assert!(err.contains("the telemetry event was not read"), "{err}");
    assert!(!err.contains("could not write"), "{err}");
}

#[test]
fn a_closed_stderr_is_not_a_failure() {
    let tmp = TempDir::new("stderr-gone");
    let fix = fixture("hdr-48bit.tif");
    let out = tmp.path("out.tiff");
    // `-v` writes progress to stderr throughout the run.
    let args = [
        &[
            "convert",
            fix.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "-v",
        ][..],
        &BASE,
    ]
    .concat();
    let (code, stdout) = run_reader_gone(&args, Gone::Stderr);
    assert_eq!(code, 0);
    assert_eq!(json(&stdout)["command"], "convert");
    assert!(is_tiff(&out));

    // The error message itself goes to stderr; the exit code still lands.
    let (code, _) = run_reader_gone(&["params", "--new-flow"], Gone::Stderr);
    assert_eq!(code, 2);
}

/// Any stdout failure other than a closed pipe is a write error. Linux only: macOS
/// has no `/dev/full`.
#[cfg(target_os = "linux")]
#[test]
fn a_failing_stdout_is_a_write_error() {
    let fix = fixture("hdr-48bit.tif");
    let runs = [
        (vec!["params"], "writing params to stdout"),
        (
            vec!["inspect", fix.to_str().unwrap()],
            "writing the report to stdout",
        ),
    ];
    for (args, message) in &runs {
        let full = std::fs::File::options()
            .write(true)
            .open("/dev/full")
            .unwrap();
        let out = Command::new(NC)
            .args(args)
            .stdout(full)
            .output()
            .expect("failed to spawn nc binary");
        let err = String::from_utf8(out.stderr).unwrap();
        assert_eq!(out.status.code(), Some(5), "{args:?}: {err}");
        assert!(err.contains(message), "{args:?}: {err}");
    }
}

/// A value that would hand fit range an unusable film base or a non-finite sample is a
/// usage error naming the knob, before anything is decoded — never fit range's
/// internal-invariant error.
#[test]
fn a_value_that_cannot_render_is_a_usage_error_naming_the_knob() {
    let tmp = TempDir::new("cannot-render");
    let input = fixture("hdr-48bit.tif");
    let out = tmp.path("out");
    for (value, knob) in [
        ("--contrast=100", "--contrast (recipe `look.contrast`)"),
        (
            "--density-offset=-30,-30,-30",
            "--density-offset (recipe `reconstruction.offset`)",
        ),
        (
            "--density-scale=100,100,100",
            "--density-scale (recipe `reconstruction.scale`)",
        ),
    ] {
        let (code, _, err) = run(&[
            "convert",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
            value,
        ]);
        assert_eq!(code, 2, "{value}: {err}");
        assert!(
            err.contains(&format!("It renders with {knob} at its default")),
            "{value}: {err}"
        );
        assert!(!err.contains("fit range"), "{value}: {err}");
        assert!(!out.with_extension("tiff").exists(), "{value}");
    }

    // `roll` refuses a `roll.frames` entry by its own key, before any frame is written.
    let recipe = tmp.path("roll.json");
    std::fs::write(
        &recipe,
        r#"{"recipe_version": 3,
            "calibration": {"film_base": {"explicit": [0.9, 0.55, 0.42]}},
            "roll": {"white_stops": 2.5, "frames": {"hdr-48bit.tif": {"white_stops": 0.001}}}}"#,
    )
    .unwrap();
    let out_dir = tmp.path("roll");
    let (code, _, err) = run(&[
        "roll",
        input.to_str().unwrap(),
        "--params",
        recipe.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains(
            r#"It renders with recipe `roll.frames."hdr-48bit.tif".white_stops` at its default"#
        ),
        "{err}"
    );
    assert!(!out_dir.exists() || std::fs::read_dir(&out_dir).unwrap().next().is_none());
}

/// A channel the encoder writes as 0 everywhere is in range, so no loss counter sees it;
/// it warns, and `--strict` promotes the warning.
#[test]
fn a_channel_rendered_black_everywhere_warns() {
    const MARKER: &str = "no written sample is above 0";
    let tmp = TempDir::new("black-channel");
    let input = fixture("hdr-48bit.tif");
    let convert = |name: &str, extra: &[&str]| {
        let out = tmp.path(name);
        let mut argv = vec![
            "convert",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--film-base",
            "0.9,0.55,0.42",
        ];
        argv.extend_from_slice(extra);
        run(&argv)
    };
    let warnings = |stdout: &str| -> Vec<String> {
        json(stdout)["warnings"]
            .as_array()
            .map(|a| a.iter().map(|w| w.as_str().unwrap().to_string()).collect())
            .unwrap_or_default()
    };
    // A measured roll white: under `MEASURED`'s contrast the exposure below would also
    // drive the film base to 0, which is a usage error rather than a black render.
    let mut measured = vec![
        "--roll-white",
        "2.5",
        "--roll-white-balance",
        "1,1,1",
        "--roll-exposure",
        "0",
    ];
    let roll = measured.clone();
    let (code, stdout, err) = convert("normal", &measured);
    assert_eq!(code, 0, "{err}");
    assert!(
        !warnings(&stdout).iter().any(|w| w.contains(MARKER)),
        "{stdout}"
    );

    measured.push("--exposure=-100");
    let (code, stdout, err) = convert("black", &measured);
    assert_eq!(code, 0, "{err}");
    assert!(
        warnings(&stdout)
            .iter()
            .any(|w| w.contains("above 0 in the red, green and blue channels")),
        "{stdout}"
    );
    measured.push("--strict");
    let (code, _, err) = convert("strict", &measured);
    assert_ne!(code, 0, "--strict must refuse: {err}");

    // One channel, through the white balance.
    let mut one = roll;
    one.push("--white-balance=1e-30,1,1");
    let (code, stdout, err) = convert("red", &one);
    assert_eq!(code, 0, "{err}");
    assert!(
        warnings(&stdout)
            .iter()
            .any(|w| w.contains("above 0 in the red channel:")),
        "{stdout}"
    );

    // The film master, whose decode underflows to 0.
    let (code, stdout, err) = convert("master", &["--film-master", "--density-offset=-30,-30,-30"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        warnings(&stdout).iter().any(|w| w.contains(MARKER)),
        "{stdout}"
    );
}

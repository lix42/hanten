use serde_json::Value;

use super::*;
use crate::destination::{Container, DisplayAxes, Gamut, ROWS, Range, Status, Transfer};
use crate::stage::StageKind;
use crate::telemetry::{ConversionInfo, ImageInfo, OutcomeInfo};

const CONTRACT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/contracts/telemetry/upload-v1");
const SCHEMA_ID: &str = "https://hanten.invalid/telemetry/upload-v1.schema.json";

fn read(path: &str) -> String {
    std::fs::read_to_string(format!("{CONTRACT}/{path}")).unwrap()
}

fn json(path: &str) -> Value {
    serde_json::from_str(&read(path)).unwrap()
}

/// The request schema, its `$defs.envelope` and `$defs.response`, compiled from the
/// checked-in file.
struct Contract {
    schemas: boon::Schemas,
    request: boon::SchemaIndex,
    envelope: boon::SchemaIndex,
    response: boon::SchemaIndex,
}

impl Contract {
    fn load() -> Self {
        let mut schemas = boon::Schemas::new();
        let mut compiler = boon::Compiler::new();
        compiler
            .add_resource(SCHEMA_ID, json("upload-v1.schema.json"))
            .unwrap();
        let request = compiler.compile(SCHEMA_ID, &mut schemas).unwrap();
        let envelope = compiler
            .compile(&format!("{SCHEMA_ID}#/$defs/envelope"), &mut schemas)
            .unwrap();
        let response = compiler
            .compile(&format!("{SCHEMA_ID}#/$defs/response"), &mut schemas)
            .unwrap();
        Contract {
            schemas,
            request,
            envelope,
            response,
        }
    }

    fn accepts_request(&self, v: &Value) -> bool {
        self.schemas.validate(v, self.request).is_ok()
    }

    fn accepts_envelope(&self, v: &Value) -> bool {
        self.schemas.validate(v, self.envelope).is_ok()
    }

    fn accepts_response(&self, v: &Value) -> bool {
        self.schemas.validate(v, self.response).is_ok()
    }
}

/// `(name, body)` for every case in a corpus file.
fn cases(path: &str, body: &str) -> Vec<(String, Value)> {
    let Value::Array(cases) = json(path) else {
        panic!("{path} is not an array")
    };
    cases
        .into_iter()
        .map(|c| (c["name"].as_str().unwrap().to_owned(), c[body].clone()))
        .collect()
}

fn corpus_event(name: &str) -> Value {
    let (_, request) = cases("requests/valid.json", "request")
        .into_iter()
        .find(|(n, _)| n == name)
        .unwrap();
    request["events"][0].clone()
}

fn request(events: &[&UploadEvent]) -> Value {
    serde_json::json!({ "upload_schema_version": UPLOAD_SCHEMA_VERSION, "events": events })
}

fn sdr_p3_tiff() -> OutputSection {
    OutputSection::Display(DisplayAxes {
        range: Some(Range::Sdr),
        transfer: Some(Transfer::Native),
        gamut: Some(Gamut::DisplayP3),
        container: Some(Container::Tiff),
    })
}

/// A finished SDR conversion; its projection is the corpus's `full-success`.
fn full() -> TelemetryEvent {
    TelemetryEvent {
        schema_version: SCHEMA_VERSION,
        event_id: EventId([
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54,
            0x32, 0x10,
        ]),
        event: EventName::Conversion,
        command: CommandKind::Convert,
        timestamp_ms: 1_781_000_000_000,
        nc_version: "0.1.0".into(),
        target: "aarch64-apple-darwin".into(),
        cpu_count: Some(11),
        stage: EventStage::Finalize,
        image: Some(ImageInfo {
            format: SilverFastFormat::Hdri,
            width: 5000,
            height: 3740,
            megapixels: 18.7,
            bit_depth: 16,
            channels: 3,
            ir_present: true,
            input_bytes: Some(200_000_000),
            output_bytes: Some(112_000_000),
        }),
        timing_ms: TimingInfo {
            total: 1834.4,
            decode: Some(411.6),
            film_base: Some(38.2),
            reconstruction: Some(205.0),
            scene_correction: Some(12.49),
            look: Some(96.5),
            fit_range: Some(64.0),
            fit_gamut: Some(71.0),
            destination: Some(188.0),
            encode: Some(530.0),
            ir_export: Some(61.0),
        },
        conversion: Some(ConversionInfo {
            destination: sdr_p3_tiff(),
            params_hash: "a6bcbaf9b33f4480".into(),
            film_base_source: FilmBaseProvenance::Region([10, 20, 300, 200]),
            output_depth: "u16".into(),
        }),
        outcome: OutcomeInfo {
            status: OutcomeStatus::Success,
            error_kind: ErrorKind::None,
            exit_code: 0,
            warnings: 1,
            total_samples: Some(56_100_000),
            clipped: Some(20_000),
            non_finite: Some(0),
        },
    }
}

/// A usage failure in setup; its projection is the corpus's `minimal-failure`.
fn minimal() -> TelemetryEvent {
    TelemetryEvent {
        event_id: EventId([0; 16]),
        timestamp_ms: 0,
        target: "".into(),
        cpu_count: None,
        stage: EventStage::Setup,
        image: None,
        timing_ms: TimingInfo {
            total: 0.3,
            ..TimingInfo::default()
        },
        conversion: None,
        outcome: OutcomeInfo {
            status: OutcomeStatus::Failure,
            error_kind: ErrorKind::Usage,
            exit_code: 2,
            warnings: 0,
            total_samples: None,
            clipped: None,
            non_finite: None,
        },
        ..full()
    }
}

fn project(event: &TelemetryEvent) -> UploadEvent {
    to_upload_event(event).unwrap()
}

#[test]
fn the_schema_accepts_every_valid_request_and_rejects_every_invalid_one() {
    let contract = Contract::load();
    let valid = cases("requests/valid.json", "request");
    let invalid = cases("requests/invalid.json", "request");
    assert!(valid.len() > 10 && invalid.len() > 200, "corpus shrank");
    for (name, request) in valid {
        assert!(contract.accepts_request(&request), "valid/{name} rejected");
    }
    for (name, request) in invalid {
        assert!(
            !contract.accepts_request(&request),
            "invalid/{name} accepted"
        );
    }
}

#[test]
fn the_schema_accepts_every_valid_response_and_rejects_every_invalid_one() {
    let contract = Contract::load();
    for (name, response) in cases("responses/valid.json", "response") {
        assert!(
            contract.accepts_response(&response),
            "valid/{name} rejected"
        );
    }
    for (name, response) in cases("responses/invalid.json", "response") {
        assert!(
            !contract.accepts_response(&response),
            "invalid/{name} accepted"
        );
    }
}

#[test]
fn the_envelope_rejects_exactly_the_http_400_cases() {
    // The Worker answers 400 from `$defs/envelope` alone, then judges each event.
    let contract = Contract::load();
    for (name, request) in cases("requests/valid.json", "request") {
        assert!(contract.accepts_envelope(&request), "valid/{name}");
    }
    let Value::Array(cases) = json("requests/invalid.json") else {
        panic!()
    };
    for case in cases {
        let http_400 = case["expect"] == "http_400";
        assert_eq!(
            !contract.accepts_envelope(&case["request"]),
            http_400,
            "invalid/{}",
            case["name"]
        );
    }
}

/// Whether `err`'s tree holds a failure of `pred`'s keyword.
fn fails_on(err: &boon::ValidationError, pred: &dyn Fn(&boon::ErrorKind) -> bool) -> bool {
    pred(&err.kind) || err.causes.iter().any(|c| fails_on(c, pred))
}

#[test]
fn out_of_range_means_a_numeric_bound_and_unsupported_version_the_version() {
    let contract = Contract::load();
    let Value::Array(cases) = json("requests/invalid.json") else {
        panic!()
    };
    let bound = |k: &boon::ErrorKind| {
        matches!(
            k,
            boon::ErrorKind::Minimum { .. }
                | boon::ErrorKind::Maximum { .. }
                | boon::ErrorKind::ExclusiveMinimum { .. }
                | boon::ErrorKind::ExclusiveMaximum { .. }
        )
    };
    for case in cases {
        let (name, expect, request) = (&case["name"], &case["expect"], &case["request"]);
        if expect == "http_400" {
            continue;
        }
        if expect == "unsupported_version" {
            // Only the version is wrong: restoring it makes the request valid.
            let mut fixed = request.clone();
            for event in fixed["events"].as_array_mut().unwrap() {
                event["source_schema_version"] = SCHEMA_VERSION.into();
            }
            assert!(contract.accepts_request(&fixed), "{name}");
            continue;
        }
        let err = contract
            .schemas
            .validate(request, contract.request)
            .unwrap_err();
        assert_eq!(fails_on(&err, &bound), expect == "out_of_range", "{name}");
    }
}

#[test]
fn every_invalid_request_names_what_the_worker_answers() {
    let Value::Array(cases) = json("requests/invalid.json") else {
        panic!()
    };
    let expects = [
        "http_400",
        "invalid_field",
        "unsupported_version",
        "out_of_range",
    ];
    for case in cases {
        let expect = case["expect"].as_str().unwrap();
        assert!(expects.contains(&expect), "{}: {expect}", case["name"]);
    }
}

#[test]
fn upload_wire_shape_is_pinned() {
    // Snapshot the exact bytes of a full and a minimal projection, and hold the
    // corpus to them. A change here is a change to the Worker's contract.
    let full = serde_json::to_string(&project(&full())).unwrap();
    let expected_full = concat!(
        r#"{"source_schema_version":11,"event_id":"0123456789abcdeffedcba9876543210","#,
        r#""event_day":20613,"event_name":"conversion","nc_version":"0.1.0","#,
        r#""platform":{"os":"macos","arch":"aarch64","cpu_bucket":"8"},"#,
        r#""command":"convert","stage":"finalize","#,
        r#""outcome":{"status":"success","exit_code":0,"error_kind":"none","#,
        r#""warning_bucket":"1","clipped_fraction_bucket":"lt_0_1_pct","non_finite":false},"#,
        r#""timing_ms":{"total":1834,"decode":412,"film_base":38,"reconstruction":205,"#,
        r#""scene_correction":12,"look":97,"fit_range":64,"fit_gamut":71,"destination":188,"#,
        r#""encode":530,"ir_export":61},"#,
        r#""image":{"format":"hdri","megapixels_tenths":187,"#,
        r#""input_size_bucket":"128_511_mib","bit_depth":16,"ir_present":true},"#,
        r#""conversion":{"encoding":"sdr_tiff","film_base_source":"region","ir_exported":true}}"#,
    );
    assert_eq!(full, expected_full);

    let minimal = serde_json::to_string(&project(&minimal())).unwrap();
    let expected_minimal = concat!(
        r#"{"source_schema_version":11,"event_id":"00000000000000000000000000000000","#,
        r#""event_day":0,"event_name":"conversion","nc_version":"0.1.0","#,
        r#""platform":{"os":"unknown","arch":"unknown","cpu_bucket":"unknown"},"#,
        r#""command":"convert","stage":"setup","#,
        r#""outcome":{"status":"failure","exit_code":2,"error_kind":"usage","#,
        r#""warning_bucket":"0"},"timing_ms":{"total":0}}"#,
    );
    assert_eq!(minimal, expected_minimal);

    for (name, text) in [("full-success", full), ("minimal-failure", minimal)] {
        let projected: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(projected, corpus_event(name), "corpus {name} drifted");
    }
}

#[test]
fn every_local_timing_field_is_uploaded() {
    // `full()` sets every local timing field, so a new one the projection drops
    // shows up as a missing key.
    let keys = |v: Value| -> Vec<String> { v.as_object().unwrap().keys().cloned().collect() };
    let local = keys(serde_json::to_value(full().timing_ms).unwrap());
    let upload = keys(serde_json::to_value(project(&full()).timing_ms).unwrap());
    assert_eq!(local, upload);
}

#[test]
fn every_projection_satisfies_the_schema() {
    let contract = Contract::load();
    let mut events = vec![full(), minimal()];
    // Every writable destination, and the film master.
    for row in ROWS {
        if matches!(row.status, Status::Ready(_)) {
            let mut e = full();
            e.conversion.as_mut().unwrap().destination = OutputSection::Display(DisplayAxes {
                range: Some(row.range),
                transfer: Some(row.transfer),
                gamut: Some(row.gamut),
                container: Some(row.container),
            });
            events.push(e);
        }
    }
    let mut film_master = full();
    film_master.conversion.as_mut().unwrap().destination = OutputSection::FilmMaster;
    events.push(film_master);
    // Every failure the local event can record, in each stage.
    let kinds = [
        (ErrorKind::Usage, 2),
        (ErrorKind::Decode, 3),
        (ErrorKind::Unsupported, 4),
        (ErrorKind::Write, 5),
        (ErrorKind::Resource, 6),
        (ErrorKind::Strict, 1),
        (ErrorKind::Other, 1),
    ];
    let stages = [
        EventStage::Parse,
        EventStage::Setup,
        EventStage::Preflight,
        EventStage::Finalize,
    ]
    .into_iter()
    .chain(StageKind::ALL.map(EventStage::Stage));
    for stage in stages {
        for (kind, exit_code) in kinds {
            let mut e = minimal();
            e.stage = stage;
            e.outcome.error_kind = kind;
            e.outcome.exit_code = exit_code;
            events.push(e);
        }
    }
    // Failures after the frame finished keep image, conversion and the clip fields.
    for (kind, exit_code) in [(ErrorKind::Strict, 1), (ErrorKind::Write, 5)] {
        let mut e = full();
        e.outcome.status = OutcomeStatus::Failure;
        e.outcome.error_kind = kind;
        e.outcome.exit_code = exit_code;
        assert!(e.image.is_some() && e.outcome.clipped.is_some());
        events.push(e);
    }
    // A misread per-pixel depth, a clip with no denominator, a monster frame.
    let mut odd = full();
    let image = odd.image.as_mut().unwrap();
    image.bit_depth = 48;
    (image.width, image.height) = (u32::MAX, u32::MAX);
    odd.outcome.total_samples = Some(0);
    odd.timing_ms.total = 1e12;
    odd.timestamp_ms = u64::MAX;
    odd.cpu_count = Some(u32::MAX);
    events.push(odd);

    for e in &events {
        let up = project(e);
        assert!(
            contract.accepts_request(&request(&[&up])),
            "{}",
            serde_json::to_string(&up).unwrap()
        );
    }
}

#[test]
fn hostile_local_values_never_reach_the_upload() {
    let mut e = full();
    // No digits in the ID, so a leaked number cannot hide in it.
    e.event_id = EventId([0xaa; 16]);
    e.target = "x86_64-/Users/alice/secret\nscan.tif".into();
    e.timestamp_ms = 1_700_000_012_345;
    e.cpu_count = Some(13);
    let image = e.image.as_mut().unwrap();
    (image.width, image.height) = (7777, 3333);
    image.megapixels = 25.920_741;
    image.input_bytes = Some(123_456_789);
    image.output_bytes = Some(987_654_321);
    let conversion = e.conversion.as_mut().unwrap();
    conversion.params_hash = "HOSTILEHASH".into();
    conversion.film_base_source = FilmBaseProvenance::Region([123_457, 234_568, 345_679, 1]);
    e.outcome.warnings = 9;
    e.outcome.total_samples = Some(77_760_003);
    e.outcome.clipped = Some(4242);
    e.outcome.non_finite = Some(17);
    e.timing_ms.total = 1234.567;
    let mut explicit = e.clone();
    explicit.conversion.as_mut().unwrap().film_base_source =
        FilmBaseProvenance::Explicit([0.123_457, 0.234_568, 0.345_679]);

    for event in [e, explicit] {
        let wire = serde_json::to_string(&project(&event)).unwrap();
        for leak in [
            "alice",
            "secret",
            "scan",
            "/",
            "\\n",
            "HOSTILE",
            "7777",
            "3333",
            "25.9",
            "123456789",
            "987654321",
            "1700000012345",
            "123457",
            "234568",
            "345679",
            "0.12",
            "4242",
            "77760003",
            "1234.5",
            "u16",
            "display-p3",
            "\"tiff\"",
        ] {
            assert!(!wire.contains(leak), "{leak:?} leaked: {wire}");
        }
    }
}

#[test]
fn an_off_contract_event_has_no_upload_form() {
    let mut e = full();
    e.schema_version = 10;
    assert_eq!(to_upload_event(&e), Err(NotUploadable::SchemaVersion));

    let mut e = full();
    e.nc_version = "0.1.0+dirty".into();
    assert_eq!(to_upload_event(&e), Err(NotUploadable::NcVersion));

    let mut e = full();
    e.conversion.as_mut().unwrap().film_base_source = FilmBaseProvenance::EffectiveArea;
    assert_eq!(to_upload_event(&e), Err(NotUploadable::FilmBase));

    // Adobe RGB is SDR-only: no row, so no encoding to name.
    let mut e = full();
    e.conversion.as_mut().unwrap().destination = OutputSection::Display(DisplayAxes {
        range: Some(Range::Hdr),
        gamut: Some(Gamut::AdobeRgb),
        ..DisplayAxes::default()
    });
    assert_eq!(to_upload_event(&e), Err(NotUploadable::Destination));
    let mut e = full();
    e.conversion.as_mut().unwrap().destination = OutputSection::Display(DisplayAxes {
        range: Some(Range::Hdr),
        transfer: Some(Transfer::Pq),
        gamut: Some(Gamut::AdobeRgb),
        container: Some(Container::Tiff),
    });
    assert_eq!(
        to_upload_event(&e),
        Err(NotUploadable::Destination),
        "every axis stated"
    );

    // Pairings the Worker rejects.
    let mut e = full();
    e.stage = EventStage::Setup;
    assert_eq!(
        to_upload_event(&e),
        Err(NotUploadable::Outcome),
        "success in setup"
    );
    let mut e = full();
    e.outcome.exit_code = 1;
    assert_eq!(
        to_upload_event(&e),
        Err(NotUploadable::Outcome),
        "success exit 1"
    );
    let mut e = full();
    e.outcome.error_kind = ErrorKind::Other;
    assert_eq!(
        to_upload_event(&e),
        Err(NotUploadable::Outcome),
        "success kind"
    );
    let mut e = full();
    e.image = None;
    assert_eq!(
        to_upload_event(&e),
        Err(NotUploadable::Outcome),
        "success image"
    );
    let mut e = full();
    (
        e.outcome.total_samples,
        e.outcome.clipped,
        e.outcome.non_finite,
    ) = (None, None, None);
    assert_eq!(
        to_upload_event(&e),
        Err(NotUploadable::Outcome),
        "success without clip fields"
    );
    let mut e = full();
    e.outcome.non_finite = None;
    assert_eq!(
        to_upload_event(&e),
        Err(NotUploadable::Outcome),
        "clip pair"
    );
    let mut e = minimal();
    e.outcome.exit_code = 3;
    assert_eq!(
        to_upload_event(&e),
        Err(NotUploadable::Outcome),
        "usage exit 3"
    );
    let mut e = minimal();
    e.outcome.error_kind = ErrorKind::None;
    assert_eq!(
        to_upload_event(&e),
        Err(NotUploadable::Outcome),
        "failure kind none"
    );
}

#[test]
fn this_build_s_version_is_uploadable() {
    assert!(is_release_version(env!("CARGO_PKG_VERSION")));
}

#[test]
fn release_versions_follow_the_schema_pattern() {
    for ok in [
        "0.1.0",
        "10.20.30",
        "1.0.0-rc.1",
        "1.0.0-0.3.7",
        "1.0.0-x-y.z",
        "1.0.0-a1",
    ] {
        assert!(is_release_version(ok), "{ok}");
    }
    let long = format!("0.1.0-{}", "a".repeat(59));
    for bad in [
        "",
        "0.1",
        "01.0.0",
        "v0.1.0",
        "0.1.0+abc",
        "0.1.0-",
        "0.1.0-01",
        "0.1.0-a..b",
        "0.1.0-é",
        "0.1.0\n",
        long.as_str(),
    ] {
        assert!(!is_release_version(bad), "{bad:?}");
    }
}

#[test]
fn platforms_come_from_the_target_triple() {
    for (target, want) in [
        ("aarch64-apple-darwin", ("macos", "aarch64")),
        ("x86_64-apple-darwin", ("macos", "x86_64")),
        ("x86_64-unknown-linux-gnu", ("linux", "x86_64")),
        ("aarch64-unknown-linux-musl", ("linux", "aarch64")),
        ("arm64ec-pc-windows-msvc", ("windows", "aarch64")),
        ("arm64_32-apple-watchos", ("other", "other")),
        ("arm64e-apple-darwin", ("macos", "aarch64")),
        ("arm64e-apple-ios", ("other", "aarch64")),
        ("x86_64h-apple-darwin", ("macos", "x86_64")),
        ("x86_64-pc-windows-msvc", ("windows", "x86_64")),
        ("i686-pc-windows-gnu", ("windows", "x86")),
        ("armv7-unknown-linux-gnueabihf", ("linux", "arm")),
        ("riscv64gc-unknown-freebsd", ("other", "other")),
        ("", ("unknown", "unknown")),
    ] {
        assert_eq!(platform(target), want, "{target}");
    }
}

#[test]
fn buckets_hold_their_boundaries() {
    for (n, want) in [
        (None, "unknown"),
        (Some(0), "unknown"),
        (Some(1), "1"),
        (Some(3), "2"),
        (Some(4), "4"),
        (Some(15), "8"),
        (Some(16), "16"),
        (Some(63), "32"),
        (Some(64), "64_plus"),
    ] {
        assert_eq!(cpu_bucket(n), want, "{n:?}");
    }
    for (n, want) in [(0, "0"), (1, "1"), (2, "2_3"), (3, "2_3"), (4, "4_plus")] {
        assert_eq!(warning_bucket(n), want, "{n}");
    }
    for ((clipped, total), want) in [
        ((0, 0), "0"),
        ((0, 1000), "0"),
        ((1, 0), "unknown"),
        ((1, 1001), "lt_0_1_pct"),
        ((1, 1000), "lt_1_pct"),
        ((1, 100), "lt_10_pct"),
        ((1, 10), "gte_10_pct"),
        ((u64::MAX, u64::MAX), "gte_10_pct"),
    ] {
        assert_eq!(
            clipped_fraction_bucket(clipped, total),
            want,
            "{clipped}/{total}"
        );
    }
    const MIB: u64 = 1 << 20;
    for (bytes, want) in [
        (None, "unknown"),
        (Some(8 * MIB - 1), "lt_8_mib"),
        (Some(8 * MIB), "8_31_mib"),
        (Some(32 * MIB), "32_127_mib"),
        (Some(128 * MIB), "128_511_mib"),
        (Some(512 * MIB), "512_plus_mib"),
    ] {
        assert_eq!(input_size_bucket(bytes), want, "{bytes:?}");
    }
    assert_eq!(megapixels_tenths(5000, 3740), 187);
    assert_eq!(megapixels_tenths(1000, 50), 1, "0.05 MP rounds up");
    assert_eq!(megapixels_tenths(1000, 49), 0);
    assert_eq!(megapixels_tenths(u32::MAX, u32::MAX), MAX_MEGAPIXELS_TENTHS);
    assert_eq!((ms(0.49), ms(0.5), ms(-3.0)), (0, 1, 0));
    assert_eq!((ms(1e12), ms(f64::NAN)), (MAX_MS, 0));
    assert_eq!(event_day(86_399_999), 0);
    assert_eq!(event_day(86_400_000), 1);
    assert_eq!(event_day(u64::MAX), u16::MAX);
}

#[test]
fn the_local_panic_fixture_is_the_corpus_panic() {
    // The local panic event's shape (`telemetry/panic-hook` writes it); its upload form
    // is the corpus's `panic` event.
    let text = read("local/panic-ready.json");
    assert!(text.len() <= 16 * 1024);
    let body = text.strip_suffix('\n').unwrap();
    assert!(
        body.starts_with('{') && body.ends_with('}') && !body.contains(char::is_whitespace),
        "one compact object and a newline"
    );
    let local: Value = serde_json::from_str(&text).unwrap();
    let keys: Vec<&str> = local
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let mut want = [
        "schema_version",
        "event_id",
        "event",
        "command",
        "timestamp_ms",
        "nc_version",
        "target",
        "cpu_count",
        "stage",
        "frames",
    ];
    want.sort_unstable();
    assert_eq!(keys, want, "the README's key set");
    assert_eq!(local["schema_version"], SCHEMA_VERSION);
    assert_eq!(local["event"], "panic");

    let upload = corpus_event("panic");
    assert_eq!(upload["event_id"], local["event_id"]);
    assert_eq!(upload["frames"], local["frames"]);
    assert_eq!(upload["nc_version"], local["nc_version"]);
    assert_eq!(upload["stage"], local["stage"]);
    let ts = local["timestamp_ms"].as_u64().unwrap();
    assert_eq!(upload["event_day"], event_day(ts));
    let cpu = local["cpu_count"].as_u64().map(|n| n as u32);
    assert_eq!(upload["platform"]["cpu_bucket"], cpu_bucket(cpu));
    let (os, arch) = platform(local["target"].as_str().unwrap());
    assert_eq!(
        (&upload["platform"]["os"], &upload["platform"]["arch"]),
        (&Value::from(os), &Value::from(arch))
    );
}

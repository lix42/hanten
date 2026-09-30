//! Shared core types — the neutral contract between pipeline stages.
//!
//! This module is pure data: no I/O, and no crate-specific image types
//! (conversions to/from `image`/`tiff` live in the `io` stages). Every stage
//! takes `(input, params) -> output`; these are the `input`/`output` and the
//! `params`. Param structs mirror the CLI/recipe keys in design-spec §9 so a
//! recipe JSON round-trips to exactly the knobs the pipeline reads.

use serde::{Deserialize, Serialize};

/// Linear scanner image in `f32`, interleaved RGB plus optional IR plane.
///
/// Values are in a linear working space, range ~`[0, 1]`. `rgb` is interleaved
/// (`r,g,b, r,g,b, …`) with `len == width * height * 3`. The IR plane, when
/// present (HDRi input), is `len == width * height`. It is exported verbatim
/// (`--export-ir`) and, since `ir-holder-detection`, consumed by the film-base
/// holder mask — but **only when [`ir_verified`](Self::ir_verified) is true**, and
/// only when the plane measures able to separate holder from film on that frame
/// (`pipeline::film_base::ir_separability`; design-spec §6.1).
#[derive(Clone, Debug)]
pub struct LinearImage {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<f32>,
    pub ir: Option<Vec<f32>>,
    /// Whether the IR plane's provenance is **marker-verified** — the decoder
    /// found the SilverFast IR IFD's `NewSubfileType=4` marker, not merely a
    /// same-dimension 16-bit grayscale page identified by shape alone. A
    /// shape-only IR plane is still carried and exportable, but it must **not** be
    /// trusted by a conversion consumer (a stray grayscale page could otherwise be
    /// thresholded as IR and corrupt the film base), so the holder mask is skipped
    /// for it. Meaningful only when `ir.is_some()`; [`new`](Self::new) defaults it
    /// `false` and `io::decode` sets it from the marker.
    pub ir_verified: bool,
}

impl LinearImage {
    /// Validated constructor — the single entry point `io::decode` should use to
    /// build an image, so the buffer-length invariants (`rgb.len() == w*h*3`,
    /// `ir.len() == w*h`) are checked once at the boundary instead of surfacing
    /// as a panic deep in the pipeline. Fields stay `pub` for stage ergonomics.
    pub fn new(width: u32, height: u32, rgb: Vec<f32>, ir: Option<Vec<f32>>) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(NcError::Other(format!(
                "image dimensions must be non-zero (got {width}x{height})"
            )));
        }
        // Checked arithmetic: a hostile/corrupt header advertising huge
        // dimensions must surface as an error, not a debug panic / release wrap.
        let overflow = || {
            NcError::Other(format!(
                "image dimensions {width}x{height} overflow address space"
            ))
        };
        let pixels = (width as usize)
            .checked_mul(height as usize)
            .ok_or_else(overflow)?;
        let rgb_len = pixels.checked_mul(3).ok_or_else(overflow)?;
        if rgb.len() != rgb_len {
            return Err(NcError::Other(format!(
                "rgb buffer length {} != width*height*3 ({rgb_len})",
                rgb.len()
            )));
        }
        if let Some(ir_plane) = &ir {
            let ir_len = ir_plane.len();
            if ir_len != pixels {
                return Err(NcError::Other(format!(
                    "ir buffer length {ir_len} != width*height ({pixels})"
                )));
            }
        }
        Ok(Self {
            width,
            height,
            rgb,
            ir,
            // Provenance is not known at this boundary; `io::decode` sets it from
            // the IR IFD's `NewSubfileType=4` marker. A shape-only IR plane stays
            // `false` (carried/exportable but not trusted by consumers).
            ir_verified: false,
        })
    }
}

/// Per-channel unexposed-film base transmission — the `Dmin` anchor.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FilmBase {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

// The recipe/CLI carries the film base as an `[r, g, b]` array (mirroring the
// `--film-base R,G,B` flag), while the pipeline prefers the named `FilmBase`.
// Keep that one conversion here so the two representations can't drift.
impl From<[f32; 3]> for FilmBase {
    fn from([r, g, b]: [f32; 3]) -> Self {
        Self { r, g, b }
    }
}

impl From<FilmBase> for [f32; 3] {
    fn from(b: FilmBase) -> Self {
        [b.r, b.g, b.b]
    }
}

/// Output bit depth for the TIFF paths. Always *resolved* from the destination, and
/// never stated: the `--out-depth` / `output.depth` knob retired with the `legacy`
/// and `custom` presets, the only two that read it.
///
/// [`Display`](std::fmt::Display) gives the spelling (`u16` / `f32`) diagnostics and
/// the telemetry record use; `{:?}` would print `U16`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutDepth {
    /// 16-bit integer. Clamped and rounded at encode.
    #[default]
    U16,
    /// 32-bit float, written verbatim: values above 1.0 survive, and so do
    /// non-finite samples (counted, never laundered).
    F32,
}

impl std::fmt::Display for OutDepth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            OutDepth::U16 => "u16",
            OutDepth::F32 => "f32",
        })
    }
}

/// BigTIFF promotion policy for the encoder. Every written image uses `Auto` since
/// `--bigtiff` retired with the `legacy` and `custom` presets; `On` / `Off` remain
/// as the resolved decision handed to the writer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BigTiff {
    /// Promote to BigTIFF only when the output would exceed the classic limit.
    #[default]
    Auto,
    On,
    Off,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Top-level error type for the whole tool. Each variant maps to a stable exit
/// code (design-spec §11) via [`NcError::exit_code`].
#[derive(Clone, Debug)]
pub enum NcError {
    /// Invalid CLI usage or parameters. Exit 2.
    Usage(String),
    /// Input read/decode error (unreadable or unsupported file). Exit 3.
    Decode(String),
    /// Unsupported variant (e.g. a channel layout we can't handle yet). Exit 4.
    Unsupported(String),
    /// Output write error. Exit 5.
    Write(String),
    /// A resource limit would be exceeded — today, the memory preflight's
    /// estimated peak allocation against the run's budget
    /// (`pipeline::memory`). Exit 6.
    ///
    /// Distinct from [`Unsupported`](Self::Unsupported) on purpose: the input is
    /// perfectly supported, it is *this run on this budget* that cannot proceed,
    /// and an agent that catches exit 6 knows to retry with `--max-memory` (or on
    /// a bigger machine) rather than give up on the file.
    Resource(String),
    /// Generic / unexpected error. Exit 1.
    Other(String),
}

impl NcError {
    /// Stable process exit code for this error (design-spec §11). Kept here so
    /// `cli` and `pipeline` map errors to codes in exactly one place.
    pub fn exit_code(&self) -> i32 {
        match self {
            NcError::Other(_) => 1,
            NcError::Usage(_) => 2,
            NcError::Decode(_) => 3,
            NcError::Unsupported(_) => 4,
            NcError::Write(_) => 5,
            NcError::Resource(_) => 6,
        }
    }
}

impl NcError {
    /// The message without the `kind:` prefix [`Display`](std::fmt::Display) adds.
    ///
    /// For composing one error's text into another message: `Display` prefixes the
    /// kind, so `format!("{e} …")` inside a warning carries a stray `usage:`, and
    /// inside a new `NcError` prints it twice.
    ///
    /// **Not a second rendering of the error** — it is the one `Display` is built
    /// from (below), so the two cannot drift. Print an error with `Display`; use
    /// this only to compose its text into a longer message.
    pub fn message(&self) -> &str {
        match self {
            NcError::Usage(m)
            | NcError::Decode(m)
            | NcError::Unsupported(m)
            | NcError::Write(m)
            | NcError::Resource(m)
            | NcError::Other(m) => m,
        }
    }

    /// The same error with `prefix: ` before its message — naming the file it came
    /// from, say — keeping its kind, and so its exit code.
    pub fn prefixed(self, prefix: impl std::fmt::Display) -> Self {
        let with = |m: String| format!("{prefix}: {m}");
        match self {
            NcError::Usage(m) => NcError::Usage(with(m)),
            NcError::Decode(m) => NcError::Decode(with(m)),
            NcError::Unsupported(m) => NcError::Unsupported(with(m)),
            NcError::Write(m) => NcError::Write(with(m)),
            NcError::Resource(m) => NcError::Resource(with(m)),
            NcError::Other(m) => NcError::Other(with(m)),
        }
    }

    /// The `kind:` label [`Display`](std::fmt::Display) prefixes, which is also the
    /// exit-code family (design-spec §11).
    fn kind(&self) -> &'static str {
        match self {
            NcError::Usage(_) => "usage",
            NcError::Decode(_) => "decode",
            NcError::Unsupported(_) => "unsupported",
            NcError::Write(_) => "write",
            NcError::Resource(_) => "resource",
            NcError::Other(_) => "error",
        }
    }
}

impl std::fmt::Display for NcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind(), self.message())
    }
}

impl std::error::Error for NcError {}

#[cfg(test)]
mod error_tests {
    use super::NcError;

    /// `Display` is `kind: message`, and `message()` is exactly the second half —
    /// so composing one error's text into another cannot pick up a stray `usage:`
    /// and the two spellings cannot drift apart.
    #[test]
    fn a_prefix_keeps_the_kind_and_so_the_exit_code() {
        for e in [
            NcError::Usage("u".into()),
            NcError::Decode("d".into()),
            NcError::Unsupported("x".into()),
            NcError::Write("w".into()),
            NcError::Resource("r".into()),
            NcError::Other("o".into()),
        ] {
            let (code, message) = (e.exit_code(), format!("f.tif: {}", e.message()));
            let p = e.prefixed("f.tif");
            assert_eq!((p.exit_code(), p.message()), (code, message.as_str()));
        }
    }

    #[test]
    fn message_is_display_without_the_kind_prefix() {
        for e in [
            NcError::Usage("u".into()),
            NcError::Decode("d".into()),
            NcError::Unsupported("n".into()),
            NcError::Write("w".into()),
            NcError::Resource("r".into()),
            NcError::Other("o".into()),
        ] {
            let shown = e.to_string();
            let (kind, msg) = shown.split_once(": ").expect("`kind: message`");
            assert_eq!(msg, e.message(), "{shown}");
            assert!(!kind.is_empty() && !kind.contains(' '), "{shown}");
            // Falsifiability: the prefix is real, so this is not a tautology.
            assert_ne!(shown, e.message());
        }
    }
}

/// Convenience alias for fallible operations across the tool.
pub type Result<T> = std::result::Result<T, NcError>;

// ---------------------------------------------------------------------------
// Stage parameter structs (one per stage; CLI/recipe keys, design-spec §9)
// ---------------------------------------------------------------------------
//
// Downstream tasks fill in the behavior; these establish the stable shape and
// serde key names. Defaults are deliberately neutral (identity-ish) placeholders
// — the algorithm tasks refine them.

/// Transfer-encoding assertion for the input (design-spec §9, `input.transfer`).
///
/// One of the two **independent** input axes (the other is [`MeaningAssertion`]).
/// It asserts only how the samples are *encoded*, never what they *measure*:
/// `Linear` says the transfer is linear (no inverse-transfer decoding needed),
/// which does not by itself prove scanner-device provenance. `Auto` (default)
/// lets the input semantic resolver (`pipeline::input_semantics`) decide from
/// container evidence, failing loudly in `convert` when it stays ambiguous.
/// Serializes kebab-case (`"auto"` / `"linear"`; kebab-case matches its mirror
/// [`MeaningAssertion`] and `TransferDescription`, so a future multi-word variant
/// stays consistent); parsed the same on the CLI via `ValueEnum`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum TransferAssertion {
    /// Resolve the transfer from container evidence (structural raw-mode, a
    /// descriptive gamma tag).
    #[default]
    Auto,
    /// Assert a supported linear transfer. Overrides a contradicting descriptive
    /// gamma tag (recorded as displaced evidence); it cannot override container
    /// structure that proves a non-linear encoding.
    Linear,
}

/// Measurement-meaning assertion for the input (design-spec §9, `input.meaning`).
///
/// The second independent input axis: what the pixel values *are*. Only
/// [`ScannerDevice`](Self::ScannerDevice) measurements paired with a supported
/// linear transfer enter Dmin/density without a source→working color transform.
/// [`Colorimetric`](Self::Colorimetric) is recognized but unsupported (no inverse
/// transfer/reconstruction path exists yet). `Auto` (default) resolves from
/// container evidence — an embedded ICC alone does not establish it. Serializes
/// kebab-case (`"auto"` / `"scanner-device"` / `"colorimetric"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum MeaningAssertion {
    /// Resolve the meaning from container evidence.
    #[default]
    Auto,
    /// Assert scanner-device measurements (the supported meaning).
    ScannerDevice,
    /// Assert colorimetric RGB. Recognized but unsupported; `convert` rejects it
    /// even when asserted (an override cannot make it supported).
    Colorimetric,
}

/// Descriptive transfer/gamma evidence parsed from container metadata, with a
/// **third state** the resolver needs: a gamma tag that is *present but
/// uninterpretable* is ambiguous, **not** absent. Collapsing malformed → absent
/// would let a raw scan whose gamma is actually non-linear but written unparseably
/// (e.g. a German-locale `"2,2"` — LaserSoft is German software) silently resolve
/// to linear and skip the contradiction path. Lives here (not in
/// `pipeline::input_semantics`) so both `io::decode` (which produces it) and the
/// resolver (which consumes it) can share it without an io→pipeline dependency.
#[derive(Clone, Debug, PartialEq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum GammaFact {
    /// No gamma tag present in the metadata.
    #[default]
    Absent,
    /// A gamma tag parsed to this numeric value.
    Value(f64),
    /// A gamma tag was present but could not be interpreted as a number (carries
    /// the offending raw string for the diagnostic). Ambiguous, never linear.
    Malformed(String),
}

/// Declared film chemistry (design-spec §9 `input.film_type`, §6.1) — a
/// **provenance declaration that gates nothing today**. **Chromogenic** dyes (C-41
/// colour *and* C-41-process B&W) are transparent to infrared; **silver** halide
/// B&W blocks IR in proportion to accumulated density.
///
/// It used to gate IR-assisted film-holder detection. It no longer does
/// (`ir-usability-detection`): chemistry is the wrong predictor, because
/// separability is a property of the *frame's* density, not the stock's — an
/// unexposed silver frame is IR-transparent against an opaque holder (measured
/// ~20:1) while its own fully-exposed leader is opaque throughout. The two
/// disagree on exactly the frames the calibration workflow uses, so
/// `film_base::ir_separability` measures the plane instead and this declaration
/// takes no part in the decision.
///
/// It is kept as a **shared input-medium declaration** the roadmap still needs:
/// the black & white `bw-support` task (roadmap item 3) for its B&W handling, and
/// the separate IR dust-removal task (roadmap item 1), which gates its defect map
/// on chemistry (silver blocks IR like dust). Whether *that* gate should also be a
/// measurement is an open question for those tasks — dust separability is not the
/// same question as holder separability. Serializes kebab-case (`"unknown"` /
/// `"silver"` / `"chromogenic"`); parsed the same on the CLI via `ValueEnum`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum FilmType {
    /// Film chemistry not declared (default).
    #[default]
    Unknown,
    /// Silver-halide B&W — silver blocks IR in proportion to accumulated density,
    /// so an *unexposed* frame is still IR-transparent while a leader is opaque.
    Silver,
    /// Chromogenic dye film (C-41 colour or C-41-process B&W) — IR-transparent at
    /// any exposure (measured 0.58-0.73 interior IR transmission over 25 frames,
    /// 9 rolls, leaders included).
    Chromogenic,
}

/// Measurement knobs — where a statistic read off the frame may be read from
/// (design-spec §9, `measure`).
///
/// Its own section rather than a key under `film_base`, because the effective area
/// governs **every** measurement path (`Dmin`, a roll's white balance, content exposure and
/// contrast, tiling uniformity), not just the film base. Filing it under one
/// consumer would misplace it permanently: every recipe struct carries
/// `deny_unknown_fields`, so a key's section is part of its identity.
///
/// Operational-only flags do not belong here — a `measure` key is a conversion
/// knob and must be both a CLI flag and a recipe key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MeasureParams {
    /// The static border inset, as a fraction of the **original** frame's shorter
    /// dimension — the second of the effective area's two cuts.
    ///
    /// The holder is cut first, from IR, so this only has to clear a rebate; the
    /// default is [`DEFAULT_MEASURE_INSET`] and the bound is [`MAX_MEASURE_INSET`],
    /// both checked by [`check_measure_inset`]. Where the holder
    /// could **not** be measured (no IR plane, or film too IR-opaque to separate —
    /// routine for silver stock) this is the *only* cut, and the default will
    /// under-clear a real holder. Raising it is then the user's call, deliberately:
    /// nc declines to guess a depth it could not measure, and says in the report
    /// which case a run was in.
    pub inset: f32,
}

impl Default for MeasureParams {
    fn default() -> Self {
        Self {
            inset: DEFAULT_MEASURE_INSET,
        }
    }
}

/// Default static border inset, as a fraction of the **original** frame's shorter
/// dimension — the second of the effective area's two cuts.
///
/// It is sized for a rebate, because the holder is cut first from IR. Where the
/// holder could *not* be measured this same 5% is the only cut, and it may then
/// under-clear the holder and its rebate together: the only directly measured
/// holder depth nc has is **2.5-4% of the shorter edge** (the IR march across 31
/// real frames, `film-base/holder-depth-mask`). The 10-15% figure quoted elsewhere
/// is *not* a depth — it is the holder's share of a rendered frame's top codes
/// (`analysis/conversion-metrics`) — so it does not bound this. Under-clearing is
/// deliberate: nc declines to guess a depth it could not measure, and the user
/// raises the fraction instead (user decision 2026-09-16).
pub const DEFAULT_MEASURE_INSET: f32 = 0.05;

/// Largest accepted [`MeasureParams::inset`]. Insetting half the shorter dimension
/// from *each* side would leave nothing, so the bound sits strictly below 0.5 — a
/// fraction this large is a mistake, not a measurement choice. The bound itself is
/// accepted ([`check_measure_inset`] refuses only `frac > MAX_MEASURE_INSET`).
pub const MAX_MEASURE_INSET: f32 = 0.4;

/// Check a measurement inset fraction, or refuse it.
///
/// The **single** definition of the rule, called from both places that need it:
/// `cli::validate_shared` (so `convert`, `roll` and every per-frame override refuse a bad
/// value before a frame is decoded) and `pipeline::film_base::effective_area` (so a
/// programmatic caller cannot bypass it). Two gates bounding one knob differently
/// is the defect this shape exists to prevent.
pub fn check_measure_inset(frac: f32) -> Result<()> {
    if !frac.is_finite() || frac < 0.0 {
        return Err(NcError::Usage(format!(
            "--measure-inset / measure.inset must be finite and non-negative (got \
             {frac}). It is the fraction of the shorter edge inset from each side \
             after the holder cut; `0` asks for no inset (floored at one \
             holder-probe step on a frame where the holder was measured — see \
             `pipeline::film_base::effective_area`)."
        )));
    }
    if frac > MAX_MEASURE_INSET {
        return Err(NcError::Usage(format!(
            "--measure-inset / measure.inset is {frac}, beyond the supported maximum \
             of {MAX_MEASURE_INSET}. It is inset from *each* side, so {frac} would \
             remove {:.0}% of the shorter dimension; the default is \
             {DEFAULT_MEASURE_INSET}. Measured holder depth is 2.5-4% of the shorter \
             edge (IR march, 31 real frames), so lower the fraction. There is no \
             explicit measurement region to point at instead — `--base-region` sets \
             the film-base source, not the measured area. A conversion measures \
             nothing over the area; `hanten measure-roll` reads it to pool a roll's \
             white balance.",
            (frac * 2.0 * 100.0).min(100.0)
        )));
    }
    Ok(())
}

/// Input / decode knobs (design-spec §9, stage 1).
///
/// Transfer and meaning are **two independent axes** (not a single combined
/// `input.color` choice, which conflated them): the resolver
/// (`pipeline::input_semantics`) resolves each from separate evidence. There is
/// deliberately no `input.color` field — the old combined key is rejected with a
/// migration error at recipe load (see `cli::load_recipe_for`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct InputParams {
    /// Transfer-encoding assertion (default `auto`).
    pub transfer: TransferAssertion,
    /// Measurement-meaning assertion (default `auto`).
    pub meaning: MeaningAssertion,
    /// Declared film chemistry (default `unknown`). Provenance only — it no longer
    /// gates IR-assisted film-holder detection, which measures the IR plane instead
    /// (`ir-usability-detection`). Reserved for the roadmap tasks that still need a
    /// chemistry axis (`bw-support`; IR dust removal). See [`FilmType`].
    pub film_type: FilmType,
    /// Write the decoded IR plane to this path (HDRi only); `None` skips export.
    /// An input/decode-domain artifact (design-spec §9, Input/decode) — carried
    /// here so `pipeline-orchestration` can drive the IR exporter.
    pub export_ir: Option<String>,
}

/// Where a conversion's film base comes from (design-spec §9, stage 2): stated
/// outright, or read from a stated region. Serializes as
/// `{ "region": [x, y, w, h] }` / `{ "explicit": [r, g, b] }`.
///
/// Every source is a *statement*. Measuring a reference frame's effective area
/// (`film_base::measure_area`) is not a source: it runs in the measurement commands,
/// and its result reaches a conversion as `Explicit`. The retired `"auto"` rebate
/// search is refused with a migration message (`recipe::check_body`).
///
/// **Deliberately has no `Default`.** `Dmin` sets black point and colour balance
/// together, so `convert` requires the choice to be stated (see
/// [`crate::recipe::Calibration::film_base`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FilmBaseSource {
    /// Read the base from this region `[x, y, w, h]`.
    Region([u32; 4]),
    /// Explicit per-channel base transmission `[r, g, b]`.
    Explicit([f32; 3]),
}

/// Where a report's film base came from: a stated [`FilmBaseSource`], or the
/// effective-area measurement (`film_base::measure_area`), which a recipe cannot
/// state. Serializes as `"effective_area"` / `{"region":[…]}` / `{"explicit":[…]}`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FilmBaseProvenance {
    /// The median over the effective area (`report.effective_area`).
    EffectiveArea,
    /// A stated region.
    Region([u32; 4]),
    /// An explicit value.
    Explicit([f32; 3]),
}

impl From<&FilmBaseSource> for FilmBaseProvenance {
    fn from(source: &FilmBaseSource) -> Self {
        match *source {
            FilmBaseSource::Region(r) => Self::Region(r),
            FilmBaseSource::Explicit(b) => Self::Explicit(b),
        }
    }
}

/// Default specular headroom for fit range (`fit_range.headroom_stops`), in stops.
///
/// `6` stops is `W = 64`, the value measured to beat the shipped sigmoid on both
/// highlight metrics on all seven fixture frames at matched brightness. `W = 256`
/// scored better on clipped fraction alone but leaves a pre-clamp peak of 1.016 —
/// nothing above diffuse white — which is the condition that makes a gain map inert, so
/// it is deliberately not the default.
pub const DEFAULT_HEADROOM_STOPS: f32 = 6.0;

/// The largest accepted specular headroom, in stops.
///
/// Beyond this the operator is indistinguishable from plain `v/(1 + v)` and the number
/// only looks like a setting. The measured useful range is 4–8 stops.
///
/// Lives here beside the default and [`headroom_white_point`] so the recipe's validation
/// and fit range cannot bound the knob differently — the failure that pattern produces
/// is a recipe validation accepts and the render then refuses, at exit 1 after a whole
/// roll has decoded.
pub const MAX_HEADROOM_STOPS: f32 = 24.0;

/// The white point a specular headroom asks for: `2^stops`.
///
/// The **single** definition: `pipeline::fit_range` and its tests read it, so the
/// stops→white-point meaning has one home.
pub fn headroom_white_point(stops: f32) -> f32 {
    stops.exp2()
}

/// Which part of the headroom rule a value breaks — see [`headroom_fault`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HeadroomFault {
    /// Negative or non-finite.
    Negative(f32),
    /// Above [`MAX_HEADROOM_STOPS`].
    TooLarge(f32),
}

/// The specular-headroom rule, as data: the **single** definition the recipe's
/// validation and fit range both check. Both refuse through [`headroom_fault_message`],
/// naming the knob as its provenance spells it.
///
/// A negative headroom is not loud on its own: `2^-40` is a white point of ~9e-13, which
/// maps essentially every sample past the ceiling and turns the render into a solid
/// white field at exit 0 with the clip merely *counted*.
pub fn headroom_fault(stops: f32) -> Option<HeadroomFault> {
    if !stops.is_finite() || stops < 0.0 {
        Some(HeadroomFault::Negative(stops))
    } else if stops > MAX_HEADROOM_STOPS {
        Some(HeadroomFault::TooLarge(stops))
    } else {
        None
    }
}

/// The refusal for a [`HeadroomFault`], with `name` spelling the knob — one wording, so
/// the same bad value is explained the same way wherever it is caught.
pub fn headroom_fault_message(fault: HeadroomFault, name: &str) -> String {
    match fault {
        HeadroomFault::Negative(stops) => format!(
            "{name} must be finite and non-negative (got {stops}). It is the specular \
             headroom above reference white fit range keeps distinguishable, in \
             stops; `0` is the identity."
        ),
        HeadroomFault::TooLarge(stops) => format!(
            "{name} is {stops} stops, beyond the supported maximum of {MAX_HEADROOM_STOPS}. \
             Above ~8 stops the operator converges on plain Reinhard and the extra headroom \
             buys nothing; the measured useful range is 4–8 (the default \
             {DEFAULT_HEADROOM_STOPS} is a white point of {}).",
            headroom_white_point(DEFAULT_HEADROOM_STOPS)
        ),
    }
}

/// Output decades between mid-grey and display white: `−log10(0.18)`.
///
/// A mid-grey card reflects ~18% of the light falling on it, so on a correctly exposed
/// display-referred image it belongs at 0.18 of white — about 2.5 stops down. This is a
/// property of what "18% reflectance" means, not a tunable.
pub const MID_GREY_OUTPUT_DECADES: f32 = 0.744_727_5;

/// Density between a mid-grey card and a diffuse white on a correctly exposed colour
/// negative, from the manufacturers' own aim tables.
///
/// Every Kodak colour-negative datasheet carries the same *Judging Negative Exposures*
/// table (Status M, red channel, for "a normally exposed and processed color negative").
/// The absolute values differ per stock, but their **difference is essentially constant**:
/// Ektar 100, Portra 160 and Portra 400 all give 0.36 (Gold 200, a consumer stock, 0.40).
/// Sources: Kodak E-4046, E-4051, E-4050, E-7022.
///
/// The decode is stock-agnostic (design-spec §7), so this single professional-film figure
/// is the reference.
#[cfg(test)]
pub const REFERENCE_MID_TO_WHITE_DELTA: f32 = 0.36;

/// The migration message for the retired `simple` reconstruction, shared by the
/// recipe deserializer and the `--reconstruction` flag.
pub const REMOVED_SIMPLE_RECONSTRUCTION: &str = "`simple` reconstruction (the direct \
     `1 − scan/Dmin` inversion) was removed: it is an affine inversion of the scan rather \
     than a decode of the film, and density is now the only reconstruction. Remove \
     `reconstruction.type` / `--reconstruction`; the default density reconstruction \
     applies";

/// What the encode stage observed while writing — fed into the JSON report by
/// the orchestrator. Records two kinds of trouble the output samples can carry,
/// since no colour stage clamps and the density-domain
/// algorithm can produce non-finite values from log/division math:
///
/// - **clipping** (`clipped_low`/`clipped_high`): finite samples outside `[0, 1]`
///   clamped into range. Only the u16 path clamps, so these are u16-only.
/// - **non-finite** (`non_finite`): `NaN`/`±inf` samples — a pipeline numerical
///   fault. Counted for *both* depths (u16 forces them to 0; f32 writes them
///   verbatim), so the fault surfaces regardless of output depth.
///
/// This rides back on the value path rather than down `Result` because it is a
/// quality warning, not a write failure (`--strict` can promote it to an error).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[must_use]
pub struct EncodeReport {
    /// Samples examined (`width * height * channels`). The denominator that makes
    /// the clip / non-finite counts interpretable as a fraction.
    pub total_samples: u64,
    /// Finite samples below 0.0 clamped up to 0 (u16 output only).
    pub clipped_low: u64,
    /// Finite samples above 1.0 clamped down to 65535 (u16 output only).
    pub clipped_high: u64,
    /// Non-finite (`NaN`/`±inf`) samples. Counted separately because they signal
    /// a numerical fault rather than mere out-of-gamut clipping.
    pub non_finite: u64,
}

impl EncodeReport {
    /// Total finite samples clamped at a range end (excludes non-finite).
    pub fn clipped_total(&self) -> u64 {
        self.clipped_low + self.clipped_high
    }

    /// Whether any sample is problematic — clamped at a range end or non-finite.
    /// The condition a normal run surfaces as a warning and `--strict` promotes
    /// to an error.
    pub fn any_loss(&self) -> bool {
        self.clipped_total() > 0 || self.non_finite > 0
    }

    /// Fraction of examined samples that were clipped or non-finite, in `[0, 1]`.
    /// Returns 0.0 when no samples were examined.
    pub fn loss_fraction(&self) -> f64 {
        if self.total_samples == 0 {
            0.0
        } else {
            (self.clipped_total() + self.non_finite) as f64 / self.total_samples as f64
        }
    }
}

/// Per-channel statistics of the samples **as written** to the output file —
/// report-only, and the numeric basis for cross-version comparison
/// (`core/conversion-versioning`).
///
/// Only the mean is recorded, deliberately: for a fixed scan + recipe, the
/// per-channel *mean ΔRGB* between two builds is exactly the difference of the two
/// runs' per-channel means (`mean(a) - mean(b) = mean(a - b)`), so `nctool compare`
/// derives that metric from two run records without ever re-reading, registering,
/// or shipping pixels. Richer metrics (ΔE2000, SSIM) need real pixel access and
/// belong to the QA harness (design-spec §12 item 7), not here.
///
/// Units are the written sample's own domain: the u16 path reports the quantized
/// value scaled back to `[0, 1]` (so it is exact integer arithmetic, identical on
/// every target given identical pixels); the f32 path reports the verbatim
/// (unclamped, possibly > 1.0) float mean over the **finite** samples, with
/// non-finite samples excluded so one `NaN` cannot swallow the whole statistic —
/// `EncodeReport::non_finite` is where that fault is reported.
///
/// For a lossy JPEG output this is the normalized 8-bit primary-image buffer
/// handed to the compressor, not a decoder-dependent measurement after JPEG
/// reconstruction. That keeps the comparison basis deterministic without
/// pretending the codec preserves exact samples.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct OutputStats {
    /// Mean written sample value per channel `[r, g, b]`. Zero for an empty image.
    pub mean: [f64; 3],
}

/// What the encode stage produced: the loss accounting the orchestrator turns into
/// report warnings, plus the report-only per-channel statistics of the written
/// samples.
///
/// Bundled so the caller never has to re-read the output file to get the statistics.
/// The means are a **second** pass over the sample buffer (after `quantize_u16` /
/// the non-finite scan), not a free by-product of the first, and it is paid
/// unconditionally — including under `--report none`, where nothing consumes it.
/// Deliberate for now: making it conditional would push the report mode down into
/// `io::encode`, coupling the encoder to an orchestration concern for one linear scan
/// of already-hot memory. Revisit if `telemetry/perf-instrumentation` ever shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[must_use]
pub struct EncodeOutcome {
    pub loss: EncodeReport,
    pub stats: OutputStats,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nc_error_exit_codes() {
        assert_eq!(NcError::Other(String::new()).exit_code(), 1);
        assert_eq!(NcError::Usage(String::new()).exit_code(), 2);
        assert_eq!(NcError::Decode(String::new()).exit_code(), 3);
        assert_eq!(NcError::Unsupported(String::new()).exit_code(), 4);
        assert_eq!(NcError::Write(String::new()).exit_code(), 5);
        assert_eq!(NcError::Resource(String::new()).exit_code(), 6);
    }

    #[test]
    fn linear_image_new_checks_buffer_lengths() {
        // 2x1 RGB needs 6 floats; IR needs 2.
        assert!(LinearImage::new(2, 1, vec![0.0; 6], Some(vec![0.0; 2])).is_ok());
        assert!(LinearImage::new(2, 1, vec![0.0; 6], None).is_ok());
        // Wrong rgb length and wrong ir length both fail loudly.
        assert!(LinearImage::new(2, 1, vec![0.0; 5], None).is_err());
        assert!(LinearImage::new(2, 1, vec![0.0; 6], Some(vec![0.0; 3])).is_err());
        // Zero dimensions are rejected, not silently accepted as an empty image.
        assert!(LinearImage::new(0, 1, vec![], None).is_err());
        assert!(LinearImage::new(2, 0, vec![], None).is_err());
        // A pathological size that overflows is an error, not a panic.
        assert!(LinearImage::new(u32::MAX, u32::MAX, vec![0.0; 1], None).is_err());
    }

    #[test]
    fn film_base_array_round_trip() {
        let base = FilmBase::from([0.9, 0.5, 0.4]);
        assert_eq!(
            base,
            FilmBase {
                r: 0.9,
                g: 0.5,
                b: 0.4
            }
        );
        assert_eq!(<[f32; 3]>::from(base), [0.9, 0.5, 0.4]);
    }

    #[test]
    fn film_base_source_serializes_all_variants() {
        // Both variants are tagged objects.
        assert_eq!(
            serde_json::to_string(&FilmBaseSource::Region([1, 2, 3, 4])).unwrap(),
            r#"{"region":[1,2,3,4]}"#
        );
        for src in [
            FilmBaseSource::Region([1, 2, 3, 4]),
            FilmBaseSource::Explicit([0.9, 0.5, 0.4]),
        ] {
            let json = serde_json::to_string(&src).unwrap();
            assert_eq!(serde_json::from_str::<FilmBaseSource>(&json).unwrap(), src);
        }
    }

    #[test]
    fn input_axes_default_to_auto_and_round_trip() {
        // The two independent input axes default to `auto` and serialize in their
        // documented wire forms.
        let p = InputParams::default();
        assert_eq!(p.transfer, TransferAssertion::Auto);
        assert_eq!(p.meaning, MeaningAssertion::Auto);

        assert_eq!(
            serde_json::to_string(&TransferAssertion::Linear).unwrap(),
            "\"linear\""
        );
        assert_eq!(
            serde_json::to_string(&MeaningAssertion::ScannerDevice).unwrap(),
            "\"scanner-device\""
        );
        assert_eq!(
            serde_json::to_string(&MeaningAssertion::Colorimetric).unwrap(),
            "\"colorimetric\""
        );

        // A partial `input` section fills the untouched axis with its default.
        let p: InputParams = serde_json::from_str(r#"{"transfer":"linear"}"#).unwrap();
        assert_eq!(p.transfer, TransferAssertion::Linear);
        assert_eq!(p.meaning, MeaningAssertion::Auto);
    }

    #[test]
    fn film_type_defaults_to_unknown_and_declares_nothing() {
        // Undeclared is the default, and the declaration is provenance only: since
        // `ir-usability-detection` it gates nothing, so there is no predicate here
        // to assert — `film_base::ir_separability` measures the plane instead.
        assert_eq!(FilmType::default(), FilmType::Unknown);
        assert_eq!(InputParams::default().film_type, FilmType::Unknown);
    }

    #[test]
    fn film_type_round_trips_kebab_case() {
        assert_eq!(
            serde_json::to_string(&FilmType::Chromogenic).unwrap(),
            "\"chromogenic\""
        );
        for t in [FilmType::Unknown, FilmType::Silver, FilmType::Chromogenic] {
            let json = serde_json::to_string(&t).unwrap();
            assert_eq!(serde_json::from_str::<FilmType>(&json).unwrap(), t);
        }
        // A partial `input` section fills the untouched film_type with its default.
        let p: InputParams = serde_json::from_str(r#"{"film_type":"silver"}"#).unwrap();
        assert_eq!(p.film_type, FilmType::Silver);
        assert_eq!(p.transfer, TransferAssertion::Auto);
    }

    #[test]
    fn input_params_rejects_unknown_and_legacy_color_key() {
        // `deny_unknown_fields`: the removed combined `color` key is not a field,
        // so it is rejected at the struct level (the friendlier migration message
        // is emitted earlier, by `recipe::check_body`).
        assert!(serde_json::from_str::<InputParams>(r#"{"color":"linear"}"#).is_err());
    }
}

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

/// Reconstruction-type selector (design-spec §9, `--reconstruction` /
/// `reconstruction.type`).
///
/// A neutral selector that mirrors the CLI/recipe key, like the param structs —
/// it does not depend on the `algo` implementations. Serializes lowercase
/// (`"simple"` / `"density"`) and parses the same on the CLI via `ValueEnum`.
/// The full tagged configuration is [`Reconstruction`]; this is only its `type`
/// discriminator (the wire tag, the CLI value, and the telemetry summary).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ReconstructionType {
    /// Channel-inversion baseline — a **debugging** path, not a production
    /// one. Carries no density correction, curve or `Dmax`, so it isolates
    /// decode plus film base. **Not** the B&W path: B&W film is still a
    /// density medium with its own characteristic curve, so `algo/bw-support`
    /// runs through `density` and adds a mono colour model instead.
    Simple,
    /// Density-domain reconstruction (Cineon / negadoctor) — the default.
    #[default]
    Density,
}

/// Density-curve selector (design-spec §9, `--density-curve` /
/// `reconstruction.curve.type`). Like [`ReconstructionType`], only the `type`
/// discriminator of the tagged [`DensityCurve`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum DensityCurveType {
    /// The straight-line `10^(gamma·(D′ − Dmax))` curve — the explicit diagnostic
    /// option since 2026-08-08.
    Exponential,
    /// The S-curve (photographic H&D / paper-response) with toe/shoulder knees —
    /// **the default**, in step with [`DensityCurve::default`].
    #[default]
    Sigmoid,
    /// Invert the named film stock's published characteristic curve, per channel. Reads
    /// the film's measured response instead of modelling it, so there is no slope or
    /// anchor to set — both come from the curve. Select the stock with `--film-stock`.
    //
    // Kept free of rustdoc intra-doc links on purpose: clap renders this doc comment
    // verbatim as the value's `--help` text, where a `[`Type`]` link is noise the user
    // cannot follow. See [`CharacteristicParams`] for the knobs.
    Characteristic,
}

/// Output bit depth for the TIFF paths. Always *resolved* from the preset, by
/// [`OutputParams::depth`], and never stated: the `--out-depth` / `output.depth`
/// knob retired with the `legacy` and `custom` presets, the only two that read it.
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
/// governs **every** measurement path (`Dmin`/`Dmax`, content exposure and
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
/// `cli::validate` (so `convert`, `roll` and every per-frame override refuse a bad
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
             the film-base source, not the measured area — so if nothing should be \
             measured over that area, drop what measures it: `--auto-d-max` on the \
             current chain, or under `--new-flow` an auto white balance (`--auto-wb`, \
             or `scene_correction.white_balance` in the recipe).",
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

/// Where the film base comes from (design-spec §9, stage 2).
///
/// A single mutually-exclusive choice, not independent flags: more-specific
/// sources always win with no fallback, so this is one selection. Serializes as
/// `"auto"` / `{ "region": [x, y, w, h] }` / `{ "explicit": [r, g, b] }`.
///
/// The acquisition-ladder tier 3 **content-based source**
/// (`calibration.film_base = "content"` / `--base-content`) is owned by the separate
/// `film-base/content-fallback` task and is deliberately **not** a variant here —
/// the auto detector only *suggests* it on refusal, never falls back to it.
/// **Deliberately has no `Default`.** `Dmin` is a roll calibration, and picking
/// one silently is the difference between a measured conversion and a guessed
/// one — so `convert` requires the choice to be stated (see
/// [`CalibrationParams::film_base`]). `Auto` remains a perfectly good *stated* answer;
/// what is gone is arriving at it by omission.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FilmBaseSource {
    /// Estimate the base from the detected unexposed rebate band behind the
    /// film holder (the inward-scan detector; fails loudly on low confidence).
    Auto,
    /// Sample the base from this border region `[x, y, w, h]`.
    Region([u32; 4]),
    /// Explicit per-channel base transmission `[r, g, b]`.
    Explicit([f32; 3]),
}

/// The roll's measured values (design-spec §8, the recipe's `calibration`
/// section) — what was read off *this* film, as opposed to the look that is
/// chosen and reused across rolls. A pipeline profile is a recipe with no
/// `calibration` section; a roll calibration is a recipe with nothing else.
///
/// **Deliberately an open section, not a fixed set.** A key belongs here when it
/// is (a) measured from the film, (b) fixed across the roll, and (c) consumed by
/// a rule that lives elsewhere — the rule stays a look knob, the measurement it
/// reads lives here. That is why [`AnchorPlacement`] stays in
/// `reconstruction.curve` while the reference density it places does not. The
/// section is expected to grow: a roll **content white** (and possibly its
/// per-frame spread) joins it if `nf-calibration/anchor-comparison` picks a
/// content-referenced or hybrid placement — see `docs/spike/white-placement.md`.
/// So nothing here may assume a closed pair, and every member carries its own
/// optionality and its own default rather than the section carrying one for all
/// of them.
///
/// **Producing a calibration is not "one frame in, one calibration out".**
/// `film_base` and `dmax` each come from a single reference frame, but a roll
/// content white is a percentile taken across many frames. The acquisition
/// cascade that resolves a complete calibration is
/// `core/base-acquisition-planner`; this struct is only its shape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct CalibrationParams {
    /// Where the film base comes from — **required, with no default**.
    ///
    /// `None` means the user has not chosen, and `cli::validate` rejects it for
    /// `convert`/`roll` rather than silently estimating. The measurement commands
    /// exist to *produce* a base, so requiring one first would be circular:
    /// `estimate` resolves an unstated source to [`FilmBaseSource::Auto`], and
    /// `inspect` never reads this struct at all — it always runs the detector
    /// (`rebate_candidates` + `select_auto_base` directly).
    ///
    /// Why: `Dmin` is a per-roll calibration that sets both the black point and
    /// the colour balance (it is the divisor of the density conversion). Auto
    /// detection is best-effort on real scans — the rebate is a thin inset band,
    /// not the outer margin — so falling into it by omission produced conversions
    /// whose most important parameter nobody had decided. Stating `--auto-base`
    /// is still one flag; the point is that it is now a decision.
    pub film_base: Option<FilmBaseSource>,
    /// The roll's reference density — the `Dmax` a placement rule reads
    /// ([`DmaxSource`], default [`DmaxSource::Fixed`]).
    ///
    /// It lived in `reconstruction.curve` until `core/calibration-recipe-section`,
    /// where it sat only because it had once been a parameter of the exponential
    /// equation. It is a measurement of the roll, so it moved here; what stays in
    /// the curve is `anchor`, the *rule* that decides which tone the reference
    /// places.
    ///
    /// **Carried, not always consumed.** The `characteristic` curve reads its
    /// reference and its mid-grey placement off the stock's published response, so
    /// a `dmax` stated beside it is not read. That is accepted rather than
    /// rejected — composing a roll calibration with a stock-curve profile is the
    /// whole point of the section — and reported, so it is never silently ignored
    /// (`cli::unconsumed_dmax_warning`).
    pub dmax: DmaxSource,
}

/// Where the density render's display-white anchor (`Dmax`) comes from
/// (design-spec §7.2/§9, `calibration.dmax`).
///
/// A single mutually-exclusive choice, like [`FilmBaseSource`] — not independent
/// flags. `Dmax` is the corrected density that the render maps to display white
/// (`1.0`) so the default u16 encode fills the display range instead of leaving
/// all detail above `1.0`.
///
/// Like `Dmin`, `Dmax` is a **roll-fixed calibration** (a property of the film
/// stock + development + scanner), so the default is a *fixed* anchor reused
/// across the roll — not a per-frame measurement. The `dmax-reference` task
/// (design-spec §7.2/§12) established this: anchoring each frame's densest pixel
/// to display white is per-frame *exposure normalization* (it brightens
/// underexposed frames and forces an overcast grey to white), which conflicts
/// with NC's "convert faithfully, grade in Lightroom" purpose. The roll-fixed
/// anchor is resolved reference → per-stock constant → nominal: a value measured
/// once from a fully-exposed reference frame (or a known per-stock constant) is
/// carried here as [`Explicit`](Self::Explicit); with no calibration the default
/// [`Fixed`](Self::Fixed) nominal anchor applies. Serializes as `"fixed"` /
/// `{ "explicit": <d> }` / `"auto"` / `"none"`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DmaxSource {
    /// Fixed nominal anchor: a scene-independent corrected-density placement
    /// (`algo::density::NOMINAL_DMAX`), reused across every frame. The default
    /// when none of the `--d-max` / `--fixed-d-max` / `--auto-d-max` /
    /// `--no-d-max` flags is given — the roll-fixed behavior when no reference /
    /// per-stock value has been measured. Because it is a constant, darker frames
    /// render darker (faithful relative exposure), unlike the per-frame `Auto`.
    #[default]
    Fixed,
    /// Explicit scalar anchor density — the roll-fixed calibration value. Carries
    /// a `Dmax` measured once from a fully-exposed reference frame
    /// (`estimate --d-max-region`) or a known per-stock constant, reused across
    /// the roll exactly like an explicit `Dmin` base. Frozen into a roll recipe as
    /// `calibration.dmax = { "explicit": <d> }`.
    Explicit(f32),
    /// Measure the anchor per frame from the corrected-density distribution
    /// (a high percentile). **Per-frame exposure normalization** — an explicit
    /// opt-in (`--auto-d-max`), *demoted* from the former default: it silently
    /// brightens underexposed frames and breaks roll-to-roll consistency, so it
    /// is a grading convenience, not the faithful-conversion default.
    ///
    /// Measured over the **effective area** ([`MeasureParams`]), not the whole
    /// scan: the opaque film holder is at the density floor's other end and would
    /// own the top percentile outright (it resolved 2.23-2.37 against a roll `Dmax`
    /// of 1.28-1.38 that way, rendering every frame black). This is the **only**
    /// anchor source that reads anything off the frame, which is why it is also the
    /// only one for which `convert` resolves a region at all.
    Auto,
    /// No anchor: scene-referred output (base → `1.0`, exposed detail above it).
    /// Reproduces the pre-anchor render bit-for-bit — HDR f32 workflows rely on it.
    ///
    /// **Valid only with the exponential curve** (`cli::validate`): the sigmoid is
    /// anchored on `[0, Dmax]` and cannot run without a reference. The
    /// `characteristic` curve reads no reference at all, so this — like every other
    /// value here — is simply carried past it.
    ///
    /// This is the *reference*, not the anchor: [`AnchorPlacement`] derives the
    /// anchor from it, and the base-derived placements do not read it at all, so
    /// `"none"` beside one of them is a coherent combination — no reference exists
    /// and none is wanted. It resolves the reference to `0`, which reproduces the
    /// historical unity anchor `A = 0` only under [`AnchorPlacement::WhiteAtDmax`].
    /// Under [`AnchorPlacement::MidAtDmaxFraction`] the reference term vanishes but
    /// the `0.745/slope` term does not, so mid-grey ends up pinned that far above
    /// the base — legal, and drastic: at the default gamma it renders essentially
    /// the whole frame clipped. That is the same degeneracy
    /// [`AnchorPlacement::MidAtBaseOffset`] rejects at `offset = 0`; the
    /// reference-derived spelling of it validates clean because the bound there is
    /// on the fraction, not on the resolved anchor.
    None,
}

/// The roll's reference density as the reconstruction stage receives it: the
/// configured [`DmaxSource`] plus the region [`DmaxSource::Auto`] measures over.
///
/// **One value, because the region exists only for `Auto`.** It is the sole source
/// that reads anything off the frame, so a region beside any other source is
/// meaningless — passing them as two parameters let a caller state that
/// combination, and threaded a `measure_region` argument through six `stages::`
/// functions that nothing else reads.
///
/// This is a *stage input*, not a recipe section: `calibration.dmax` supplies the
/// source, and the orchestrator resolves the region from
/// `pipeline::film_base::effective_area`. The base is the other half of the same
/// recipe section but arrives differently — already resolved, as a `FilmBase` —
/// because `Auto` here needs the **post-regional-balance** densities and so cannot
/// be resolved before the stage runs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DmaxInput {
    /// The configured source (`calibration.dmax`).
    pub source: DmaxSource,
    /// The effective area [`DmaxSource::Auto`] measures its percentile over;
    /// `None` means the whole frame. Ignored by every other source.
    pub region: Option<[u32; 4]>,
}

impl DmaxInput {
    /// A source with no region — the whole-frame form, and what every non-`Auto`
    /// source resolves under.
    pub fn new(source: DmaxSource) -> Self {
        Self {
            source,
            region: None,
        }
    }
}

/// Where the regional (shadow/highlight) balance's tone-ramp anchors come from
/// (design-spec §7.2/§9, `density.balance_range`).
///
/// A single mutually-exclusive choice, like [`DmaxSource`] — not independent
/// flags. The ramps span the corrected-density range `[lo, hi]`: `lo` is the
/// positive's deepest shadow tone, `hi` its brightest highlight tone. `Auto`
/// (default) measures the range per frame from the pre-regional corrected
/// densities (robust percentiles of the per-pixel scalar tone) and reports the
/// measured `[lo, hi]`; `Explicit` fixes it. Roll reuse is measure-once-replay:
/// run one frame under `Auto`, read its reported range, then pass it as
/// `Explicit` on the rest for deterministic, frame-independent toning.
/// Serializes as `"auto"` / `{ "explicit": [lo, hi] }`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BalanceRange {
    /// Measure `[lo, hi]` per frame from the corrected-density distribution.
    #[default]
    Auto,
    /// Explicit `[lo, hi]` corrected-density anchors (e.g. a reused measured
    /// range for roll consistency). Requires `lo < hi`, both finite.
    Explicit([f32; 2]),
}

/// Density-reconstruction knobs (design-spec §9, `reconstruction.density`) —
/// everything that shapes the corrected density `D′` (stages 1–2). The
/// density→positive curve (gamma / contrast / `Dmax`) is deliberately **not**
/// here: it belongs to the tagged [`DensityCurve`], the separate curve stage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DensityParams {
    /// Per-channel density gain `[r, g, b]`.
    ///
    /// **Default `[1, 0.84, 0.73]` under the parametric curves, `[1, 1, 1]` under
    /// `characteristic`** — see [`DensityParams::default_scale_for`], which owns the split.
    /// Both non-unity values are calibrations rather than film constants. The scalar path
    /// leaves `contrast · (D'_c − D'_R)`, so a channel whose density rises faster than red
    /// drifts against it across the tone scale; this gain is what cancels that.
    ///
    /// **Calibrated from 31 hand-marked neutral patches** over five rolls (2026-09-16): each
    /// roll's median nulling scale, averaged with **equal weight per roll**, gives green
    /// 0.837 and blue 0.733. Weighting by roll rather than by patch is the point — 21 of the
    /// 31 patches are September, so a plain corpus median lands at 0.773 / 0.708 and lets one
    /// scan date set the default. `docs/progress/algo.md` (2026-09-16) carries the reasoning.
    ///
    /// Three caveats a caller should know. **Blue is the solid half and green is not**: every
    /// roll measured wants blue 0.68–0.78, so the previous `0.860` — taken from the
    /// manufacturers' published per-channel structure — overcorrects on this scanner, while
    /// green **splits by scan date** (July rolls 0.86–0.90, September ~0.77, consistent with
    /// a change of developer). `0.84` therefore fits neither group exactly and can overshoot
    /// toward green-yellow on a July roll. It nulls per-roll medians, not frames: patch-level
    /// scales span 0.64–0.95 green and 0.40–1.04 blue. And it is calibrated on **one
    /// scanner**, which is why the residual belongs to `io/scanner-density-calibration`
    /// rather than here.
    pub scale: [f32; 3],
    /// Per-channel density offset `[r, g, b]` (orange-mask compensation).
    pub offset: [f32; 3],
    /// Per-channel density offset `[r, g, b]` applied to the **positive's
    /// shadows** (low scalar tone density; the region near the film base).
    /// A positive value brightens that channel there — pushes the region toward
    /// that channel's color. `[0, 0, 0]` (default) is identity/off.
    pub shadow_balance: [f32; 3],
    /// Per-channel density offset `[r, g, b]` applied to the **positive's
    /// highlights** (high scalar tone density; the dense negative areas).
    /// Same sign convention as `shadow_balance`. `[0, 0, 0]` (default) is off.
    pub highlight_balance: [f32; 3],
    /// Tone-ramp anchor source for the regional balance (default `auto`).
    /// Only consulted when a balance is non-zero — the neutral default skips
    /// the regional pass entirely (bit-exact with the unbalanced output).
    pub balance_range: BalanceRange,
}

impl DensityParams {
    /// The per-channel gain a curve defaults to, and the **one** definition of that split.
    ///
    /// The gain corrects channels whose density rises faster than red, so the right value
    /// depends on how much of that a curve already handles itself:
    ///
    /// - `sigmoid` / `exponential` apply one scalar contrast to every channel, so they
    ///   carry no per-channel film model at all. The gain must cover both the film's own
    ///   channel structure *and* the scanner residual: measured `[1, 0.84, 0.73]`.
    /// - `characteristic` inverts each channel through its stock's published curve, so the
    ///   film half is already removed. On a datasheet neutral it is *exactly* neutral, and
    ///   the parametric gain then corrects a second time — measured on ten reference
    ///   frames, `|G/R − 1| + |B/R − 1|` rises from **0.039 to 0.185**. Its own solved
    ///   residual gain (`[1, 0.938, 0.985]`) measured 0.047, also worse than identity, so
    ///   this curve defaults to **`[1, 1, 1]`** and the scanner residual stays visible for
    ///   `io/scanner-density-calibration` to own rather than being half-absorbed here.
    ///
    /// The decomposition behind those numbers: the sigmoid path drifts green +0.79 / blue
    /// +1.26 stops per unit density and the characteristic path +0.40 / +0.09, so the curve
    /// removes (+0.39, +1.17) — almost exactly the sheets' predicted (+0.39, +1.29).
    ///
    /// Resolved in **three** places, all of which must stay in step with this function:
    /// `Reconstruction`'s `Deserialize` (a recipe that omits `density.scale`),
    /// `cli::merge`'s `--density-curve` arm (a switch resets it, and an explicit
    /// `--density-scale` merged afterwards still wins), and `cli::resolve_frames`'
    /// per-frame overlay in `roll`.
    ///
    /// The third is the one to remember, because its *mechanism* differs: the overlay is
    /// merged onto an already-serialized config, where `scale` is present whether or not
    /// the overlay mentioned it, so the `Deserialize` resolution cannot fire and the
    /// default is re-applied by hand instead.
    pub fn default_scale_for(curve: DensityCurveType) -> [f32; 3] {
        match curve {
            // Both parametric curves are scalar-contrast, so they share the calibration.
            DensityCurveType::Sigmoid | DensityCurveType::Exponential => [1.0, 0.84, 0.73],
            DensityCurveType::Characteristic => [1.0, 1.0, 1.0],
        }
    }
}

impl Default for DensityParams {
    fn default() -> Self {
        Self {
            // Derived from the default curve rather than restated, so `Self::default()`
            // and a recipe that omits the key cannot drift apart.
            scale: Self::default_scale_for(DensityCurveType::Sigmoid),
            offset: [0.0, 0.0, 0.0],
            shadow_balance: [0.0, 0.0, 0.0],
            highlight_balance: [0.0, 0.0, 0.0],
            balance_range: BalanceRange::Auto,
        }
    }
}

/// Where the print white-balance gains come from (design-spec §9,
/// `print.white_balance`).
///
/// A single mutually-exclusive choice, like [`FilmBaseSource`] / [`DmaxSource`] —
/// not parallel fields. Modeling the source as **one enum** is what makes the
/// precedence rule sound: an explicit `--white-balance 1,1,1` replaces a recipe's
/// auto mode *by source*, because the variant itself records where the gains came
/// from (explicit vs auto *provenance*), so precedence is decided by source, not
/// by value — a separate bool/Option pair would carry the value but not that
/// provenance. Serializes as
/// `{ "explicit": [r, g, b] }` / `"gray-world"` / `"percentile"`.
///
/// The auto modes are **deterministic statistics** over the rendered positive
/// (no ML, per the project's "AI-friendly ≠ ML" rule): same input + params ⇒
/// identical gains. The resolved gains ride into the convert JSON report so a
/// roll can freeze one frame's estimate into a recipe (measure once, reuse).
///
/// **Wire compatibility:** it *writes* the tagged form above, but its custom
/// [`Deserialize`] also accepts a legacy **bare `[r, g, b]` array**
/// (`"white_balance": [1, 1, 1]`) as `Explicit` gains — before this feature
/// `print.white_balance` was a plain `[f32; 3]`, so recipes/sidecars written by
/// older `nc` must still parse (reproducibility). See design-spec §9.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WbSource {
    /// Fixed per-channel gains `[r, g, b]`. The default (`[1, 1, 1]` = neutral,
    /// i.e. auto white balance off).
    Explicit([f32; 3]),
    /// Gray-world estimate (≈ NLP Auto-AVG): equalize the trimmed per-channel
    /// means. Simple, but a dominant scene color (a green lawn, a red wall)
    /// biases it — the whole frame is assumed to average to neutral.
    GrayWorld,
    /// Neutral-percentile estimate (≈ NLP Auto-Neutral): equalize the channels
    /// at a matched high percentile (near-white). More robust to dominant
    /// colors than gray-world — highlights are where neutrality matters most.
    Percentile,
}

impl Default for WbSource {
    fn default() -> Self {
        WbSource::Explicit([1.0, 1.0, 1.0])
    }
}

impl<'de> Deserialize<'de> for WbSource {
    /// Accepts both the current tagged form (`{ "explicit": [r, g, b] }` /
    /// `"gray-world"` / `"percentile"`) and the legacy **bare `[r, g, b]`** array
    /// that pre-`WbSource` recipes/sidecars wrote (when `print.white_balance` was
    /// a plain `[f32; 3]`), mapping the bare array to `Explicit`. Keeps old
    /// recipes reproducible; `Serialize` still emits only the tagged form.
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // A tagged copy of the variants (the derived shape), plus an untagged
        // wrapper that tries the bare array first, then the tagged form.
        #[derive(Deserialize)]
        #[serde(rename_all = "kebab-case")]
        enum Tagged {
            Explicit([f32; 3]),
            GrayWorld,
            Percentile,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Bare([f32; 3]),
            Tagged(Tagged),
        }
        Ok(match Repr::deserialize(deserializer)? {
            Repr::Bare(gains) | Repr::Tagged(Tagged::Explicit(gains)) => WbSource::Explicit(gains),
            Repr::Tagged(Tagged::GrayWorld) => WbSource::GrayWorld,
            Repr::Tagged(Tagged::Percentile) => WbSource::Percentile,
        })
    }
}

/// Print / tone-render knobs (design-spec §9). A **separate** sub-stage from
/// density conversion — the core fidelity rule; don't collapse the two.
///
/// Three of these knobs (`white_balance`, `print_exposure`, `black_point`) plus
/// [`linear_range`](Self::linear_range) are the **shared** print controls the
/// named-output split resolves once for both display branches
/// (`pipeline::render_split`, design-spec §6): the pinned order is
/// `white balance → exposure → black point → linear_range placement`.
/// [`display_tone`](Self::display_tone) and `highlight_compress` are deliberately
/// **not** applied in the shared stage: they resolve once into the tone each named
/// display renderer then scales into its own domain — see
/// [`DisplayTone`](crate::pipeline::display_tone::DisplayTone), which owns the knee
/// resolution so it is stated in exactly one place. Every preset except
/// `film-master` (which bypasses print and display entirely) goes through the shared
/// stage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PrintParams {
    /// Overall positive exposure, in stops (EV); `0.0` is neutral (design-spec §9).
    pub print_exposure: f32,
    /// Paper black / shadow floor.
    pub black_point: f32,
    /// Highlight/neutral white-balance gain source (default explicit `[1, 1, 1]`
    /// = neutral). Auto modes estimate the gains per frame; see [`WbSource`].
    pub white_balance: WbSource,
    /// Which tone curve the named display renderers apply (default `shoulder`).
    /// See [`DisplayToneCurve`]; display presets only, like
    /// [`linear_range`](Self::linear_range).
    pub display_tone: DisplayToneCurve,
    /// Named-display highlight roll-off amount. Non-negative; positive values
    /// move each branch's shoulder knee earlier without changing its fixed
    /// reference white or peak. It is a **width**, so a non-default one describes
    /// nothing under `display_tone = none` — that pairing is rejected, not silently
    /// dropped. The default `0` is the identity and asks for nothing, so it is
    /// accepted beside `none` (the rule is on the resolved value, like every other
    /// display-only rule).
    pub highlight_compress: f32,
    /// Black/white-range placement endpoints `[low, high]` in the rendered
    /// positive's linear domain — the exact affine `(x − low)/(high − low)` the
    /// shared display stage applies last (design-spec §6/§9,
    /// `print.linear_range` / `--linear-range LOW,HIGH`). The default `[0, 1]` is
    /// the exact identity. Requires finite `low < high`.
    ///
    /// This is the replacement home for `simple` reconstruction's removed
    /// `clip_low`/`clip_high` endpoints (design-spec §7.1) and is distinct from
    /// the density print `black_point`. Only the shared display stage consumes it:
    /// every display preset accepts a non-default value, while `film-master`
    /// rejects it loudly rather than silently ignoring it.
    pub linear_range: [f32; 2],
}

impl Default for PrintParams {
    fn default() -> Self {
        Self {
            print_exposure: 0.0,
            black_point: 0.0,
            white_balance: WbSource::default(),
            display_tone: DisplayToneCurve::default(),
            highlight_compress: 0.0,
            linear_range: [0.0, 1.0],
        }
    }
}

/// Which tone curve a named display preset applies (design-spec §6/§9,
/// `print.display_tone` / `--display-tone`).
///
/// This is the **selector**; the render stage pairs it with
/// `print.highlight_compress` into the resolved
/// [`DisplayTone`](crate::pipeline::display_tone::DisplayTone) that the SDR and HDR
/// renderers actually consume.
///
/// A *width* knob cannot express "off" — which is why this is a separate selector
/// and not a distinguished `highlight_compress` value: `highlight_compress` moves
/// the knee within a bounded `[0.5, 0.75]` and no value of it removes the curve.
///
/// **Room for an operator that carries parameters.** `output/display-tone-mapping`
/// wants a real tone-mapping operator with a stated white point (extended Reinhard
/// measured well). It arrives here as a *new variant with a payload* —
/// `Reinhard { white: … }` — and serde's default externally-tagged representation
/// makes that a pure addition: unit variants keep their bare-string spellings, so
/// every recipe and sidecar written today still parses, and only the new operator
/// needs an object (`{"reinhard": {…}}`). That is the same shape [`WbSource`]
/// (`{"explicit": [r, g, b]}` beside `"gray-world"`) and [`DmaxSource`] already use;
/// an internally-tagged `{"type": …}` form would have respelled these two and needed
/// a migration. `display_tone_wire_form_leaves_room_for_a_parameterized_operator`
/// pins it.
///
/// What such a variant *does* cost is the CLI side: `clap::ValueEnum` cannot derive
/// over a payload, so the selector then needs its own parse function (the
/// `OutputPreset::parse` pattern) plus a flag for the operator's parameter — exactly
/// how [`DmaxSource`] is spelled at the CLI. Recipe shape stays put; only the flag
/// wiring changes.
///
/// **The parameterized operator arrived (2026-09-01, `output/display-tone-mapping`).**
/// `Reinhard` is the payload variant this note anticipated, and the wire form held: the
/// two unit variants keep their bare-string spellings, so every recipe and sidecar
/// written before it still parses. The `clap::ValueEnum` derive is gone as predicted —
/// [`DisplayToneCurve::parse`] replaces it — and the operator's parameter arrives on its
/// own flag.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Default)]
// `Deserialize` is hand-written below — the derive could not accept the bare
// `"reinhard"` shorthand, and the accepted spellings must match what `--display-tone`
// takes. `Serialize` stays derived, so the canonical object form is what gets written.
#[serde(rename_all = "kebab-case")]
pub enum DisplayToneCurve {
    /// The shipped C¹ Hermite shoulder, its knee placed by `highlight_compress`.
    #[default]
    Shoulder,
    /// No display tone curve: the reconstruction alone places every tone.
    ///
    /// Gamut mapping and the transfer encode still run — this skips *tone*, not the
    /// rest of display rendering — and the renderers' range checks make it
    /// self-policing: a reconstruction that overshoots the render's ceiling — 1.0 on
    /// SDR, the 1000-nit peak on HDR — is a loud error rather than a quiet clip. The
    /// two ceilings differ by design, so the same overshoot can render on `hdr-pq`
    /// and be refused on `display-p3`. Pair it with a bounded reconstruction (a
    /// sigmoid with `shoulder > 0` and neutral print gains is bounded by
    /// construction).
    None,
    /// Extended Reinhard `v(1 + v/W²)/(1 + v)`, compressing globally against a stated
    /// white point rather than rolling off to a fixed ceiling.
    ///
    /// Unlike the other two this one is **not bounded**: content above the white point
    /// still exceeds the render's ceiling, so its loss is counted at the encode
    /// boundary instead of refused. That is the difference in kind from
    /// [`None`](Self::None), which relies on the range check *being* the policy — this
    /// operator is for a reconstruction that deliberately overshoots.
    ///
    /// **Taken by every display preset.** The HDR branch applies a *lifted* form over an
    /// asymptotic base ([`highlight_lifted_reinhard`]) so its composite stays strictly
    /// inside the 1000-nit peak, which is why the two branches need separate boundedness
    /// predicates: the same tone is unbounded on SDR and bounded on HDR.
    ///
    /// [`highlight_lifted_reinhard`]: crate::pipeline::display_tone::highlight_lifted_reinhard
    Reinhard {
        /// Specular headroom above reference white, in **stops** — how far above
        /// diffuse white content may sit and still be distinguishable, so
        /// `W = 2^headroom_stops`.
        ///
        /// Display-referred deliberately. The alternative was density above the
        /// reconstruction's anchor, which would make a *print* key read the
        /// reconstruction's anchor and contrast — the stage coupling
        /// `algo/split-default-migration` exists to remove — and cannot
        /// resolve under `simple` at all.
        ///
        /// `0` makes the operator the exact **identity** (`W = 1` gives
        /// `v(1+v)/(1+v) = v`) on **both** branches — the HDR form returns its input
        /// unchanged when the white point leaves no span above the crossover, which it
        /// needs an explicit early return for because its base is `v/(1 + v)` regardless
        /// of `W`. Without that, this setting was the identity on SDR and a full stop of
        /// darkening on the seven single-rendition HDR presets.
        ///
        /// So it coincides with [`None`](Self::None) in tone. On SDR they still differ in
        /// range policy — `None` refuses an overshoot, this counts it — while on HDR, where
        /// this tone is bounds-checked, zero headroom matches `None` including the refusal
        /// (`DisplayTone::applies_no_curve` is what routes both to the same diagnosis).
        ///
        /// Defaulted on the wire, so `{"reinhard": {}}` — and the bare `"reinhard"` —
        /// both resolve [`DEFAULT_HEADROOM_STOPS`]. Without that there was no recipe way
        /// to say "reinhard at the default", which the CLI has always accepted.
        #[serde(default = "default_headroom_stops")]
        headroom_stops: f32,
    },
}

/// Default specular headroom for [`DisplayToneCurve::Reinhard`], in stops.
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
/// Lives here beside the default and [`headroom_white_point`] so the CLI gate and the
/// renderer's `Headroom` cannot bound the knob differently — the failure that pattern
/// produces is a config `cli::validate` accepts and the render then refuses, at exit 1
/// after a whole roll has decoded.
pub const MAX_HEADROOM_STOPS: f32 = 24.0;

/// The white point a specular headroom asks for: `2^stops`.
///
/// The **single** definition. `DisplayToneCurve::white_point` (what the validation gate
/// and the report read) and `pipeline::display_tone::Headroom::white_point` (what the
/// renderer multiplies by) both call it, so a change to the stops→white-point meaning
/// cannot move one and leave the other.
pub fn headroom_white_point(stops: f32) -> f32 {
    stops.exp2()
}

/// Check a specular headroom in stops, or refuse it.
///
/// The single definition of the rule, called from both gates that need it:
/// `cli::validate` (so `roll` and every per-frame override inherit it *before* a decode)
/// and `pipeline::display_tone::Headroom::new` (so a stage caller cannot skip it). The
/// stage check is deliberately a duplicate, not a fallback — see that constructor.
///
/// A negative headroom is not loud on its own: `2^-40` is a white point of ~9e-13, which
/// maps essentially every sample past the ceiling and turns the render into a solid
/// white field at exit 0 with the clip merely *counted*.
pub fn check_headroom_stops(stops: f32) -> Result<()> {
    if !stops.is_finite() || stops < 0.0 {
        return Err(NcError::Usage(format!(
            "--display-tone-headroom / print.display_tone.reinhard.headroom_stops must \
             be finite and non-negative (got {stops}). It is specular headroom above \
             reference white in stops; `0` is the identity."
        )));
    }
    if stops > MAX_HEADROOM_STOPS {
        return Err(NcError::Usage(format!(
            "--display-tone-headroom / print.display_tone.reinhard.headroom_stops is \
             {stops} stops, beyond the supported maximum of {MAX_HEADROOM_STOPS}. Above \
             ~8 stops the operator converges on plain Reinhard and the extra headroom \
             buys nothing; the measured useful range is 4–8 (the default \
             {DEFAULT_HEADROOM_STOPS} is a white point of {}).",
            headroom_white_point(DEFAULT_HEADROOM_STOPS)
        )));
    }
    Ok(())
}

impl DisplayToneCurve {
    /// The accepted `--display-tone` spellings, for diagnostics and the help text.
    pub const NAMES: [&'static str; 3] = ["shoulder", "none", "reinhard"];

    /// The accepted names as a comma-separated backticked list, for diagnostics.
    /// Shared by the flag parser and the recipe deserializer so a new operator cannot
    /// reach one list and not the other.
    fn accepted_list() -> String {
        Self::NAMES
            .iter()
            .map(|n| format!("`{n}`"))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Parse a `--display-tone` value.
    ///
    /// Hand-written because `clap::ValueEnum` cannot derive over a payload variant —
    /// the cost this type's docs predicted. `reinhard` resolves the documented default
    /// headroom; `--display-tone-headroom` refines it.
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "shoulder" => Ok(Self::Shoulder),
            "none" => Ok(Self::None),
            "reinhard" => Ok(Self::Reinhard {
                headroom_stops: DEFAULT_HEADROOM_STOPS,
            }),
            other => Err(NcError::Usage(format!(
                "unknown --display-tone `{other}`; expected one of {}",
                Self::accepted_list()
            ))),
        }
    }

    /// The operator's white-point **parameter**: the linear multiple of reference
    /// white that sizes the curve, not the input mapping to display white — the
    /// unity point sits at `W / gain` (see
    /// `pipeline::display_tone::Headroom::white_point`). `None` for the two curves
    /// that have no white point.
    pub fn white_point(self) -> Option<f32> {
        match self {
            Self::Shoulder | Self::None => Option::None,
            Self::Reinhard { headroom_stops } => Some(headroom_white_point(headroom_stops)),
        }
    }

    /// Whether this operator has a knee for `print.highlight_compress` to place.
    ///
    /// Only the shoulder does. Named rather than spelled `== Shoulder` at each site so
    /// the "a stated knee width would be silently ignored" rule stays one predicate
    /// across `cli::validate` and `DisplayTone::resolve`.
    pub fn has_knee(self) -> bool {
        matches!(self, Self::Shoulder)
    }

    /// The selector as a *diagnostic* names it: the flag spelling, plus the operator's
    /// own parameter where it has one.
    ///
    /// [`Display`](std::fmt::Display) is deliberately the bare, retypeable flag spelling
    /// — `--display-tone` takes the name alone. A message that reports a **value** the
    /// user set needs the parameter too, or the two configs `reinhard` at 6 and at 24
    /// stops produce word-for-word identical errors and the user is told which knob is
    /// non-default but never which value.
    pub fn described(self) -> String {
        match self {
            Self::Shoulder | Self::None => self.to_string(),
            Self::Reinhard { headroom_stops } => {
                format!("{self} at {headroom_stops} stops of headroom")
            }
        }
    }

    /// Check the operator's own parameter, or refuse it.
    ///
    /// A **value** rule, so it belongs to `cli::validate` rather than
    /// `validate_convert`: `roll` and every per-frame override must inherit it, and they
    /// go through `validate` only.
    pub fn check_parameters(self) -> Result<()> {
        match self {
            Self::Shoulder | Self::None => Ok(()),
            Self::Reinhard { headroom_stops } => check_headroom_stops(headroom_stops),
        }
    }
}

/// [`DEFAULT_HEADROOM_STOPS`] as serde's field default. A function because
/// `#[serde(default = …)]` names one; it exists only to avoid a second literal.
fn default_headroom_stops() -> f32 {
    DEFAULT_HEADROOM_STOPS
}

impl<'de> Deserialize<'de> for DisplayToneCurve {
    /// Accepts the canonical externally-tagged forms plus the **bare operator name**:
    /// `"shoulder"`, `"none"`, `"reinhard"`, `{"reinhard": {}}` and
    /// `{"reinhard": {"headroom_stops": 6.0}}` all parse. `Serialize` still emits only
    /// the canonical form, so a round trip normalizes.
    ///
    /// Hand-written for one reason: **every spelling this type hands a user must parse
    /// back.** `Display` is the bare flag name (`--display-tone reinhard`), and the
    /// validation messages interpolate it beside `print.display_tone` — so a derived
    /// `Deserialize`, which rejects a bare string for a struct variant, made those
    /// messages quote a recipe value the parser refused. Same shape as [`WbSource`]:
    /// a shorthand accepted on input, one canonical form on output.
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        /// The payload, with its own `deny_unknown_fields`: a mistyped nested key must be
        /// loud, like every other recipe object.
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ReinhardParams {
            #[serde(default = "default_headroom_stops")]
            headroom_stops: f32,
        }

        // A self-describing intermediate, like `DensityCurve`'s: the two shapes are a
        // string and a single-key object, which no derive spells in one type.
        let value = serde_json::Value::deserialize(deserializer)?;
        let unknown = |name: &str| {
            D::Error::custom(format!(
                "unknown print.display_tone `{name}`; expected one of {}",
                Self::accepted_list()
            ))
        };
        match value {
            serde_json::Value::String(name) => match name.as_str() {
                "shoulder" => Ok(Self::Shoulder),
                "none" => Ok(Self::None),
                "reinhard" => Ok(Self::Reinhard {
                    headroom_stops: DEFAULT_HEADROOM_STOPS,
                }),
                other => Err(unknown(other)),
            },
            serde_json::Value::Object(obj) => {
                if obj.len() != 1 {
                    return Err(D::Error::custom(format!(
                        "print.display_tone must name exactly one tone curve (got {} keys); \
                         expected one of {}, either bare (\"reinhard\") or as a single-key \
                         object ({{\"reinhard\": {{\"headroom_stops\": 6.0}}}})",
                        obj.len(),
                        Self::accepted_list()
                    )));
                }
                let (tag, payload) = obj.into_iter().next().expect("exactly one key");
                match tag.as_str() {
                    "reinhard" => {
                        // Re-worded rather than forwarded: serde's own message names the
                        // private `ReinhardParams`, which appears in no doc, no `--help`
                        // and no recipe key, so a user cannot map it back to anything.
                        // The field errors it raises (unknown key, wrong value type) are
                        // already well-worded, so only the "not an object at all" case
                        // needs replacing.
                        let params: ReinhardParams =
                            serde_json::from_value(payload).map_err(|e| {
                                let text = e.to_string();
                                if text.contains("ReinhardParams") {
                                    D::Error::custom(format!(
                                        "print.display_tone.reinhard must be an object of \
                                         its parameters ({{\"headroom_stops\": 6.0}}), or \
                                         empty ({{}}) for the default {DEFAULT_HEADROOM_STOPS} \
                                         stops; the bare string \"reinhard\" also works"
                                    ))
                                } else {
                                    D::Error::custom(format!("print.display_tone.reinhard: {text}"))
                                }
                            })?;
                        Ok(Self::Reinhard {
                            headroom_stops: params.headroom_stops,
                        })
                    }
                    // Named individually: "this one takes no parameter" is a better
                    // diagnosis than "unknown tone", and it is the mistake a user makes
                    // after seeing the reinhard object form.
                    "shoulder" | "none" => Err(D::Error::custom(format!(
                        "print.display_tone `{tag}` takes no parameters — write it as the \
                         bare string \"{tag}\". Only `reinhard` carries an object."
                    ))),
                    other => Err(unknown(other)),
                }
            }
            other => Err(D::Error::custom(format!(
                "print.display_tone must be a tone name or a single-key object (got {other})"
            ))),
        }
    }
}

impl std::fmt::Display for DisplayToneCurve {
    /// The **flag spelling**, deliberately without the operator's parameter.
    ///
    /// `display_tone_display_impl_matches_its_serde_spelling` left this decision to
    /// whoever added a parameterized variant. The property the validation messages
    /// depend on is that a spelling handed to a user is one they can *type*, and
    /// `--display-tone` takes the bare name — the headroom arrives on its own flag. So
    /// `reinhard` spells `reinhard`; a message wanting the value interpolates it
    /// separately rather than making this string un-typeable.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            DisplayToneCurve::Shoulder => "shoulder",
            DisplayToneCurve::None => "none",
            DisplayToneCurve::Reinhard { .. } => "reinhard",
        })
    }
}

/// Exponential-curve knobs (design-spec §7.2/§9,
/// `reconstruction.curve.type = "exponential"`): the straight-line
/// `10^(gamma·(D′ − Dmax))` density→positive mapping. The curve owns the
/// placement *rule* ([`AnchorPlacement`]); the reference density `Dmax` it places
/// is a roll measurement and lives in [`CalibrationParams::dmax`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExponentialParams {
    /// Film/print curve gamma (the straight line's slope).
    pub gamma: f32,
    /// Which tone is pinned, and at what density (default: display white at the
    /// reference — this curve's historical and only behaviour before 2026-08).
    ///
    /// Kept at `white-at-dmax` deliberately rather than moved to a base-derived rule.
    /// Since `pipeline_version` 2 this curve is the explicit diagnostic straight line, and
    /// its value there is being the *debuggable reference*: `white-at-dmax` renders
    /// bit-identically to every build before this field existed, which a golden pins.
    /// Moving the default is a rendering decision with its own evidence bar — the floor
    /// value for [`AnchorPlacement::BlackAtBase`] has not been chosen on this curve's own
    /// measurements — and it is tracked in `algo/exponential-anchor-placement`.
    pub anchor: AnchorPlacement,
}

impl Default for ExponentialParams {
    fn default() -> Self {
        Self {
            // `2.0`, not the straight line's `1.0` (2026-08-08). Contrast 1.0 is
            // the slope at which this curve confines the whole image to
            // `Dmax` decades and leaves the black floor at 57–72/255 — the
            // measured "pale, not dark" defect `algo/reference-anchored-sigmoid`
            // was opened for. `reports/sigmoid-reference-baseline.md` measured the
            // remedy on user-confirmed shadow patches: contrast 2.0 takes the
            // floor 72 → 12/255.
            //
            // It is a partial fix and the report says so — and which fix it is depends
            // on `anchor` (below). Under this curve's *default* placement,
            // `WhiteAtDmax`, white is pinned at `Dmax`, so steepening pivots the line
            // *around white* and drags everything below it down, costing 2.75 EV of
            // midtone placement. The two knobs fight, which is exactly what an anchor
            // other than white avoids, and why the sigmoid is the default curve; at this
            // default the slope is therefore the better of two imperfect slopes rather
            // than a calibrated value. Under either base-derived placement they stop
            // fighting and 2.0 is no longer a compromise — it is roughly what
            // `target_system_gamma / film_gamma` asks for (`1.2 / 0.6`), and within 3% of
            // the datasheet route
            // `MID_GREY_OUTPUT_DECADES / REFERENCE_MID_TO_WHITE_DELTA` ≈ 2.07.
            gamma: 2.0,
            anchor: AnchorPlacement::WhiteAtDmax,
        }
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
/// A per-stock value belongs to `algo/film-stock-profiles`; until that registry exists this
/// single professional-film figure is the reference.
pub const REFERENCE_MID_TO_WHITE_DELTA: f32 = 0.36;

/// The default [`SigmoidParams::contrast`], **derived rather than chosen**.
///
/// Placing a mid-grey [`REFERENCE_MID_TO_WHITE_DELTA`] below white, and requiring it to
/// land at 0.18 of white, fixes the slope: `contrast = 0.745 / 0.36 ≈ 2.07`. Two
/// independent checks agree — it implies a film gamma of `0.36 / log10(0.90/0.18) = 0.52`,
/// textbook for colour negative, and an overall system gamma of `0.745/0.699 = 1.07`, i.e.
/// near-faithful reproduction of scene luminance ratios. It also sits inside the
/// conventional 1.7–2.2 negative→print range.
///
/// The previous default of `1.0` is the value at which the S-curve reduces bit-exactly to
/// the straight line, which suggests it was a testability default rather than a rendering
/// intent. It confined the whole image to `contrast · Dmax ≈ 1.3` decades and left the black
/// floor at 57–72/255 — the measured defect `algo/reference-anchored-sigmoid` was opened for.
pub const REFERENCE_CONTRAST: f32 = MID_GREY_OUTPUT_DECADES / REFERENCE_MID_TO_WHITE_DELTA;

/// The default [`SigmoidParams::shoulder`].
///
/// Chosen because it begins bending at `D′ ≈ 0.70`, essentially at mid-grey (≈0.67), which
/// is where a print shoulder belongs. The previous `0.2` only starts at 0.95 and so
/// collapses highlight differentiation to *zero* above the anchor once the anchor moves down
/// to diffuse white; `1.0` starts at 0.45 and is no longer a highlight shoulder at all but a
/// flattening of the entire upper range. Measured on real frames: the output gap across a
/// curtain's density range is 0.00003 at 0.2, 0.0164 at 0.6 and 0.0502 at 1.0, while
/// mid-grey placement costs 0.1799 → 0.1740 → 0.1525.
pub const REFERENCE_SHOULDER: f32 = 0.6;

/// Which tone a density curve pins, and at what density (design-spec §7.3/§9,
/// `reconstruction.curve.anchor`).
///
/// The curve is an affine map in log space: a slope ([`SigmoidParams::contrast`], or the
/// exponential's [`ExponentialParams::gamma`]) plus **one** pinned `(density, output)` pair,
/// from which everything else follows. This enum is that choice — a single
/// mutually-exclusive rule, like [`DmaxSource`], not independent fields. **Both curves share
/// it**: placement is orthogonal to curve shape, which is what let the 2026-08-03 candidate
/// harness score eight anchoring forms through one curve implementation.
///
/// Why it exists: pinning white at the reference density placed midtones 2.5–3.6 stops too
/// dark once the contrast became photographic, because steepening the line pivots it *around
/// white* and drags everything below down. Pinning a mid-tone instead removes that conflict.
///
/// # Reference-derived versus base-derived
///
/// The first two variants scale with the resolved reference density; the last two do not.
/// That distinction is the load-bearing one, because the reference is a **leader** density —
/// film saturation, not diffuse white — and two rolls of one stock have measured 0.295 apart
/// while their bases agreed to 0.0005 (`film-base/dmax-anchor-reliability`). Since
/// `dAnchor/dReference` is the reference's coefficient, that spread reaches the render
/// multiplied by it: 1.96 stops at [`Self::WhiteAtDmax`], 0.98 at a
/// [`Self::MidAtDmaxFraction`] of 0.5, and **zero** for the base-derived pair, which never
/// read the reference at all. Base-derived placement is the only form with no roll-to-roll
/// term.
///
/// The base needs no measurement here: stage 1 divides it out
/// (`D = −log10(scan / base)`), so the film base *is* `D′ = 0` by construction, modulo the
/// `density.offset` and regional balance the user asked for.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnchorPlacement {
    /// Pin **display white** at the resolved reference density. The pre-2026-08 behaviour,
    /// kept as an explicit diagnostic: it is what makes the reference's own unreliability
    /// fully visible in the render (`film-base/dmax-anchor-reliability`).
    WhiteAtDmax,
    /// Pin **mid-grey** (output 0.18) at `fraction × reference`, letting white fall where it
    /// must — above the reference, where the shoulder compresses it, which is what a print
    /// shoulder is for.
    ///
    /// `0.5` is the default. Mid-grey sits roughly half-way up the measured reference range
    /// (measured 0.48–0.57 across the fixture stocks), and this form is **half as sensitive**
    /// to a reference error as pinning white: `dAnchor/dReference` is `fraction`, so the
    /// 0.046 density disagreement seen between two rolls of one stock costs 0.15 stop rather
    /// than 0.31.
    MidAtDmaxFraction(f32),
    /// Pin the **film base** to a stated output `floor`, letting white fall where the
    /// contrast puts it. Reference-free, so it carries no roll-to-roll term at all.
    ///
    /// The base is `D′ = 0`, so `10^(contrast·(0 − A)) = floor` gives
    /// `A = −log10(floor)/contrast`. That reproduces the anchors the 2026-08-03 candidate
    /// retest recorded — floor 0.002 → 1.349, floor 0.005 → 1.151 at contrast 2.0 — which
    /// is a free check on this arithmetic.
    ///
    /// `floor` is **linear light against the 203-nit reference white**, not an sRGB code
    /// value; 0.005 encodes to about 16/255, not 5/255. (The candidate-5b row in
    /// `reports/sigmoid-reference-baseline.md` reads 20/255 for this floor — that is the
    /// darkest confirmed *shadow patch*, which sits just above the base, not the base.)
    ///
    /// On the **sigmoid** this places the straight-line-extrapolated base, not the rendered
    /// one — the toe carries the actual base below `floor`. That is the same approximation
    /// [`Self::MidAtDmaxFraction`] already makes on that curve, not a new one.
    BlackAtBase(f32),
    /// Pin **mid-grey** (output 0.18) at `offset` density *above the film base*, rather
    /// than at a fraction of the reference. Reference-free, like [`Self::BlackAtBase`].
    ///
    /// `A = offset + 0.745/contrast`. This is the shape of the leading candidate from
    /// `algo/reference-anchored-sigmoid` (mid at `Dmin` + a per-stock datasheet offset,
    /// which scored best of every shippable form). The per-stock offset itself is **not**
    /// available yet — it needs `mid aim − D-min`, and the registry's `D-min` figures are
    /// chart reads that `algo/film-stock-profiles` forbids any render path from consuming.
    /// Until then this variant takes the offset explicitly.
    MidAtBaseOffset(f32),
}

impl Default for AnchorPlacement {
    fn default() -> Self {
        AnchorPlacement::MidAtDmaxFraction(0.5)
    }
}

impl AnchorPlacement {
    /// Derive the curve's anchor `A` from the resolved reference density and the contrast.
    ///
    /// `A` is the density that maps to display white, so pinning mid-grey at `M` means
    /// solving `10^(contrast·(M − A)) = 0.18`, i.e. `A = M + 0.745/contrast`.
    ///
    /// The base-derived variants ignore `reference` entirely — that is the whole point of
    /// them, not an oversight.
    pub fn anchor(self, reference: f32, contrast: f32) -> f32 {
        match self {
            AnchorPlacement::WhiteAtDmax => reference,
            AnchorPlacement::MidAtDmaxFraction(f) => {
                f * reference + MID_GREY_OUTPUT_DECADES / contrast
            }
            AnchorPlacement::BlackAtBase(floor) => -floor.log10() / contrast,
            AnchorPlacement::MidAtBaseOffset(offset) => offset + MID_GREY_OUTPUT_DECADES / contrast,
        }
    }

    /// Whether resolving this placement **consumes** the reference density
    /// (`calibration.dmax`). The base-derived pair does not: hand [`Self::anchor`] any
    /// reference at all and they return the same number.
    ///
    /// **This, not [`DmaxSource`] alone, is the question every `Dmax`-policy gate must
    /// ask.** `auto` measures the reference per frame, which is frame-local adaptation
    /// only if something *reads* it — under `black-at-base` / `mid-at-base-offset` the
    /// measurement is computed and discarded, so the render is deterministic and
    /// roll-consistent. Gating on the source alone made `film-master` hard-reject a valid
    /// config and made `roll` warn (and `--strict` fail) about a consistency break that
    /// cannot happen.
    pub fn reads_reference(self) -> bool {
        match self {
            AnchorPlacement::WhiteAtDmax | AnchorPlacement::MidAtDmaxFraction(_) => true,
            AnchorPlacement::BlackAtBase(_) | AnchorPlacement::MidAtBaseOffset(_) => false,
        }
    }
}

/// Sigmoid-curve knobs (design-spec §7.3/§9,
/// `reconstruction.curve.type = "sigmoid"`): the S-curve mapping corrected
/// density to positive linear. It shares the density-reconstruction stage
/// ([`DensityParams`]) with the exponential curve; `contrast` is the `gamma`
/// analogue (gamma itself exists only in the exponential variant). The reference
/// density both knees derive from is a roll measurement and lives in
/// [`CalibrationParams::dmax`]; what this curve owns is the placement *rule*.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SigmoidParams {
    /// Mid-density slope of the curve in log-output space (the exponential
    /// `gamma` analogue). Must be finite and > 0.
    ///
    /// The default is **derived, not chosen**: see [`REFERENCE_CONTRAST`].
    pub contrast: f32,
    /// Toe (shadow) knee width in log10 density units: how softly the curve
    /// approaches paper black. `0` disables the toe (hard straight-line black).
    pub toe: f32,
    /// Shoulder (highlight) knee width in log10 density units: how softly the
    /// curve approaches display white. `0` disables the shoulder.
    pub shoulder: f32,
    /// Which tone is pinned, and at what density (default: mid-grey at half the reference).
    ///
    /// [`DmaxSource::None`] is rejected beside this curve (`cli::validate`) — the
    /// S-curve needs a positive reference to place anything against.
    pub anchor: AnchorPlacement,
}

impl Default for SigmoidParams {
    fn default() -> Self {
        Self {
            contrast: REFERENCE_CONTRAST,
            toe: 0.2,
            shoulder: REFERENCE_SHOULDER,
            anchor: AnchorPlacement::default(),
        }
    }
}

/// A film stock with a digitized characteristic curve
/// (`reconstruction.curve.stock`, `--film-stock`).
///
/// One enum field rather than parallel options, per the project rule for mutually
/// exclusive knobs. **Naming a stock is a refinement, never a precondition**: an unnamed
/// stock resolves to [`Self::GenericC41`], which is the average of the nine measured
/// stocks and renders correctly on any of them. A stock that is *named but unknown* is a
/// loud usage error listing the accepted spellings — silently falling back would hide a
/// typo behind a plausible render.
///
/// The variants are exactly the entries in `algo::film_stock::curves::STOCKS`; a test
/// pins that correspondence, because a variant with no table would panic at render time.
///
/// `Serialize`/`Deserialize` are written by hand against [`FilmStock::as_str`] and
/// [`FilmStock::parse`] rather than derived: serde's `kebab-case` renames `Portra400` to
/// `portra400` (it splits on case boundaries, and there is none before a digit), which
/// would give the recipe a *different* spelling from the CLI flag and the curve-table key.
/// An emitted recipe would then fail to load back — the round-trip nc's determinism
/// contract rests on. One spelling, one function.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FilmStock {
    /// The average of the nine measured stocks (ten ship; this one is derived) — red
    /// mid-scale gamma 0.541 with mid-grey
    /// 0.624 density above base. For scale: the per-stock spread is 0.50–0.61 and
    /// 0.54–0.70, and ACES's own generic film model sits at 0.55 / 0.70.
    #[default]
    GenericC41,
    Ektar100,
    Portra160,
    /// The discontinued vivid-colour Portra. Kept because its aim table is the evidence
    /// that `Δ` is genuinely stock-dependent (0.41 against the NC pair's 0.36 at the same
    /// speed), which a registry of only current stocks would not show.
    Portra160vc,
    Portra400,
    Portra400vc,
    /// Box speed (EI 800). The published push curves are a separate response and are not
    /// in the registry — a pushed roll is a different development, not a different stock.
    Portra800,
    Gold200,
    Ultramax400,
    /// Box speed (EI 800). Digitizes to the **same curve as [`Self::Portra800`]** — D-min
    /// and per-channel gamma agree to 0.003 — from a different publication, year and page,
    /// which is an independent check on the extraction as much as a fact about the film.
    Ultramax800,
}

impl FilmStock {
    /// Every variant, in the order `--film-stock` and the parse diagnostics list them.
    pub const ALL: &'static [FilmStock] = &[
        FilmStock::GenericC41,
        FilmStock::Ektar100,
        FilmStock::Portra160,
        FilmStock::Portra160vc,
        FilmStock::Portra400,
        FilmStock::Portra400vc,
        FilmStock::Portra800,
        FilmStock::Gold200,
        FilmStock::Ultramax400,
        FilmStock::Ultramax800,
    ];

    /// The wire spelling — the recipe value, the CLI value, and the key into the pinned
    /// curve table, which is why it is one function rather than three.
    pub fn as_str(self) -> &'static str {
        match self {
            FilmStock::GenericC41 => "generic-c41",
            FilmStock::Ektar100 => "ektar-100",
            FilmStock::Portra160 => "portra-160",
            FilmStock::Portra160vc => "portra-160vc",
            FilmStock::Portra400 => "portra-400",
            FilmStock::Portra400vc => "portra-400vc",
            FilmStock::Portra800 => "portra-800",
            FilmStock::Gold200 => "gold-200",
            FilmStock::Ultramax400 => "ultramax-400",
            FilmStock::Ultramax800 => "ultramax-800",
        }
    }

    /// Parse a CLI/recipe spelling, listing the accepted names on failure.
    pub fn parse(name: &str) -> std::result::Result<Self, String> {
        Self::ALL
            .iter()
            .copied()
            .find(|s| s.as_str() == name)
            .ok_or_else(|| {
                format!(
                    "unknown film stock `{name}` (accepted: {})",
                    Self::ALL
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
    }
}

impl Serialize for FilmStock {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for FilmStock {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        FilmStock::parse(&name).map_err(serde::de::Error::custom)
    }
}

/// Knobs for [`DensityCurve::Characteristic`] — deliberately just the stock.
///
/// There is no contrast and no anchor here, and that absence is the design: both are
/// *read off* the published curve rather than chosen, which is what makes this stage
/// honest to the film. Anything that shapes tone belongs to `print.display_tone`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CharacteristicParams {
    /// Which stock's published response to invert.
    pub stock: FilmStock,
}

/// The tagged density→positive curve (design-spec §8/§9,
/// `reconstruction.curve`): exactly one of the three curve variants, each carrying
/// its own knobs — the two parametric ones plus the `Dmax` placement they own, and
/// `characteristic`, which owns neither because it reads both off the published curve. Serializes internally tagged
/// (`{"type":"exponential","gamma":…,"dmax":…}`); the custom `Deserialize`
/// mirrors that while rejecting cross-variant keys **by name** (e.g. `contrast`
/// under `exponential`) with a loud usage message instead of serde's generic
/// unknown-field error, and fills each variant's defaults for omitted fields.
/// The `type` tag is required whenever a `curve` object is present; only a fully
/// *omitted* `reconstruction.curve` defaults, to this enum's [`Default`] — the
/// **sigmoid**, with its defaults.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum DensityCurve {
    Exponential(ExponentialParams),
    Sigmoid(SigmoidParams),
    Characteristic(CharacteristicParams),
}

impl Default for DensityCurve {
    /// **The sigmoid (2026-08-08).** The exponential straight line was the default
    /// through Step 1 because it was the first curve implemented, not because it
    /// renders film well: pinning white at `Dmax` with a photographic slope is
    /// geometrically unable to place both the black floor and the midtones (see
    /// [`ExponentialParams::default`] and `reports/sigmoid-reference-baseline.md`).
    /// The sigmoid pins **mid-grey** instead, which removes that conflict, and its
    /// contrast/shoulder defaults are *derived* from manufacturer reference
    /// densities rather than chosen.
    ///
    /// `algo/sigmoid-parameter-calibration` remains open, so those values are
    /// provisional — but provisional-and-derived beats a slope that was never a
    /// rendering intent. The exponential stays available as the explicit
    /// diagnostic straight line.
    fn default() -> Self {
        DensityCurve::Sigmoid(SigmoidParams::default())
    }
}

impl DensityCurve {
    /// This curve's `type` discriminator (wire tag / CLI value).
    pub fn curve_type(&self) -> DensityCurveType {
        match self {
            DensityCurve::Exponential(_) => DensityCurveType::Exponential,
            DensityCurve::Sigmoid(_) => DensityCurveType::Sigmoid,
            DensityCurve::Characteristic(_) => DensityCurveType::Characteristic,
        }
    }

    /// Whether this curve reads the roll's reference density
    /// ([`CalibrationParams::dmax`]) at all — the variant-agnostic question every
    /// `Dmax`-policy site asks now that the value itself lives outside the curve.
    ///
    /// `false` for [`Self::Characteristic`], which resolves neither a reference nor a
    /// placement: both come from the stock's published response. A `calibration.dmax`
    /// stated beside it is **carried, not consumed** — accepted, so a roll calibration
    /// composes with a stock-curve profile, and reported, so it is never silently
    /// ignored.
    ///
    /// Note this answers a *narrower* question than "does the render depend on the
    /// reference": a parametric curve under a base-derived placement consumes no
    /// reference either. That is [`AnchorPlacement::reads_reference`], and the two
    /// must be asked together — see `cli::region_reaches_a_rendered_pixel`.
    pub fn consumes_reference(&self) -> bool {
        self.curve_type().consumes_reference()
    }

    /// The anchor-placement rule this curve carries. Shared by both variants since
    /// `algo/exponential-anchor-placement`, so placement-aware code (reports,
    /// provenance) stays variant-agnostic like [`Self::consumes_reference`].
    /// `None` for [`Self::Characteristic`], which has no placement *rule* to state: it
    /// pins mid-grey wherever the stock's published curve puts it, so reporting one of the
    /// four rules here would name a knob the render never consulted. Deliberately an
    /// `Option` rather than a plausible-looking default — a report that names a number the
    /// render did not use is the exact defect PR #70 fixed.
    pub fn anchor(&self) -> Option<AnchorPlacement> {
        match self {
            DensityCurve::Exponential(e) => Some(e.anchor),
            DensityCurve::Sigmoid(s) => Some(s.anchor),
            DensityCurve::Characteristic(_) => None,
        }
    }
}

impl DensityCurveType {
    /// Every variant, so a hand-written enumeration can be checked against one list
    /// instead of growing its own. Added because two *migration* messages listed only
    /// `exponential` and `sigmoid` after the third curve shipped: the `--density-curve`
    /// parse diagnostic is generated by clap and stayed correct, but prose is not.
    ///
    /// **Test-only.** The runtime enumerates curves by exhaustive `match`, which is the
    /// stronger guarantee; a list is only useful for asserting that hand-written *prose*
    /// covers the set. `all_density_curve_types_are_listed` keeps the list itself honest.
    #[cfg(test)]
    pub const ALL: [DensityCurveType; 3] = [
        DensityCurveType::Exponential,
        DensityCurveType::Sigmoid,
        DensityCurveType::Characteristic,
    ];

    /// Whether a curve of this type reads [`CalibrationParams::dmax`] — the
    /// type-level twin of [`DensityCurve::consumes_reference`], for code that has
    /// only the discriminator.
    pub fn consumes_reference(self) -> bool {
        match self {
            Self::Exponential | Self::Sigmoid => true,
            Self::Characteristic => false,
        }
    }
}

/// Extract an optional field from a raw recipe object, distinguishing an **absent**
/// key (`Ok(None)`) from a **present** one (`Ok(Some(v))`) — including a present
/// explicit `null`, which is not a valid value for any of these typed fields and
/// so errors loudly rather than reading as "absent". A plain `Option<T>` struct
/// field cannot make this distinction (serde collapses JSON `null` to `None`), so
/// the tagged deserializers below capture the raw object and use key *presence* to
/// reject a cross-variant key or a null discriminator that would otherwise be
/// silently ignored / defaulted.
fn take_recipe_field<T: serde::de::DeserializeOwned>(
    obj: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> std::result::Result<Option<T>, String> {
    match obj.get(key) {
        Some(v) => serde_json::from_value(v.clone())
            .map(Some)
            .map_err(|e| e.to_string()),
        None => Ok(None),
    }
}

impl<'de> Deserialize<'de> for DensityCurve {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        // Capture the raw object so a cross-variant key is detected by *presence*,
        // not by its value: a plain `Option<f32>` field turns an explicit `null`
        // into `None`, indistinguishable from an absent key, and would silently
        // accept e.g. `{"type":"exponential","contrast":null}` instead of rejecting
        // the invalid tagged combination. `type` is still required — a
        // present-but-untagged curve object must not guess a variant.
        let value = serde_json::Value::deserialize(deserializer)?;
        let obj = value
            .as_object()
            .ok_or_else(|| D::Error::custom("reconstruction.curve must be a JSON object"))?;

        // Before the unknown-field scan, and before any per-variant cross-key rule:
        // `dmax` used to live here, so "unknown field" / "`dmax` is a parametric-curve
        // key" are both true and both useless. A recipe written against the old schema
        // needs the path it moved to (CLAUDE.md: diagnose the more specific fault
        // first). nc is unreleased — this is a migration error, never an alias.
        if obj.contains_key("dmax") {
            return Err(D::Error::custom(
                "`dmax` is no longer a `reconstruction.curve` key — the roll's reference \
                 density moved to the top-level `calibration` section, because it is a \
                 measurement of the roll rather than a parameter of the curve. Replace \
                 `\"curve\": {…, \"dmax\": {\"explicit\": <d>}}` with a top-level \
                 `\"calibration\": {\"dmax\": {\"explicit\": <d>}}` (also \"fixed\", \
                 \"auto\", \"none\"). The value and the render are unchanged; only the \
                 path moved. What stays in the curve is `anchor`, the rule that decides \
                 which tone the reference places. See design-spec §8.",
            ));
        }

        const KNOWN: [&str; 7] = [
            "type", "gamma", "contrast", "toe", "shoulder", "anchor", "stock",
        ];
        if let Some(k) = obj.keys().find(|k| !KNOWN.contains(&k.as_str())) {
            return Err(D::Error::custom(format!(
                "unknown field `{k}` in reconstruction.curve"
            )));
        }

        let curve_type: DensityCurveType = take_recipe_field(obj, "type")
            .map_err(D::Error::custom)?
            .ok_or_else(|| {
                D::Error::custom(
                    "reconstruction.curve is missing `type` (\"exponential\", \"sigmoid\" \
                     or \"characteristic\")",
                )
            })?;

        match curve_type {
            DensityCurveType::Exponential => {
                // `anchor` is deliberately absent from this list: since
                // `algo/exponential-anchor-placement` the placement rule is shared by both
                // *parametric* curves, so it is a cross-curve key rather than a sigmoid
                // one. (`characteristic` accepts neither, and rejects `anchor` by name.)
                if let Some(key) = ["contrast", "toe", "shoulder"]
                    .into_iter()
                    .find(|k| obj.contains_key(*k))
                {
                    return Err(D::Error::custom(format!(
                        "`{key}` is a sigmoid-curve key, but the curve type is \
                         \"exponential\" (its knobs are `gamma` and `anchor`)"
                    )));
                }
                if obj.contains_key("stock") {
                    return Err(D::Error::custom(
                        "`stock` is a characteristic-curve key, but the curve type is \
                         \"exponential\" — a film stock selects a *measured* response, so \
                         it cannot refine a parametric one",
                    ));
                }
                let d = ExponentialParams::default();
                Ok(DensityCurve::Exponential(ExponentialParams {
                    gamma: take_recipe_field(obj, "gamma")
                        .map_err(D::Error::custom)?
                        .unwrap_or(d.gamma),
                    anchor: take_recipe_field(obj, "anchor")
                        .map_err(D::Error::custom)?
                        .unwrap_or(d.anchor),
                }))
            }
            DensityCurveType::Sigmoid => {
                if obj.contains_key("gamma") {
                    return Err(D::Error::custom(
                        "`gamma` is an exponential-curve key, but the curve type is \
                         \"sigmoid\" (the mid-density slope analogue is `contrast`)",
                    ));
                }
                if obj.contains_key("stock") {
                    return Err(D::Error::custom(
                        "`stock` is a characteristic-curve key, but the curve type is \
                         \"sigmoid\" — a film stock selects a *measured* response, so it \
                         cannot refine a parametric one",
                    ));
                }
                let d = SigmoidParams::default();
                Ok(DensityCurve::Sigmoid(SigmoidParams {
                    contrast: take_recipe_field(obj, "contrast")
                        .map_err(D::Error::custom)?
                        .unwrap_or(d.contrast),
                    toe: take_recipe_field(obj, "toe")
                        .map_err(D::Error::custom)?
                        .unwrap_or(d.toe),
                    shoulder: take_recipe_field(obj, "shoulder")
                        .map_err(D::Error::custom)?
                        .unwrap_or(d.shoulder),
                    anchor: take_recipe_field(obj, "anchor")
                        .map_err(D::Error::custom)?
                        .unwrap_or(d.anchor),
                }))
            }
            DensityCurveType::Characteristic => {
                // Every parametric key is rejected here rather than ignored, and the
                // message says why: this curve has no slope or anchor to set because it
                // reads both off the published response. Accepting them silently would
                // let a recipe look like it were tuning a render it cannot touch.
                if let Some(key) = ["gamma", "contrast", "toe", "shoulder", "anchor"]
                    .into_iter()
                    .find(|k| obj.contains_key(*k))
                {
                    return Err(D::Error::custom(format!(
                        "`{key}` is a parametric-curve key, but the curve type is \
                         \"characteristic\", which reads its slope and its mid-grey \
                         placement off the stock's published curve. Its only key is \
                         `stock`; tone shaping belongs to `print.display_tone`"
                    )));
                }
                let d = CharacteristicParams::default();
                Ok(DensityCurve::Characteristic(CharacteristicParams {
                    stock: take_recipe_field(obj, "stock")
                        .map_err(D::Error::custom)?
                        .unwrap_or(d.stock),
                }))
            }
        }
    }
}

/// Wire schema version of the tagged `reconstruction` recipe/report object
/// (design-spec §8). Versions the **schema shape only** — it is not the
/// behavioral `pipeline_version` (owned by the `conversion-versioning` task,
/// bumped only when default pixels change). Every resolved recipe/report emits
/// it; partial input may omit it (defaults to this value); any other value is
/// rejected loudly.
pub const RECONSTRUCTION_SCHEMA_VERSION: u32 = 1;

/// The tagged reconstruction configuration (design-spec §8/§9, the recipe's one
/// `reconstruction` object): `simple` (direct inversion, no further knobs) or
/// `density` (density correction plus exactly one tagged [`DensityCurve`]).
///
/// One enum, not parallel section fields, so an illegal combination (simple
/// with a curve, two algorithms at once) is unrepresentable — the tagged-enum
/// convention `FilmBaseSource`/`DmaxSource` established, applied to the
/// algorithm selection itself. The custom serde keeps the documented wire shape
/// exactly: `schema_version` + `type` always emitted; `density`/`curve` emitted
/// for (and accepted only with) `type = "density"`; omitted sections fill their
/// defaults (an omitted `curve` normalizes to the tagged **sigmoid** defaults —
/// [`DensityCurve::default`] — so omission never survives into a resolved recipe or
/// report).
#[derive(Clone, Debug, PartialEq)]
pub enum Reconstruction {
    /// Channel-inversion baseline: the direct unclamped positive
    /// `1 − scan/Dmin`. No density or curve configuration — a **debugging**
    /// path that isolates decode plus film base, **not** the B&W one
    /// (`algo/bw-support` runs B&W through `density`). It is affine in
    /// transmission, where `density` is a power law in it, so the two are
    /// different curve shapes rather than different tunings of one.
    Simple,
    /// Density-domain reconstruction (the default): corrected density `D′`
    /// (stages 1–2, `density`) mapped through the tagged `curve`.
    Density {
        density: DensityParams,
        curve: DensityCurve,
    },
}

impl Default for Reconstruction {
    fn default() -> Self {
        Reconstruction::Density {
            density: DensityParams::default(),
            curve: DensityCurve::default(),
        }
    }
}

impl Reconstruction {
    /// This configuration's `type` discriminator (wire tag / CLI value).
    pub fn reconstruction_type(&self) -> ReconstructionType {
        match self {
            Reconstruction::Simple => ReconstructionType::Simple,
            Reconstruction::Density { .. } => ReconstructionType::Density,
        }
    }

    /// The resolved curve's type, when this is a density reconstruction
    /// (`None` for `simple` — it has no curve stage).
    pub fn curve_type(&self) -> Option<DensityCurveType> {
        match self {
            Reconstruction::Simple => None,
            Reconstruction::Density { curve, .. } => Some(curve.curve_type()),
        }
    }
}

impl Serialize for Reconstruction {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        match self {
            Reconstruction::Simple => {
                let mut st = serializer.serialize_struct("Reconstruction", 2)?;
                st.serialize_field("schema_version", &RECONSTRUCTION_SCHEMA_VERSION)?;
                st.serialize_field("type", "simple")?;
                st.end()
            }
            Reconstruction::Density { density, curve } => {
                let mut st = serializer.serialize_struct("Reconstruction", 4)?;
                st.serialize_field("schema_version", &RECONSTRUCTION_SCHEMA_VERSION)?;
                st.serialize_field("type", "density")?;
                st.serialize_field("density", density)?;
                st.serialize_field("curve", curve)?;
                st.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Reconstruction {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        // Capture the raw object so `type` presence is distinguished from a present
        // explicit `null`: with `Option<ReconstructionType>`, `{"type":null}` would
        // collapse to `None` and silently default to density, and `type:"simple"`
        // would accept a null `density`/`curve` section. Presence-checking rejects
        // those malformed recipes loudly instead.
        let value = serde_json::Value::deserialize(deserializer)?;
        let obj = value
            .as_object()
            .ok_or_else(|| D::Error::custom("reconstruction must be a JSON object"))?;

        const KNOWN: [&str; 4] = ["schema_version", "type", "density", "curve"];
        if let Some(k) = obj.keys().find(|k| !KNOWN.contains(&k.as_str())) {
            return Err(D::Error::custom(format!(
                "unknown field `{k}` in reconstruction"
            )));
        }

        if let Some(v) =
            take_recipe_field::<u32>(obj, "schema_version").map_err(D::Error::custom)?
            && v != RECONSTRUCTION_SCHEMA_VERSION
        {
            return Err(D::Error::custom(format!(
                "unsupported reconstruction.schema_version {v} \
                 (this build reads schema_version {RECONSTRUCTION_SCHEMA_VERSION})"
            )));
        }

        // `type` present-but-null errors here (null is not a valid tag); absent
        // defaults to density.
        let reconstruction_type: ReconstructionType = take_recipe_field(obj, "type")
            .map_err(D::Error::custom)?
            .unwrap_or_default();
        match reconstruction_type {
            ReconstructionType::Simple => {
                if let Some(key) = ["density", "curve"]
                    .into_iter()
                    .find(|k| obj.contains_key(*k))
                {
                    return Err(D::Error::custom(format!(
                        "reconstruction.type = \"simple\" takes no `{key}` section — \
                         density correction and density curves belong to \
                         `type = \"density\"`"
                    )));
                }
                Ok(Reconstruction::Simple)
            }
            ReconstructionType::Density => {
                // `density` must be a JSON object, for the same reason `curve` must be
                // (see `DensityCurve`'s deserializer): the `scale` probe below reads the
                // raw value. `DensityParams` is a plain derive, so serde would also
                // accept a positional array — which deserializes *successfully* while
                // `as_object()` returns `None`, so the probe would read "not stated" and
                // overwrite the array's stated gain with the per-curve default. Reject
                // the shape here rather than guessing at it.
                if let Some(v) = obj.get("density")
                    && !v.is_object()
                {
                    return Err(D::Error::custom(
                        "reconstruction.density must be a JSON object",
                    ));
                }
                // Whether `scale` was *stated* has to be read off the raw object: it is a
                // concrete `[f32; 3]` in `DensityParams`, so by the time serde has built
                // one, an omitted key and an explicit `[1, 1, 1]` are the same value. The
                // difference matters — the default is per-curve, and an explicit gain must
                // survive under any curve.
                let stated_scale = obj
                    .get("density")
                    .and_then(serde_json::Value::as_object)
                    .is_some_and(|d| d.contains_key("scale"));
                let mut density: DensityParams = take_recipe_field(obj, "density")
                    .map_err(D::Error::custom)?
                    .unwrap_or_default();
                // An omitted curve normalizes to tagged sigmoid defaults —
                // omission never survives into a resolved recipe.
                let curve: DensityCurve = take_recipe_field(obj, "curve")
                    .map_err(D::Error::custom)?
                    .unwrap_or_default();
                if !stated_scale {
                    density.scale = DensityParams::default_scale_for(curve.curve_type());
                }
                Ok(Reconstruction::Density { density, curve })
            }
        }
    }
}

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

/// Named output preset (design-spec §5/§9, `output.preset` /
/// `--output-preset`) — the atomic output *policy* choice: which branch the
/// render takes out of the NC film RGB v1 ACEScg boundary, and the container /
/// depth / profile that branch resolves.
///
/// One mutually-exclusive enum field, like [`FilmBaseSource`] / [`DmaxSource`]:
/// a preset resolves a whole coherent policy, so it can never be a bag of
/// independent bools. Serializes kebab-case (`"gain-map-hdr"` / `"film-master"`).
///
/// **Ten variants are accepted** — `film-master`, `gain-map-hdr`, `ultra-hdr-v1`,
/// `display-p3`, `compatibility`, `hdr-pq`, `hdr-hlg`, `hdr-linear-tiff`,
/// `hdr-pq-tiff` and `hdr-hlg-tiff`, enumerated once in [`ALL`](Self::ALL). Every
/// one is atomic: it resolves container, depth and profile itself.
/// [`parse`](Self::parse) rejects the removed `legacy` and `custom` and the
/// pre-release `scene-master` with migration messages rather than a generic
/// unknown-value error.
///
/// Keep this list in step with `parse`, [`ALL`](Self::ALL), and
/// `OutputOverrides::output_preset`'s help text — it has gone stale twice, and the
/// help text is what `--help` shows. The diagnostics no longer restate it: they are
/// generated from `ALL`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum OutputPreset {
    /// **`film-master`** — an unclamped 32-bit float linear ACEScg TIFF taken
    /// **directly** from the NC film RGB v1 mapping. It preserves the intentional
    /// film, lens, development, scanner, reconstruction, and density-curve
    /// rendering (including supported fixed/roll `Dmax` placement) and bypasses
    /// every later white-balance, exposure, black/range-placement, highlight,
    /// display tone, gamut, and transfer operation. It is **not** a physical
    /// scene-linear recovery.
    ///
    /// The bypass is strict, not silent: `cli::validate` rejects frame-local
    /// `auto` `Dmax` and every non-default downstream control, whatever their
    /// source. A linear float export that *wants* the print and display controls
    /// applied is [`HdrLinearTiff`](Self::HdrLinearTiff).
    FilmMaster,
    /// **`ultra-hdr-v1`** — an explicitly legacy Ultra HDR v1 JPEG: an
    /// SDR Display P3 base image plus a luminance gain-map JPEG and XMP/MPF
    /// metadata. This name deliberately does not claim ISO 21496-1 conformance.
    ///
    /// Retained as the *compatibility* gain-map output beside
    /// [`GainMapHdr`](Self::GainMapHdr), not superseded by it: this is the file to
    /// write for a decoder that reads only Google's dialect. Its bytes are frozen —
    /// a test asserts they contain no `21496`.
    ///
    /// **It is not HDR on Apple platforms** (measured 2026-08-06): ImageIO ignores
    /// the legacy XMP entirely and opens the file as an ordinary SDR JPEG.
    UltraHdrV1,
    /// **`gain-map-hdr`** — the same gain-map JPEG carrying **both** metadata
    /// dialects: Google's legacy Ultra HDR v1 XMP/MPF *and* ISO 21496-1 segments in
    /// both images, describing the one shared luminance gain map
    /// (`io::ultra_hdr::Dialects::LegacyPlusIso`).
    ///
    /// Dual-dialect is the whole point of the name rather than a refinement of
    /// [`UltraHdrV1`](Self::UltraHdrV1): Apple reads only the ISO dialect and
    /// Android 15+ reads both, so this is the one gain-map file that is actually
    /// HDR on Apple platforms. The pixels are identical to `ultra-hdr-v1`'s — the
    /// two presets differ **only** in the metadata segments attached.
    ///
    /// ISO 21496-1 is silent on coexistence with the legacy XMP, so which dialect a
    /// dual-aware decoder prefers is *observed behaviour*, never a conformance
    /// claim.
    ///
    /// **The product default** since `pipeline_version` 3. Being a named preset it is
    /// atomic and requires a `.jpg`/`.jpeg` output path — so `hanten convert -o out.tif`
    /// with no preset is now a usage error naming the accepted suffixes, where it
    /// previously wrote a 16-bit TIFF. That is the documented cost of the migration,
    /// not an oversight: an unnamed output policy silently changing container would
    /// be worse.
    #[default]
    GainMapHdr,
    /// **`hdr-pq`** — a single-rendition 10-bit 4:4:4 AVIF carrying Rec.2100 PQ
    /// (CICP 9/16/9, full range) with a 203 cd/m² reference white and 1000 cd/m²
    /// mastering peak. Written by `io::avif`; requires an `.avif` output path.
    HdrPq,
    /// **`hdr-hlg`** — the same container and coding as [`HdrPq`](Self::HdrPq) but
    /// Rec.2100 HLG (CICP 9/18/9) with the reference 1000-nit, zero-black OOTF at
    /// system gamma 1.2. Being display-referred, it carries no absolute
    /// content-light metadata.
    HdrHlg,
    /// **`hdr-linear-tiff`** — the display-linear HDR *interchange* master: an
    /// unclamped 32-bit float TIFF holding the HDR renderer's pre-transfer
    /// BT.2020/D65 samples verbatim, with a synthesized linear-BT.2020 ICC
    /// profile. Requires a `.tif`/`.tiff` output path.
    ///
    /// Distinct from all three neighbours, and the distinctions are the point:
    /// - **not** [`FilmMaster`](Self::FilmMaster) — that is linear ACEScg *before*
    ///   any display rendering, whereas this has been through the shared print
    ///   controls, the reference-white-preserving shoulder, and BT.2020 gamut
    ///   mapping;
    /// - **not** [`HdrPq`](Self::HdrPq)/[`HdrHlg`](Self::HdrHlg) — no transfer
    ///   function has been applied, so these are linear luminance values, not
    ///   Rec.2100 code values.
    ///
    /// Samples are reference-white-relative: `1.0` is the 203 cd/m² reference
    /// white and highlights legitimately reach the 1000 cd/m² peak at
    /// `pipeline::hdr::LINEAR_HEADROOM` (≈4.926108). Nothing is clamped, so the
    /// embedded ICC — whose PCS stops at 1.0 — cannot by itself convey those
    /// luminance semantics; the report and sidecar are authoritative for them.
    HdrLinearTiff,
    /// **`hdr-pq-tiff`** — the Rec.2100 PQ signal stored as full-range 16-bit TIFF
    /// code values, with an extended-range BT.2020 ICC profile carrying the
    /// `cicp` 9-16-0-1 tag. Requires a `.tif`/`.tiff` output path.
    ///
    /// Lossless *relative to the quantized signal*: the renderer's normalized
    /// output is quantized once with one pinned rounding rule and TIFF stores every
    /// resulting code exactly, with the measured max/RMS quantization error
    /// reported. **16 bits is TIFF's quantization, not one of BT.2100's own bit
    /// depths** (it specifies 10 and 12), so the file carries BT.2100's transfer
    /// function at TIFF's precision — the report says exactly that rather than
    /// implying a Rec.2100 system claim.
    HdrPqTiff,
    /// **`hdr-hlg-tiff`** — as [`HdrPqTiff`](Self::HdrPqTiff) but the HLG transfer,
    /// with `cicp` 9-18-0-1. Its ICC profile is deliberately **scene-referred**
    /// (HLG's OOTF is not per-channel separable, so no 1D curve set can express
    /// it); the display-referred contract lives in the report.
    HdrHlgTiff,
    /// **`display-p3`** — a 16-bit integer SDR TIFF in Display P3, rendered through
    /// the modern display stage (NC film RGB v1 → linear ACEScg → the shared print
    /// controls → `pipeline::sdr`, including its reference-white-preserving
    /// shoulder and gamut mapping). Requires `.tif`/`.tiff`.
    DisplayP3,
    /// **`compatibility`** — the same modern SDR render as
    /// [`DisplayP3`](Self::DisplayP3), in **sRGB**: the widest-support output nc
    /// writes, and lossless (16-bit integer, no lossy codec). Requires
    /// `.tif`/`.tiff`.
    Compatibility,
}

impl OutputPreset {
    /// Parse the `--output-preset` value / `output.preset` recipe key. Shared by
    /// the CLI merge and the custom [`Deserialize`] below so a name gets the same
    /// diagnosis wherever it appears (the `OutputSpace::parse` precedent).
    pub fn parse(s: &str) -> Result<Self> {
        // Case-insensitive like `OutputSpace::parse`: these are keywords, not paths.
        match s.trim().to_ascii_lowercase().as_str() {
            "film-master" => Ok(OutputPreset::FilmMaster),
            "ultra-hdr-v1" => Ok(OutputPreset::UltraHdrV1),
            "gain-map-hdr" => Ok(OutputPreset::GainMapHdr),
            "hdr-pq" => Ok(OutputPreset::HdrPq),
            "hdr-hlg" => Ok(OutputPreset::HdrHlg),
            "hdr-linear-tiff" => Ok(OutputPreset::HdrLinearTiff),
            "hdr-pq-tiff" => Ok(OutputPreset::HdrPqTiff),
            "hdr-hlg-tiff" => Ok(OutputPreset::HdrHlgTiff),
            "display-p3" => Ok(OutputPreset::DisplayP3),
            "compatibility" => Ok(OutputPreset::Compatibility),
            // The pre-release name for the same branch. It was renamed *before*
            // release because "scene" wrongly implied physical scene-linear
            // recovery; nc is unreleased, so this is a schema break, not an alias.
            "scene-master" => Err(NcError::Usage(
                "output preset `scene-master` does not exist — it was renamed \
                 `film-master` before release (the master carries NC's intentional \
                 film/lens/development/scanner rendering, not a physical \
                 scene-linear recovery). Use `film-master`; there is no alias."
                    .into(),
            )),
            // Retired with the legacy print path (`nf-retire/legacy-custom`). nc is
            // unreleased, so a recipe naming either is a schema break, not an alias.
            removed @ ("legacy" | "custom") => Err(NcError::Usage(format!(
                "output preset `{removed}` was removed together with the legacy print \
                 path{} — for a 16-bit TIFF use `display-p3` (or `compatibility` for \
                 sRGB); for a float TIFF, `film-master` (linear ACEScg before display \
                 rendering) or `hdr-linear-tiff` (display-linear BT.2020). The old \
                 rendering is reproducible only from the reference build \
                 (`scripts/reference-snapshot/`). There is no alias.",
                if removed == "custom" {
                    " and the depth/profile/container selectors it existed to accept"
                } else {
                    ""
                }
            ))),
            other => Err(NcError::Usage(format!(
                "unknown output preset `{other}` — accepted: {}",
                Self::accepted_list()
            ))),
        }
    }

    /// Whether this preset's render can apply the **extended-Reinhard** display tone.
    ///
    /// Narrower than "is a display preset" in principle: the other two tones are
    /// bounded and every display preset takes them, while this one deliberately
    /// overshoots.
    ///
    /// Exhaustive on purpose — a new preset must state its answer rather than inherit
    /// one. Deriving it from another property is the trap `cli::required_extensions`
    /// already fell into, where "pins a suffix" was read as "convert-only" and refused
    /// every preset.
    ///
    /// **Every display preset now answers `true`**, which is the end state rather than a
    /// reason to delete the predicate — see `cli::validate_output_preset`'s rule 2, whose
    /// job is to refuse a *future* preset that answers `false` instead of letting it render
    /// a tone its branch cannot carry.
    ///
    /// Both original exclusions were lifted on 2026-09-02 and neither by relaxing a check.
    /// The HDR presets waited on the ceiling-parameterized form, which
    /// `display_tone::highlight_lifted_reinhard` supplies: a lift over an **asymptotic**
    /// base, so the composite stays strictly inside the declared 1000-nit peak. The
    /// gain-map pair waited on `gain_map::build` ratioing against `min(sdr, 1)` — the base
    /// as *stored*, which is what a decoder multiplies — since an unbounded SDR half had
    /// stored a gain short by whatever the encode clamped.
    pub fn accepts_reinhard_tone(self) -> bool {
        match self {
            // The two SDR presets, and every **single-rendition** HDR preset: the HDR form
            // was derived and measured on 2026-09-02, and its asymptotic base holds the
            // composite strictly inside the declared 1000-nit peak on all seven fixture
            // frames (4.912–4.919 against 4.926).
            OutputPreset::DisplayP3
            | OutputPreset::Compatibility
            | OutputPreset::HdrPq
            | OutputPreset::HdrHlg
            | OutputPreset::HdrLinearTiff
            | OutputPreset::HdrPqTiff
            | OutputPreset::HdrHlgTiff => true,
            // The gain-map pair, admitted once `gain_map::build` began ratioing against
            // `min(sdr, 1)` — the base as *stored*, which is what a decoder multiplies.
            // Before that the unbounded SDR half stored a gain short by whatever the encode
            // clamped, reconstructing up to 23% dark. The fix was the ratio, never a
            // relaxed check here.
            OutputPreset::GainMapHdr | OutputPreset::UltraHdrV1 => true,
            // No display tone stage at all.
            OutputPreset::FilmMaster => false,
        }
    }

    /// Whether this preset's render applies `print.display_tone` at all.
    ///
    /// Distinct from [`Self::accepts_reinhard_tone`], which asks whether *one* operator is
    /// carryable: this asks whether the display stage runs. They happen to partition the
    /// same way today, and deriving one from the other is exactly the trap
    /// `cli::required_extensions` fell into when "pins a suffix" was read as
    /// "convert-only" — so this is its own exhaustive match and a new preset states its
    /// answer rather than inheriting one.
    ///
    /// `cli`'s conversion-preset rule reads it: every `--preset` bundle sets a display
    /// tone, so pairing one with a branch that answers `false` is a contradiction to
    /// diagnose once, not three refusals to disassemble in sequence.
    pub fn applies_display_tone(self) -> bool {
        match self {
            OutputPreset::DisplayP3
            | OutputPreset::Compatibility
            | OutputPreset::HdrPq
            | OutputPreset::HdrHlg
            | OutputPreset::HdrLinearTiff
            | OutputPreset::HdrPqTiff
            | OutputPreset::HdrHlgTiff
            | OutputPreset::GainMapHdr
            | OutputPreset::UltraHdrV1 => true,
            // The master bypass has no display stage.
            OutputPreset::FilmMaster => false,
        }
    }

    /// Every preset this build accepts, in help order.
    ///
    /// Diagnostics are generated from this list rather than restating it, because
    /// two hand-written "accepted: …" lists both went stale the moment a preset
    /// shipped — and a stale list hides exactly the name the user was reaching for.
    pub const ALL: [OutputPreset; 10] = [
        OutputPreset::FilmMaster,
        OutputPreset::GainMapHdr,
        OutputPreset::UltraHdrV1,
        OutputPreset::DisplayP3,
        OutputPreset::Compatibility,
        OutputPreset::HdrPq,
        OutputPreset::HdrHlg,
        OutputPreset::HdrLinearTiff,
        OutputPreset::HdrPqTiff,
        OutputPreset::HdrHlgTiff,
    ];

    /// The accepted names as a comma-separated backticked list, for diagnostics.
    fn accepted_list() -> String {
        Self::ALL
            .iter()
            .map(|p| format!("`{}`", p.name()))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The preset's stable wire / CLI name — the same string [`parse`](Self::parse)
    /// accepts and `Serialize` emits. Diagnostics take the name from here rather
    /// than hardcoding a literal, so a message about the *next* named preset can
    /// never end up describing `film-master`.
    pub fn name(self) -> &'static str {
        match self {
            OutputPreset::FilmMaster => "film-master",
            OutputPreset::UltraHdrV1 => "ultra-hdr-v1",
            OutputPreset::GainMapHdr => "gain-map-hdr",
            OutputPreset::HdrPq => "hdr-pq",
            OutputPreset::HdrHlg => "hdr-hlg",
            OutputPreset::HdrLinearTiff => "hdr-linear-tiff",
            OutputPreset::HdrPqTiff => "hdr-pq-tiff",
            OutputPreset::HdrHlgTiff => "hdr-hlg-tiff",
            OutputPreset::DisplayP3 => "display-p3",
            OutputPreset::Compatibility => "compatibility",
        }
    }
}

impl<'de> Deserialize<'de> for OutputPreset {
    /// Delegates to [`OutputPreset::parse`], so a recipe's `output.preset` gets the
    /// same pinned migration / not-yet-accepted diagnostics as the CLI flag
    /// (serde's derived enum error would only list the two accepted variants).
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;
        let s = String::deserialize(deserializer)?;
        OutputPreset::parse(&s).map_err(|e| D::Error::custom(e.to_string()))
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

/// Output / encode knobs (design-spec §9, stage 5).
///
/// Only the preset: every preset is atomic, so container, depth and profile are
/// *resolved* from it ([`depth`](Self::depth)) rather than stated. The
/// `output.depth` / `output.output_profile` / `output.bigtiff` selectors retired with
/// the `legacy` and `custom` presets, and `cli::reject_legacy_recipe_keys` names that
/// migration for a recipe still carrying one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct OutputParams {
    /// Named output preset (default `gain-map-hdr`). Selects the branch out of the
    /// ACEScg boundary and the container/depth/profile policy.
    pub preset: OutputPreset,
}

impl OutputParams {
    /// The encoder bit depth this output resolves to. The single place a recipe
    /// value becomes a depth, so encode, IR export, and color can't disagree:
    ///
    /// - `film-master` is **always** [`OutDepth::F32`] — the master is unclamped
    ///   float linear ACEScg by definition, not by an `output.depth` value (which
    ///   must stay at its default under the preset).
    /// - the gain-map presets resolve [`OutDepth::U16`] only for optional IR TIFF
    ///   export; their primary image is fixed 8-bit JPEG.
    /// - `hdr-pq` / `hdr-hlg` likewise resolve [`OutDepth::U16`] only for the IR
    ///   TIFF; their primary image is fixed 10-bit AVIF.
    pub fn depth(&self) -> OutDepth {
        match self.preset {
            // Both are unclamped 32-bit float TIFFs.
            OutputPreset::FilmMaster | OutputPreset::HdrLinearTiff => OutDepth::F32,
            // Used only by the optional IR TIFF export. The primary image's depth
            // is fixed by the preset — 8-bit JPEG, or 10-bit AVIF.
            // `hdr-*-tiff` resolves u16 for the primary *and* the optional IR
            // plane; the AVIF/JPEG presets only use it for IR.
            OutputPreset::UltraHdrV1
            | OutputPreset::GainMapHdr
            | OutputPreset::HdrPq
            | OutputPreset::HdrHlg
            | OutputPreset::HdrPqTiff
            | OutputPreset::HdrHlgTiff
            // 16-bit integer for the primary image and any IR plane: "losslessly
            // stored SDR" is the point, and a float SDR TIFF is precision nothing
            // can display.
            | OutputPreset::DisplayP3
            | OutputPreset::Compatibility => OutDepth::U16,
        }
    }

    /// The **primary image's** bit depth, as a label for the telemetry record.
    ///
    /// Deliberately *not* [`depth`](Self::depth), which answers a different question:
    /// for the JPEG and AVIF presets that value is only the optional IR *TIFF*'s
    /// depth, so recording it verbatim labelled a gain-map run `u16` when its primary
    /// is a fixed 8-bit JPEG. The container fixes these, so they are constants rather
    /// than anything resolved.
    pub fn primary_depth_label(&self) -> &'static str {
        match self.preset {
            // Fixed by the container.
            OutputPreset::GainMapHdr | OutputPreset::UltraHdrV1 => "u8",
            OutputPreset::HdrPq | OutputPreset::HdrHlg => "u10",
            // TIFF presets: the primary really is what `depth()` resolves.
            OutputPreset::FilmMaster
            | OutputPreset::DisplayP3
            | OutputPreset::Compatibility
            | OutputPreset::HdrLinearTiff
            | OutputPreset::HdrPqTiff
            | OutputPreset::HdrHlgTiff => match self.depth() {
                OutDepth::U16 => "u16",
                OutDepth::F32 => "f32",
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_tone_wire_form_leaves_room_for_a_parameterized_operator() {
        // Both variants are bare strings, which is what makes a future operator that
        // *carries* parameters (`output/display-tone-mapping`'s Reinhard white point)
        // a pure addition rather than a migration: it would serialize as
        // `{"reinhard": {…}}` while these two keep the spellings every stored recipe
        // and sidecar already contains.
        assert_eq!(
            serde_json::to_string(&DisplayToneCurve::Shoulder).unwrap(),
            r#""shoulder""#
        );
        assert_eq!(
            serde_json::to_string(&DisplayToneCurve::None).unwrap(),
            r#""none""#
        );
        assert_eq!(
            serde_json::from_str::<DisplayToneCurve>(r#""none""#).unwrap(),
            DisplayToneCurve::None
        );
        // Pinned because the encoding that would break them is one attribute away:
        // adding `#[serde(tag = "type")]` respells both as `{"type": …}` objects, and
        // nothing else in the suite would notice — including in the default recipe
        // document that every sidecar and the `recipe` fingerprint are built from.
        let document = serde_json::to_string(&PrintParams::default()).unwrap();
        assert!(
            document.contains(r#""display_tone":"shoulder""#),
            "default recipe document changed shape: {document}"
        );
    }

    #[test]
    fn display_tone_display_impl_matches_its_serde_spelling() {
        // `Display` is what the two validation messages interpolate, and the serde
        // form is what the recipe parser accepts. Nothing but this test ties them
        // together, so a variant added to one and not the other would tell a user to
        // type a spelling their recipe rejects. Driven off `value_variants` so a new
        // variant is covered without editing the test.
        //
        // Driven off `NAMES` rather than `clap::ValueEnum`, which the parameterized
        // variant removed — `DisplayToneCurve::parse` is now the flag's third spelling
        // and this is what ties it to the other two.
        //
        // The invariant, stated once: for every accepted name, `parse` yields a variant
        // whose `Display` is that same name, and whose serde form is either that bare
        // string (unit variants) or an object keyed by it (parameterized ones).
        //
        // **The bare-string round trip is asserted for every name, outside the match.**
        // It used to sit inside the `Value::String` arm, so the parameterized variant
        // skipped it — and skipping it is exactly how `Display` came to hand users
        // `reinhard`, a spelling the derived `Deserialize` then rejected with
        // "invalid type: unit variant". That is the whole property this test exists for,
        // so it cannot be conditional on the wire shape.
        for name in DisplayToneCurve::NAMES {
            let variant = DisplayToneCurve::parse(name)
                .unwrap_or_else(|e| panic!("`{name}` is in NAMES but does not parse: {e}"));
            assert_eq!(variant.to_string(), name, "{variant:?}: Display");
            assert_eq!(
                serde_json::from_str::<DisplayToneCurve>(&format!("\"{name}\"")).unwrap_or_else(
                    |e| panic!(
                        "`{name}` is handed to users by Display but the recipe parser \
                         rejects it: {e}"
                    )
                ),
                variant,
                "{variant:?}: the bare name must parse back to what `parse` produced"
            );
            let wire = serde_json::to_value(variant).unwrap();
            match &wire {
                serde_json::Value::String(spelled) => {
                    assert_eq!(spelled, name, "{variant:?}: bare-string wire form");
                }
                serde_json::Value::Object(map) => {
                    // Externally tagged, so the object's single key is the same name.
                    // This is what kept the two unit variants' recipes parsing.
                    assert_eq!(
                        map.keys().collect::<Vec<_>>(),
                        vec![name],
                        "{variant:?}: externally-tagged key"
                    );
                    assert_eq!(
                        serde_json::from_value::<DisplayToneCurve>(wire.clone()).unwrap(),
                        variant
                    );
                    // ...and the empty payload means "this operator at its documented
                    // default", so the three spellings of that are interchangeable.
                    assert_eq!(
                        serde_json::from_str::<DisplayToneCurve>(&format!("{{\"{name}\":{{}}}}"))
                            .unwrap(),
                        variant,
                        "{variant:?}: `{{\"{name}\": {{}}}}` must resolve the default"
                    );
                }
                other => panic!("{variant:?}: unexpected wire form {other}"),
            }
        }
    }

    /// The wire form the parameterized variant actually took, pinned because the whole
    /// "pure addition" claim rests on it: the two unit variants keep bare strings, and
    /// only the new operator needs an object.
    #[test]
    fn the_parameterized_display_tone_is_an_externally_tagged_addition() {
        assert_eq!(
            serde_json::to_string(&DisplayToneCurve::Reinhard {
                headroom_stops: 6.0
            })
            .unwrap(),
            r#"{"reinhard":{"headroom_stops":6.0}}"#
        );
        // A recipe written before this variant existed still parses unchanged.
        assert_eq!(
            serde_json::from_str::<DisplayToneCurve>(r#""shoulder""#).unwrap(),
            DisplayToneCurve::Shoulder
        );
        assert_eq!(
            serde_json::from_str::<DisplayToneCurve>(r#""none""#).unwrap(),
            DisplayToneCurve::None
        );
        // `2^stops`, and zero stops is the identity white point.
        assert_eq!(DisplayToneCurve::Shoulder.white_point(), None);
        assert_eq!(DisplayToneCurve::None.white_point(), None);
        for (stops, w) in [(0.0, 1.0), (4.0, 16.0), (6.0, 64.0), (8.0, 256.0)] {
            assert_eq!(
                DisplayToneCurve::Reinhard {
                    headroom_stops: stops
                }
                .white_point(),
                Some(w)
            );
        }
    }

    #[test]
    fn the_reinhard_payload_denies_unknown_fields_like_every_other_recipe_object() {
        // Project convention: every recipe struct denies unknown fields, so a mistyped
        // key is loud instead of silently ignored. A struct *variant* needs
        // `deny_unknown_fields` on the enum to inherit it, which this was missing — the
        // one recipe object in the crate where `{"reinhard": {"bogus_key": 1, …}}`
        // converted at exit 0 while `{"print": {"bogus_key": 1}}` was an error.
        let err = serde_json::from_str::<DisplayToneCurve>(
            r#"{"reinhard":{"headroom_stops":6.0,"bogus_key":1}}"#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("unknown field `bogus_key`"), "{err}");
        // Falsifiable, and the *reason* the field is defaulted rather than required: the
        // same object without the stray key parses, and the payload really is optional —
        // an empty one, and the bare name, both resolve the documented default. Asserted
        // against `DEFAULT_HEADROOM_STOPS`, not the literal 6, so moving the default
        // cannot leave this test pinning the old number.
        for spelling in [
            r#"{"reinhard":{"headroom_stops":6.0}}"#,
            r#"{"reinhard":{}}"#,
            r#""reinhard""#,
        ] {
            assert_eq!(
                serde_json::from_str::<DisplayToneCurve>(spelling).unwrap(),
                DisplayToneCurve::Reinhard {
                    headroom_stops: DEFAULT_HEADROOM_STOPS
                },
                "{spelling}"
            );
        }
        assert_eq!(DEFAULT_HEADROOM_STOPS, 6.0);
        // The whole recipe path, not just the leaf type — that is where a user meets it.
        let err = serde_json::from_str::<crate::cli::ResolvedConfig>(
            r#"{"print":{"display_tone":{"reinhard":{"headroom_stops":6.0,"bogus_key":1}}}}"#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("unknown field `bogus_key`"), "{err}");
    }

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
    fn density_params_json_round_trip() {
        let params = DensityParams {
            scale: [1.2, 1.0, 0.8],
            offset: [0.1, 0.0, -0.05],
            shadow_balance: [0.05, 0.0, -0.02],
            highlight_balance: [-0.05, 0.01, 0.0],
            balance_range: BalanceRange::Explicit([0.25, 1.75]),
        };
        let json = serde_json::to_string(&params).unwrap();
        let back: DensityParams = serde_json::from_str(&json).unwrap();
        assert_eq!(params, back);
    }

    #[test]
    fn dmax_source_serializes_like_film_base_source() {
        // Unit variants are bare lowercase strings; the newtype variant is a
        // tagged object — the same shape convention as `FilmBaseSource`.
        assert_eq!(
            serde_json::to_string(&DmaxSource::Fixed).unwrap(),
            "\"fixed\""
        );
        assert_eq!(
            serde_json::to_string(&DmaxSource::Auto).unwrap(),
            "\"auto\""
        );
        assert_eq!(
            serde_json::to_string(&DmaxSource::None).unwrap(),
            "\"none\""
        );
        assert_eq!(
            serde_json::to_string(&DmaxSource::Explicit(1.5)).unwrap(),
            r#"{"explicit":1.5}"#
        );
        for src in [
            DmaxSource::Fixed,
            DmaxSource::Auto,
            DmaxSource::None,
            DmaxSource::Explicit(2.25),
        ] {
            let json = serde_json::to_string(&src).unwrap();
            assert_eq!(serde_json::from_str::<DmaxSource>(&json).unwrap(), src);
        }
    }

    #[test]
    fn film_base_source_serializes_all_variants() {
        // Unit variants are bare lowercase strings; data variants are tagged
        // objects.
        assert_eq!(
            serde_json::to_string(&FilmBaseSource::Auto).unwrap(),
            "\"auto\""
        );
        for src in [
            FilmBaseSource::Auto,
            FilmBaseSource::Region([1, 2, 3, 4]),
            FilmBaseSource::Explicit([0.9, 0.5, 0.4]),
        ] {
            let json = serde_json::to_string(&src).unwrap();
            assert_eq!(serde_json::from_str::<FilmBaseSource>(&json).unwrap(), src);
        }
    }

    #[test]
    fn calibration_default_dmax_is_fixed() {
        // The default reference is the roll-fixed nominal `Fixed`, not the demoted
        // per-frame `Auto` (dmax-reference): the faithful-conversion default must
        // not normalize exposure per frame. It is one default for the roll now, not
        // one per curve — `core/calibration-recipe-section` moved it out of the curve.
        assert_eq!(CalibrationParams::default().dmax, DmaxSource::Fixed);
        assert_eq!(DmaxSource::default(), DmaxSource::Fixed);
        // Which curves *read* it is a separate question, and the one the curve still
        // answers.
        assert!(DensityCurve::default().consumes_reference());
        assert!(!DensityCurveType::Characteristic.consumes_reference());
    }

    #[test]
    fn every_preset_resolves_its_own_depth() {
        // No knob states a depth any more: the two float TIFFs resolve f32 and every
        // other preset u16 (for its primary, or only for the optional IR TIFF).
        for preset in OutputPreset::ALL {
            let want = match preset {
                OutputPreset::FilmMaster | OutputPreset::HdrLinearTiff => OutDepth::F32,
                _ => OutDepth::U16,
            };
            assert_eq!(OutputParams { preset }.depth(), want, "{}", preset.name());
        }
    }

    /// The anchor arithmetic for each placement, including the values the 2026-08-03
    /// candidate retest recorded — floor 0.002 → 1.349 and 0.005 → 1.151 at contrast 2.0.
    /// Those two are why candidate 5's first rejection was overturned: the original run
    /// paired a floor with an inconsistent anchor (1.607, above every roll's Dmax), so
    /// nothing reached white and the frame rendered dark.
    #[test]
    fn anchor_placement_resolves_each_rule() {
        let c = 2.0f32;
        let r = 1.3f32;
        assert_eq!(AnchorPlacement::WhiteAtDmax.anchor(r, c), r);
        // Mid-grey `f` of the way up the reference, plus 0.745/contrast.
        let mid = AnchorPlacement::MidAtDmaxFraction(0.5).anchor(r, c);
        assert!(
            (mid - (0.5 * r + MID_GREY_OUTPUT_DECADES / c)).abs() < 1e-6,
            "{mid}"
        );
        // Base-derived rules ignore the reference entirely — the property that removes
        // the roll-to-roll term. Passing a wildly different reference must not move them.
        for placement in [
            AnchorPlacement::BlackAtBase(0.005),
            AnchorPlacement::MidAtBaseOffset(0.5),
        ] {
            assert_eq!(
                placement.anchor(r, c),
                placement.anchor(r + 0.295, c),
                "{placement:?} moved with the reference"
            );
        }
        for (floor, want) in [(0.002f32, 1.349f32), (0.005, 1.151)] {
            let got = AnchorPlacement::BlackAtBase(floor).anchor(r, c);
            assert!((got - want).abs() < 5e-4, "floor {floor}: {got} vs {want}");
        }
        let off = AnchorPlacement::MidAtBaseOffset(0.5).anchor(r, c);
        assert!(
            (off - (0.5 + MID_GREY_OUTPUT_DECADES / c)).abs() < 1e-6,
            "{off}"
        );
    }

    /// `reads_reference` must agree with [`AnchorPlacement::anchor`] rather than restate
    /// it — it is what the `Dmax`-policy gates consult, so a rule that drifts from the
    /// arithmetic would silently re-admit the false `film-master` rejection and the false
    /// roll not-frozen warning it was introduced to remove.
    #[test]
    fn reads_reference_matches_whether_the_reference_moves_the_anchor() {
        let c = 2.0f32;
        for p in [
            AnchorPlacement::WhiteAtDmax,
            AnchorPlacement::MidAtDmaxFraction(0.5),
            AnchorPlacement::BlackAtBase(0.005),
            AnchorPlacement::MidAtBaseOffset(0.5),
        ] {
            let moves = p.anchor(1.3, c) != p.anchor(1.3 + 0.295, c);
            assert_eq!(p.reads_reference(), moves, "{p:?}");
        }
    }

    #[test]
    fn all_density_curve_types_are_listed() {
        // `DensityCurveType::ALL` is what the migration messages' coverage assertions
        // iterate, so a stale list would silently re-permit the two-of-three prose it
        // exists to catch. The match is **exhaustive**: adding a variant fails to
        // compile here until it is added to `ALL` too.
        assert_eq!(DensityCurveType::ALL.len(), 3);
        for curve in DensityCurveType::ALL {
            match curve {
                DensityCurveType::Exponential
                | DensityCurveType::Sigmoid
                | DensityCurveType::Characteristic => {}
            }
        }
    }

    #[test]
    fn curve_json_round_trips_both_tagged_variants() {
        let exponential = DensityCurve::Exponential(ExponentialParams {
            gamma: 1.4,
            anchor: AnchorPlacement::WhiteAtDmax,
        });
        let sigmoid = DensityCurve::Sigmoid(SigmoidParams {
            contrast: 1.3,
            toe: 0.1,
            shoulder: 0.4,
            anchor: AnchorPlacement::WhiteAtDmax,
        });
        for curve in [exponential, sigmoid] {
            let json = serde_json::to_string(&curve).unwrap();
            assert_eq!(serde_json::from_str::<DensityCurve>(&json).unwrap(), curve);
        }
        // The wire form is internally tagged with the documented key names. The
        // default is the sigmoid; the exponential's own wire shape is asserted
        // separately so both variants stay pinned regardless of which is default.
        assert_eq!(
            serde_json::to_string(&DensityCurve::Exponential(ExponentialParams::default()))
                .unwrap(),
            r#"{"type":"exponential","gamma":2.0,"anchor":"white-at-dmax"}"#
        );
        assert_eq!(
            serde_json::to_string(&DensityCurve::default()).unwrap(),
            r#"{"type":"sigmoid","contrast":2.0686874,"toe":0.2,"shoulder":0.6,"anchor":{"mid-at-dmax-fraction":0.5}}"#
        );
    }

    #[test]
    fn curve_selector_default_matches_the_default_curve() {
        // `DensityCurveType`'s `#[default]` and `DensityCurve`'s `Default` impl are
        // two spellings of one decision, and they drifted apart once: the selector
        // still said `exponential` after the default curve became the sigmoid. That
        // is invisible in practice only because nothing calls
        // `DensityCurveType::default()` today — a latent trap, plus `--help` text
        // derived from the wrong answer. Pin them to each other rather than waiting
        // for the first caller to find it.
        assert_eq!(
            DensityCurveType::default(),
            DensityCurve::default().curve_type()
        );
    }

    #[test]
    fn curve_partial_input_fills_variant_defaults_but_requires_the_tag() {
        // A tagged-but-partial curve fills that variant's defaults.
        let c: DensityCurve = serde_json::from_str(r#"{"type":"sigmoid","contrast":2.0}"#).unwrap();
        assert_eq!(
            c,
            DensityCurve::Sigmoid(SigmoidParams {
                contrast: 2.0,
                ..SigmoidParams::default()
            })
        );
        let c: DensityCurve = serde_json::from_str(r#"{"type":"exponential"}"#).unwrap();
        assert_eq!(
            c,
            DensityCurve::Exponential(ExponentialParams::default()),
            "a tagged-but-empty exponential fills its OWN defaults, not the default curve's"
        );
        // A present-but-untagged curve object must not guess a variant.
        assert!(serde_json::from_str::<DensityCurve>(r#"{"gamma":1.2}"#).is_err());
    }

    #[test]
    fn curve_rejects_cross_variant_keys_by_name() {
        // Sigmoid-only keys under exponential are named in the error, not
        // silently ignored and not a generic unknown-field message.
        for json in [
            r#"{"type":"exponential","contrast":2.0}"#,
            r#"{"type":"exponential","toe":0.1}"#,
            r#"{"type":"exponential","shoulder":0.1}"#,
        ] {
            let err = serde_json::from_str::<DensityCurve>(json).unwrap_err();
            assert!(
                err.to_string().contains("sigmoid-curve key"),
                "unexpected error for {json}: {err}"
            );
        }
        // …and gamma under sigmoid points at `contrast`.
        let err =
            serde_json::from_str::<DensityCurve>(r#"{"type":"sigmoid","gamma":1.2}"#).unwrap_err();
        assert!(err.to_string().contains("contrast"), "{err}");
        // A key belonging to neither variant is still an unknown field.
        assert!(
            serde_json::from_str::<DensityCurve>(r#"{"type":"exponential","gama":1.2}"#).is_err()
        );
    }

    #[test]
    fn tagged_recipes_reject_null_valued_cross_variant_and_tag_keys() {
        // A present-but-null cross-variant key must be rejected by *presence*, not
        // silently read as absent (the trap a plain `Option<f32>`/`Option<T>` field
        // would fall into: serde collapses JSON `null` to `None`).
        let err = serde_json::from_str::<DensityCurve>(r#"{"type":"exponential","contrast":null}"#)
            .unwrap_err();
        assert!(err.to_string().contains("sigmoid-curve key"), "{err}");
        assert!(
            serde_json::from_str::<DensityCurve>(r#"{"type":"sigmoid","gamma":null}"#)
                .unwrap_err()
                .to_string()
                .contains("exponential-curve key")
        );
        // A null discriminator must not silently default to density…
        assert!(serde_json::from_str::<Reconstruction>(r#"{"type":null}"#).is_err());
        // …and a null forbidden section under `simple` must still be rejected.
        let err = serde_json::from_str::<Reconstruction>(r#"{"type":"simple","density":null}"#)
            .unwrap_err();
        assert!(err.to_string().contains("takes no `density`"), "{err}");
        assert!(
            serde_json::from_str::<Reconstruction>(r#"{"type":"simple","curve":null}"#).is_err()
        );
    }

    #[test]
    fn reconstruction_serializes_the_documented_tagged_shapes() {
        // `schema_version` + `type` always emitted; simple carries nothing else.
        assert_eq!(
            serde_json::to_string(&Reconstruction::Simple).unwrap(),
            r#"{"schema_version":1,"type":"simple"}"#
        );
        // Density always emits its blocks and exactly one tagged curve — an
        // omitted input curve never survives normalization.
        let json = serde_json::to_value(Reconstruction::default()).unwrap();
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["type"], "density");
        // The default gain is `[1, 0.84, 0.73]` (a scanner calibration; see
        // `DensityParams::scale`). Compared against the `f32` values rather than a JSON
        // literal, because `0.84f32` widens to `0.8399999737739563` as an `f64` — the
        // emitted *text* is still the round-trip-shortest `0.84`, which is what a user's
        // recipe carries.
        assert_eq!(
            json["density"]["scale"],
            serde_json::json!([1.0f32, 0.84f32, 0.73f32])
        );
        assert_eq!(json["curve"]["type"], "sigmoid");
    }

    /// An **omitted** `density.scale` resolves from the curve; a **stated** one is kept
    /// under every curve.
    ///
    /// The distinction cannot be read off `DensityParams` after serde has built one — the
    /// field is a concrete `[f32; 3]`, so an absent key and an explicit `[1, 1, 1]` are
    /// the same value — which is why `Reconstruction`'s `Deserialize` inspects the raw
    /// object. This pins both halves, because losing either is a silent colour change:
    /// resolve-always would ignore a gain the user wrote down, and keep-always would put
    /// the parametric curves' calibration onto the characteristic curve, which already
    /// carries each stock's per-channel response (see `DensityParams::default_scale_for`).
    #[test]
    fn an_omitted_density_scale_resolves_from_the_curve() {
        let scale = |json: &str| {
            let r: Reconstruction = serde_json::from_str(json).unwrap();
            match r {
                Reconstruction::Density { density, .. } => density.scale,
                Reconstruction::Simple => unreachable!("density expected"),
            }
        };
        let parametric = DensityParams::default_scale_for(DensityCurveType::Sigmoid);
        let identity = DensityParams::default_scale_for(DensityCurveType::Characteristic);

        // Omitted: per-curve.
        assert_eq!(
            scale(r#"{"type":"density","curve":{"type":"sigmoid"}}"#),
            parametric
        );
        assert_eq!(
            scale(r#"{"type":"density","curve":{"type":"exponential"}}"#),
            parametric
        );
        assert_eq!(
            scale(r#"{"type":"density","curve":{"type":"characteristic"}}"#),
            identity
        );
        // An omitted *curve* normalizes to sigmoid, so it takes sigmoid's gain.
        assert_eq!(scale(r#"{"type":"density"}"#), parametric);
        // A `density` block that states other keys but not `scale` still resolves.
        assert_eq!(
            scale(
                r#"{"type":"density","density":{"offset":[0.0,0.0,0.1]},
                    "curve":{"type":"characteristic"}}"#
            ),
            identity
        );

        // Stated: kept, including a value equal to another curve's default — the whole
        // reason presence is checked rather than inferred from the value.
        assert_eq!(
            scale(
                r#"{"type":"density","density":{"scale":[1.0,1.0,1.0]},
                    "curve":{"type":"sigmoid"}}"#
            ),
            [1.0, 1.0, 1.0]
        );
        assert_eq!(
            scale(
                r#"{"type":"density","density":{"scale":[1.0,0.84,0.73]},
                    "curve":{"type":"characteristic"}}"#
            ),
            [1.0, 0.84, 0.73]
        );
        assert_eq!(
            scale(
                r#"{"type":"density","density":{"scale":[1.2,1.0,0.8]},
                    "curve":{"type":"characteristic"}}"#
            ),
            [1.2, 1.0, 0.8]
        );
        assert_eq!(
            scale(
                r#"{"type":"density","density":{"scale":[1.2,1.0,0.8]},
                    "curve":{"type":"exponential"}}"#
            ),
            [1.2, 1.0, 0.8]
        );
        assert_eq!(
            scale(
                r#"{"type":"density","density":{"scale":[1.2,1.0,0.8]},
                    "curve":{"type":"sigmoid"}}"#
            ),
            [1.2, 1.0, 0.8]
        );

        // And the resolved form round-trips: re-reading an emitted recipe cannot
        // re-resolve, because serialization always writes the key.
        for json in [
            r#"{"type":"density","curve":{"type":"characteristic"}}"#,
            r#"{"type":"density","curve":{"type":"sigmoid"}}"#,
        ] {
            let once: Reconstruction = serde_json::from_str(json).unwrap();
            let emitted = serde_json::to_string(&once).unwrap();
            let twice: Reconstruction = serde_json::from_str(&emitted).unwrap();
            assert_eq!(once, twice, "resolution is not idempotent for {json}");
        }
    }

    /// A non-object `reconstruction.density` is refused, naming the section.
    ///
    /// `DensityParams` is a plain derive, so serde also accepts it in **positional-array**
    /// form — and that form used to deserialize *successfully* while the `scale` probe
    /// above (which needs `as_object()`) read "not stated", so the array's stated gain was
    /// silently replaced by the per-curve default. Rejecting the shape is the fix; reading
    /// a non-object as "assume stated" would leave the ambiguity in place. Same guard, and
    /// the same message shape, as `DensityCurve`'s `reconstruction.curve` check.
    #[test]
    fn a_non_object_density_section_is_refused() {
        // The exact array a `DensityParams` derive accepts, field for field.
        let array = r#"{"type":"density",
            "density":[[1.2,1.0,0.8],[0,0,0],[0,0,0],[0,0,0],"auto"],
            "curve":{"type":"characteristic"}}"#;
        let err = serde_json::from_str::<Reconstruction>(array)
            .expect_err("an array-shaped `density` must be refused, not half-read")
            .to_string();
        assert!(
            err.contains("reconstruction.density"),
            "the message must name the section: {err}"
        );
        // Falsifiability: the same recipe with the object spelling parses *and* keeps the
        // gain, so the guard rejects the shape rather than the value.
        let object = r#"{"type":"density",
            "density":{"scale":[1.2,1.0,0.8]},
            "curve":{"type":"characteristic"}}"#;
        let Reconstruction::Density { density, .. } = serde_json::from_str(object).unwrap() else {
            unreachable!("density expected");
        };
        assert_eq!(density.scale, [1.2, 1.0, 0.8]);

        // Every other non-object spelling is refused the same way, including a present
        // explicit `null` (which must never read as an absent key).
        for bad in [
            r#"{"type":"density","density":null}"#,
            r#"{"type":"density","density":1.0}"#,
            r#"{"type":"density","density":"scale"}"#,
        ] {
            let err = serde_json::from_str::<Reconstruction>(bad)
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("reconstruction.density"),
                "the message must name the section for {bad}: {err}"
            );
        }
    }

    #[test]
    fn reconstruction_partial_input_normalizes() {
        // Empty object = all defaults: density with the default (sigmoid) curve.
        let r: Reconstruction = serde_json::from_str("{}").unwrap();
        assert_eq!(r, Reconstruction::default());
        // Omitted schema_version defaults to 1; an explicit 1 also parses.
        let r: Reconstruction = serde_json::from_str(r#"{"schema_version":1}"#).unwrap();
        assert_eq!(r, Reconstruction::default());
        // Omitted curve under density normalizes to the default tagged curve.
        let r: Reconstruction =
            serde_json::from_str(r#"{"type":"density","density":{"scale":[1.1,1.0,0.9]}}"#)
                .unwrap();
        assert_eq!(
            r,
            Reconstruction::Density {
                density: DensityParams {
                    scale: [1.1, 1.0, 0.9],
                    ..DensityParams::default()
                },
                curve: DensityCurve::default(),
            }
        );
        // Round trip: resolved → JSON → resolved is identity for both types.
        for r in [
            Reconstruction::Simple,
            Reconstruction::Density {
                density: DensityParams::default(),
                curve: DensityCurve::Sigmoid(SigmoidParams::default()),
            },
        ] {
            let json = serde_json::to_string(&r).unwrap();
            assert_eq!(serde_json::from_str::<Reconstruction>(&json).unwrap(), r);
        }
    }

    #[test]
    fn reconstruction_rejects_bad_schema_version_and_illegal_combinations() {
        // Any schema_version other than 1 is rejected loudly.
        let err = serde_json::from_str::<Reconstruction>(r#"{"schema_version":2}"#).unwrap_err();
        assert!(err.to_string().contains("schema_version"), "{err}");
        // Simple takes no density/curve section.
        for json in [
            r#"{"type":"simple","density":{}}"#,
            r#"{"type":"simple","curve":{"type":"exponential"}}"#,
        ] {
            let err = serde_json::from_str::<Reconstruction>(json).unwrap_err();
            assert!(err.to_string().contains("simple"), "{json}: {err}");
        }
        // Unknown fields are rejected at the reconstruction level too.
        assert!(serde_json::from_str::<Reconstruction>(r#"{"algorithm":"density"}"#).is_err());
    }

    #[test]
    fn density_params_default_regional_balance_is_neutral() {
        // The identity defaults the bit-exact-default guarantee rests on.
        let d = DensityParams::default();
        assert_eq!(d.shadow_balance, [0.0, 0.0, 0.0]);
        assert_eq!(d.highlight_balance, [0.0, 0.0, 0.0]);
        assert_eq!(d.balance_range, BalanceRange::Auto);
    }

    #[test]
    fn balance_range_serializes_like_dmax_source() {
        // Unit variant is a bare lowercase string; the newtype variant is a
        // tagged object — the same shape convention as `DmaxSource`.
        assert_eq!(
            serde_json::to_string(&BalanceRange::Auto).unwrap(),
            "\"auto\""
        );
        assert_eq!(
            serde_json::to_string(&BalanceRange::Explicit([0.25, 2.5])).unwrap(),
            r#"{"explicit":[0.25,2.5]}"#
        );
        for src in [BalanceRange::Auto, BalanceRange::Explicit([0.1, 1.9])] {
            let json = serde_json::to_string(&src).unwrap();
            assert_eq!(serde_json::from_str::<BalanceRange>(&json).unwrap(), src);
        }
    }

    #[test]
    fn partial_recipe_fills_defaults() {
        // A recipe that sets only one knob should leave the rest at defaults.
        let params: PrintParams = serde_json::from_str(r#"{"print_exposure": 2.0}"#).unwrap();
        assert_eq!(params.print_exposure, 2.0);
        assert_eq!(params.white_balance, WbSource::Explicit([1.0, 1.0, 1.0]));
    }

    #[test]
    fn wb_source_serializes_like_the_other_source_enums() {
        // Unit variants are bare kebab-case strings; the payload variant is a
        // tagged object — the same shape convention as `FilmBaseSource` /
        // `DmaxSource`.
        assert_eq!(
            serde_json::to_string(&WbSource::GrayWorld).unwrap(),
            "\"gray-world\""
        );
        assert_eq!(
            serde_json::to_string(&WbSource::Percentile).unwrap(),
            "\"percentile\""
        );
        assert_eq!(
            serde_json::to_string(&WbSource::Explicit([1.1, 1.0, 0.9])).unwrap(),
            r#"{"explicit":[1.1,1.0,0.9]}"#
        );
        for src in [
            WbSource::GrayWorld,
            WbSource::Percentile,
            WbSource::Explicit([2.0, 1.0, 0.5]),
        ] {
            let json = serde_json::to_string(&src).unwrap();
            assert_eq!(serde_json::from_str::<WbSource>(&json).unwrap(), src);
        }
    }

    #[test]
    fn wb_source_deserializes_legacy_bare_array_as_explicit() {
        // Before `WbSource`, `print.white_balance` was a plain `[f32; 3]`, so
        // existing recipes/sidecars serialize the bare array. The custom
        // `Deserialize` must still accept it (→ `Explicit`) for reproducibility,
        // alongside the tagged forms.
        assert_eq!(
            serde_json::from_str::<WbSource>("[1.1,1.0,0.9]").unwrap(),
            WbSource::Explicit([1.1, 1.0, 0.9])
        );
        // The same, nested in a recipe's `print` section (defaults fill the rest).
        let print: PrintParams =
            serde_json::from_str(r#"{"white_balance":[1.1,1.0,0.9]}"#).unwrap();
        assert_eq!(print.white_balance, WbSource::Explicit([1.1, 1.0, 0.9]));
        // The tagged forms still parse (the bare array is an *addition*).
        assert_eq!(
            serde_json::from_str::<WbSource>(r#"{"explicit":[1.1,1.0,0.9]}"#).unwrap(),
            WbSource::Explicit([1.1, 1.0, 0.9])
        );
        assert_eq!(
            serde_json::from_str::<WbSource>("\"gray-world\"").unwrap(),
            WbSource::GrayWorld
        );
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
        // is emitted earlier, by `cli::load_recipe_for`).
        assert!(serde_json::from_str::<InputParams>(r#"{"color":"linear"}"#).is_err());
    }

    #[test]
    fn wb_source_default_is_neutral_explicit_gains() {
        // The default must be *explicit* neutral gains, not an auto mode — auto
        // white balance is opt-in, and the default output stays bit-identical to
        // the pre-auto-WB render.
        assert_eq!(WbSource::default(), WbSource::Explicit([1.0, 1.0, 1.0]));
        assert_eq!(PrintParams::default().white_balance, WbSource::default());
    }

    #[test]
    fn linear_range_default_is_the_exact_identity_and_lives_under_print() {
        // `(x − 0)/(1 − 0)` is the exact identity, so adding the knob cannot perturb
        // any existing output. Its recipe home is `print.linear_range` (design-spec
        // §9's Print / tone render section) — a misplaced key would be silently
        // rejected by `deny_unknown_fields` on docs-shaped recipes.
        assert_eq!(PrintParams::default().linear_range, [0.0, 1.0]);
        let p: PrintParams = serde_json::from_str(r#"{"linear_range":[0.02,0.97]}"#).unwrap();
        assert_eq!(p.linear_range, [0.02, 0.97]);
        // Round-trips, and the untouched siblings keep their defaults.
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"linear_range\":[0.02,0.97]"), "{json}");
        assert_eq!(serde_json::from_str::<PrintParams>(&json).unwrap(), p);
        assert_eq!(p.print_exposure, 0.0);
        // The removed simple-reconstruction spelling is not a recipe key here.
        assert!(serde_json::from_str::<PrintParams>(r#"{"clip_low":0.02}"#).is_err());
    }

    #[test]
    fn output_preset_parses_accepted_names_and_diagnoses_the_rest() {
        assert_eq!(
            OutputPreset::parse("film-master").unwrap(),
            OutputPreset::FilmMaster
        );
        assert_eq!(
            OutputPreset::parse("ultra-hdr-v1").unwrap(),
            OutputPreset::UltraHdrV1
        );
        // Keywords, so case/whitespace-insensitive like `OutputSpace::parse`.
        assert_eq!(
            OutputPreset::parse(" Film-Master ").unwrap(),
            OutputPreset::FilmMaster
        );
        // The product default since the `output/presets` migration.
        assert_eq!(OutputPreset::default(), OutputPreset::GainMapHdr);

        // The two retired names are removed-value errors naming the replacements and
        // the reference build — never a generic "unknown", and never an alias.
        for removed in ["legacy", "custom", " Legacy "] {
            let err = OutputPreset::parse(removed).unwrap_err();
            assert!(matches!(err, NcError::Usage(_)), "{err:?}");
            let msg = err.message();
            assert!(msg.contains("was removed"), "{msg}");
            assert!(!msg.contains("unknown output preset"), "{msg}");
            for replacement in ["display-p3", "film-master", "hdr-linear-tiff"] {
                assert!(msg.contains(replacement), "{msg}");
            }
            assert!(msg.contains("reference-snapshot"), "{msg}");
            assert!(msg.contains("no alias"), "{msg}");
        }
        // Only `custom`'s message explains the selectors it existed for.
        assert!(
            OutputPreset::parse("custom")
                .unwrap_err()
                .message()
                .contains("selectors")
        );
        assert!(
            !OutputPreset::parse("legacy")
                .unwrap_err()
                .message()
                .contains("selectors")
        );

        // The pre-release name is an unreleased-schema break, NOT an alias: the
        // message must name the rename and the reason, and must not silently accept.
        let err = OutputPreset::parse("scene-master").unwrap_err();
        assert!(matches!(err, NcError::Usage(_)), "{err:?}");
        let msg = err.to_string();
        assert!(msg.contains("scene-master"), "{msg}");
        assert!(msg.contains("film-master"), "{msg}");
        assert!(msg.contains("no alias"), "{msg}");

        // A planned-but-unimplemented name gets its own diagnosis rather than a bare
        // "unknown", so an agent can tell "not yet" from "typo".
        // There is no planned-but-unaccepted name left: `gain-map-hdr` and `custom`
        // were the last two, and `output/presets` shipped both. The "does not accept
        // yet" arm is gone with them — an unknown name now always means a typo, which
        // is why the diagnostic below is the only one left.
        // `hdr-pq` / `hdr-hlg` graduated out of that list when
        // `output/hdr-avif-output` activated them, and `hdr-linear-tiff`,
        // `hdr-pq-tiff` and `hdr-hlg-tiff` when `output/lossless-hdr-tiff` did — all
        // six are accepted now. They are asserted here name by name so the AVIF, the
        // linear-TIFF, and the coded-TIFF families cannot be confused back together.
        // `display-p3` and `compatibility` graduated when the SDR presets landed, and
        // `gain-map-hdr` when `output/presets` wired the dual-dialect container up.
        // It is asserted separately from `ultra-hdr-v1` on purpose: the two write the
        // same pixels and differ only in metadata dialect, which is exactly the pair
        // a future edit could collapse.
        assert_eq!(
            OutputPreset::parse("gain-map-hdr").unwrap(),
            OutputPreset::GainMapHdr
        );
        assert_eq!(
            OutputPreset::parse("display-p3").unwrap(),
            OutputPreset::DisplayP3
        );
        assert_eq!(
            OutputPreset::parse("compatibility").unwrap(),
            OutputPreset::Compatibility
        );
        assert_eq!(OutputPreset::parse("hdr-pq").unwrap(), OutputPreset::HdrPq);
        assert_eq!(
            OutputPreset::parse("hdr-hlg").unwrap(),
            OutputPreset::HdrHlg
        );
        assert_eq!(
            OutputPreset::parse("hdr-linear-tiff").unwrap(),
            OutputPreset::HdrLinearTiff
        );
        // The three TIFF HDR presets are distinct names, and `hdr-pq` vs
        // `hdr-pq-tiff` differing only by suffix is exactly the confusion worth
        // pinning: one writes AVIF, the other TIFF, from an identical rendition.
        assert_eq!(
            OutputPreset::parse("hdr-pq-tiff").unwrap(),
            OutputPreset::HdrPqTiff
        );
        assert_eq!(
            OutputPreset::parse("hdr-hlg-tiff").unwrap(),
            OutputPreset::HdrHlgTiff
        );
        assert_ne!(OutputPreset::HdrPq, OutputPreset::HdrPqTiff);
        let msg = OutputPreset::parse("filmmaster").unwrap_err().to_string();
        assert!(msg.contains("unknown output preset"), "{msg}");
    }

    #[test]
    fn every_accepted_preset_is_listed_in_the_parse_diagnostics() {
        // Both "accepted: …" lists used to be hand-written, and both went stale the
        // moment a preset shipped — so `--output-preset displayp3` listed eight names
        // and hid the one the user wanted. They are generated from `ALL` now, and this
        // is the test that keeps `ALL` itself honest.
        let unknown = OutputPreset::parse("displayp3").unwrap_err().to_string();
        for preset in OutputPreset::ALL {
            let name = preset.name();
            assert!(
                unknown.contains(name),
                "unknown-preset message omits {name}"
            );
            // Every entry is a name `parse` actually accepts (a typo in `ALL` would
            // otherwise advertise a name that then fails).
            assert_eq!(OutputPreset::parse(name).unwrap(), preset, "{name}");
        }
        // Completeness guard: adding a variant breaks this match, and the arm the
        // author has to write sits next to the reminder to extend `ALL`.
        for preset in OutputPreset::ALL {
            match preset {
                OutputPreset::FilmMaster
                | OutputPreset::UltraHdrV1
                | OutputPreset::GainMapHdr
                | OutputPreset::DisplayP3
                | OutputPreset::Compatibility
                | OutputPreset::HdrPq
                | OutputPreset::HdrHlg
                | OutputPreset::HdrLinearTiff
                | OutputPreset::HdrPqTiff
                | OutputPreset::HdrHlgTiff => {
                    assert!(OutputPreset::ALL.contains(&preset));
                }
            }
        }
    }

    #[test]
    fn output_preset_recipe_key_round_trips_and_shares_the_parse_diagnostics() {
        // Recipe home is `output.preset` (design-spec §9's Output / encode section).
        let o: OutputParams = serde_json::from_str(r#"{"preset":"film-master"}"#).unwrap();
        assert_eq!(o.preset, OutputPreset::FilmMaster);
        let json = serde_json::to_string(&o).unwrap();
        assert!(json.contains("\"preset\":\"film-master\""), "{json}");
        assert_eq!(serde_json::from_str::<OutputParams>(&json).unwrap(), o);
        assert_eq!(
            serde_json::from_str::<OutputParams>(r#"{}"#)
                .unwrap()
                .preset,
            OutputPreset::GainMapHdr
        );
        // The custom `Deserialize` delegates to `parse`, so the recipe key gets the
        // same pinned rename diagnosis as the flag (serde's derived error would only
        // list the accepted variants and never mention the rename).
        let err = serde_json::from_str::<OutputParams>(r#"{"preset":"scene-master"}"#).unwrap_err();
        assert!(err.to_string().contains("renamed"), "{err}");
        let err = serde_json::from_str::<OutputParams>(r#"{"preset":"legacy"}"#).unwrap_err();
        assert!(err.to_string().contains("was removed"), "{err}");
        // Every name the flag accepts, the recipe key accepts too — a preset reachable
        // from one but not the other would be unusable in `hanten roll`, which has no
        // output flags at all.
        for preset in OutputPreset::ALL {
            let json = format!(r#"{{"preset":"{}"}}"#, preset.name());
            assert_eq!(
                serde_json::from_str::<OutputParams>(&json).unwrap().preset,
                preset,
                "{json}"
            );
        }
        assert_eq!(
            serde_json::from_str::<OutputParams>(r#"{"preset":"gain-map-hdr"}"#)
                .unwrap()
                .preset,
            OutputPreset::GainMapHdr
        );
    }
}

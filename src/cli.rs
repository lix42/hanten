//! CLI orchestration — the agent-facing command surface.
//!
//! This is the scriptable contract an agent drives: clap argument parsing for
//! every subcommand and flag (design-spec §8–9), JSON recipe load/merge (flags
//! override a loaded recipe), `--dump-params` / `params` for discovery, a JSON
//! report, and stable exit codes via [`NcError`]. The conversion runs here:
//! `convert` drives the full read → film-base → fixed decode → scene correction →
//! look → fit range → fit gamut → encode chain (delegating the pure stages to
//! `pipeline`/`algo`/`io`); `inspect` and `estimate` decode and report without
//! writing an image.
//!
//! Determinism rule: stdout carries *only* the JSON report / params; all logs and
//! warnings go to stderr, so an agent can pipe stdout straight into a parser.

use std::ffi::OsStr;
use std::fmt::Display;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use clap::{Args, Parser, Subcommand};
use serde::{Deserialize, Serialize};

use crate::algo::{FilmRgbImage, fixed};
use crate::destination::{
    Axis, Container, DisplayAxes, Encoding, Gamut, OutputSection, Range, Resolved, Transfer,
};

use crate::io::decode::{DecodeInfo, decode_within, probe};
use crate::io::{avif, encode, iso_gain_map, staged};
use crate::pipeline::chain;
use crate::pipeline::fit_gamut::DestinationGamut;
use crate::pipeline::fit_range;
use crate::pipeline::input_semantics::{
    self, ContainerColorFacts, InputAssertions, InputColorReport, RawMode,
};
use crate::pipeline::memory::{self, MemoryReport, RunProfile, SamplePlan};
use crate::pipeline::working_space::AcesCgImage;
use crate::pipeline::{
    color, film_base, gain_encode, gain_ratio, hdr, look, roll_white, scene_correction,
    working_space,
};
use crate::recipe::{self, KnobNames, Recipe};
use crate::stage::{StageClock, StageKind};
use crate::telemetry;
use crate::types::{
    DEFAULT_MEASURE_INSET, EncodeOutcome, EncodeReport, FilmBase, FilmBaseSource, FilmType,
    InputParams, LinearImage, MeaningAssertion, NcError, OutDepth, OutputStats,
    REMOVED_SIMPLE_RECONSTRUCTION, Result, TransferAssertion, check_measure_inset,
};
use crate::version::{self, Identity};

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// `hanten` — film-negative → positive converter.
//
// `--version` prints the full build identity (semver + behavioral
// `pipeline_version` + commit + target), not just the crate version, so an output
// can be attributed to a build — see `version::version_string`.
#[derive(Parser, Debug)]
#[command(
    // `name` is what `--version` prints before the identity block and what every
    // usage/error line spells, so it is the product name. `nctool`'s `is_nc`
    // recognises this banner — it must accept the pre-rename `nc ` too, since the
    // reference rendition comes from the pre-rename reference build (CLAUDE.md's
    // boundary).
    name = "hanten",
    version = version::version_string(),
    about = "Hanten — film-negative → positive converter"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
// `convert` legitimately carries the full parameter surface; boxing it would
// only fight clap's derive for a one-shot CLI enum that's never stored en masse.
#[allow(clippy::large_enum_variant)]
pub enum Command {
    /// Convert a negative scan to a positive image.
    Convert(ConvertArgs),
    /// Convert a roll (batch of frames) from one shared, frozen recipe.
    Roll(RollArgs),
    /// Inspect a scan and emit a JSON report (no output image).
    Inspect(IoArgs),
    /// Run only film-base / Dmin estimation; emit JSON.
    Estimate(EstimateArgs),
    /// Measure a roll's white balance and white once, for its recipe; emit JSON.
    MeasureRoll(MeasureRollArgs),
    /// Print the full default parameter set as JSON (recipe scaffolding).
    Params(ParamsArgs),
}

/// `hanten params` options.
#[derive(Args, Debug)]
pub struct ParamsArgs {
    /// Removed: see [`reject_new_flow`].
    #[arg(long = "new-flow", hide = true)]
    pub new_flow: bool,
}

/// Refuse the removed `--new-flow` selector (`nf-core/default-flip`), on every command
/// that took it. Hidden, and kept only to emit this migration error — there is no
/// alias, since there is no longer a second chain to select.
fn reject_new_flow(new_flow: bool) -> Result<()> {
    if new_flow {
        return Err(NcError::Usage(
            "--new-flow was removed: the rendering chain it selected is now the only one \
             (pipeline_version 8). Drop the flag. A recipe written before that version is \
             refused on load; `hanten params` writes the current layout."
                .into(),
        ));
    }
    Ok(())
}

/// Report format on stdout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, clap::ValueEnum)]
#[allow(clippy::enum_variant_names)]
pub enum ReportFormat {
    /// Machine-readable JSON report.
    #[default]
    Json,
    /// No report.
    None,
}

/// Reporting / verbosity controls shared by every subcommand.
#[derive(Args, Debug, Default)]
pub struct ReportArgs {
    /// Report format emitted on stdout.
    #[arg(long, value_enum, default_value_t = ReportFormat::Json)]
    pub report: ReportFormat,
    /// Write the report here instead of stdout.
    #[arg(long, value_name = "PATH")]
    pub report_file: Option<PathBuf>,
    /// Increase stderr logging (-v, -vv). Never pollutes stdout.
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,
    /// Suppress non-error stderr logging.
    #[arg(long)]
    pub quiet: bool,
}

/// The memory preflight's budget — **operational, not a conversion knob**, so it
/// lives here on the arg structs like `--report`/`--strict`/`--telemetry` and is
/// deliberately *not* a recipe key: it never enters the recipe and can
/// never perturb a pixel (design-spec §9 Global). Shared by every subcommand that
/// decodes a scan; each gates on its own pipeline profile (`pipeline::memory`).
#[derive(Args, Debug, Default)]
pub struct MemoryArgs {
    /// Fail before decoding if this run's estimated peak memory would exceed this
    /// budget (e.g. `8GiB`, `4096MB`, raw bytes). Defaults to 6 GiB — a fixed
    /// value, so the pass/fail decision is the same on every machine. Operational
    /// flag — not a recipe key; never affects the output image.
    #[arg(long = "max-memory", value_name = "BYTES", value_parser = parse_max_memory_arg)]
    pub max_memory: Option<u64>,
}

impl MemoryArgs {
    /// The run's resolved budget (the flag, else the fixed default).
    fn budget(&self) -> memory::Budget {
        memory::Budget::resolve(self.max_memory)
    }
}

/// clap adapter for [`memory::parse_max_memory`] — clap wants a `String` error.
fn parse_max_memory_arg(s: &str) -> std::result::Result<u64, String> {
    memory::parse_max_memory(s).map_err(|e| e.to_string())
}

/// `inspect`: an input scan plus reporting controls.
#[derive(Args, Debug)]
pub struct IoArgs {
    /// Input negative scan (SilverFast HDR/HDRi TIFF).
    pub input: PathBuf,
    /// Declared film chemistry (`silver` | `chromogenic`). Provenance only: it does
    /// **not** gate IR-assisted film-holder detection, which measures the IR plane
    /// itself. See `convert --film-type`.
    #[arg(long = "film-type", value_enum, value_name = "TYPE")]
    pub film_type: Option<FilmType>,
    #[command(flatten)]
    pub measure: MeasureOverrides,
    #[command(flatten)]
    pub memory: MemoryArgs,
    #[command(flatten)]
    pub report: ReportArgs,
}

/// `estimate`: an input scan, the film-base source flags (so the
/// calibrate-once-from-a-reference workflow works, design-spec §8), the grid
/// calibration mode, and reporting controls.
#[derive(Args, Debug)]
pub struct EstimateArgs {
    /// Input negative scan (SilverFast HDR/HDRi TIFF).
    pub input: PathBuf,
    /// Sample a fixed 5-cell grid (corners + center) over the frame — or over
    /// `--base-region` — instead of a single measurement. For unexposed
    /// reference frames (design-spec §9 ladder tier 1): the per-cell spread is
    /// reported and disagreement warns loudly (it diagnoses light leaks,
    /// illumination falloff, or dust). Incompatible with an explicit
    /// `--film-base` (nothing to sample) and with `--auto-base` (grid replaces
    /// border detection).
    #[arg(long, conflicts_with_all = ["film_base", "auto_base"])]
    pub grid: bool,
    /// Retired with the roll reference density (`nf-retire/dmax-machinery`). Hidden,
    /// and kept only to emit a migration error.
    #[arg(long = "d-max-region", hide = true, value_name = "X,Y,W,H", num_args = 0..=1, default_missing_value = "", allow_hyphen_values = true)]
    pub d_max_region: Option<String>,
    /// Declared film chemistry (`silver` | `chromogenic`). Provenance only: it does
    /// **not** gate IR-assisted film-holder detection, which measures the IR plane
    /// itself. See `convert --film-type`.
    #[arg(long = "film-type", value_enum, value_name = "TYPE")]
    pub film_type: Option<FilmType>,
    #[command(flatten)]
    pub film_base: FilmBaseOverrides,
    #[command(flatten)]
    pub measure: MeasureOverrides,
    /// Treat estimation warnings (a non-uniform `--base-region`, grid
    /// disagreement, decode notes, …) as a hard error. `estimate` produces the
    /// `Dmin` a roll is calibrated on, so a script baking the result into a
    /// recipe wants a plausible-looking-but-bad base to fail loudly rather than
    /// be echoed back.
    #[arg(long)]
    pub strict: bool,
    #[command(flatten)]
    pub memory: MemoryArgs,
    #[command(flatten)]
    pub report: ReportArgs,
}

/// `measure-roll`: the roll's picture frames, its leader, and the recipe they are
/// decoded under (`nf-scene-correction/roll-white-balance`).
#[derive(Args, Debug)]
pub struct MeasureRollArgs {
    /// The roll's picture frames (SilverFast HDR/HDRi TIFF). Leave out the unexposed
    /// base, the leader and any calibration frame: every input is pooled as picture.
    #[arg(required = true)]
    pub inputs: Vec<PathBuf>,
    /// The roll's leader — a fully exposed frame, decoded with the same base. Pixels
    /// within 0.1 density of it are left out of the white balance, so a fully exposed
    /// frame mixed into the roll cannot set the gains (measured: it would move them
    /// 0.4–1.3 stops), and a frame whose white comes within 0.5 stop of it warns as near
    /// film saturation. Without it the run warns and nothing is checked for saturation,
    /// and `--strict` refuses before decoding anything.
    #[arg(long, value_name = "PATH")]
    pub leader: Option<PathBuf>,
    /// The roll's recipe (`"recipe_version": 2`): the film base and
    /// the decode the gains and the white are measured under. Its `scene_correction` and
    /// `look` values are not read — this command measures the white balance and the
    /// contrast — though the recipe must still load (a retired or unknown key there is
    /// refused).
    #[arg(long = "params", value_name = "JSON")]
    pub recipe_in: Option<PathBuf>,
    /// The roll's film base (Dmin) as `R,G,B`, over the recipe's. Required one way or
    /// the other, and explicit: a base estimated per frame would measure each frame
    /// under a different decode. Measure it once with `hanten estimate --grid` on the
    /// unexposed base frame.
    #[arg(long = "film-base", value_name = "R,G,B", value_parser = parse_rgb)]
    pub film_base: Option<[f32; 3]>,
    #[command(flatten)]
    pub measure: MeasureOverrides,
    /// Treat warnings (a capped holder march, decode notes) as a hard error, and
    /// refuse to measure without `--leader`.
    #[arg(long)]
    pub strict: bool,
    #[command(flatten)]
    pub memory: MemoryArgs,
    #[command(flatten)]
    pub report: ReportArgs,
}

/// `convert`: input, output, and every conversion knob (design-spec §9).
///
/// Stage knobs are grouped into flattened `*Overrides` structs; each field is an
/// `Option` (or a presence flag) so [`recipe::merge`] can tell "explicitly passed"
/// from "left at the recipe / default value".
#[derive(Args, Debug)]
pub struct ConvertArgs {
    /// Input negative scan (SilverFast HDR/HDRi TIFF).
    pub input: PathBuf,
    /// Output positive path. The suffix is optional: leave it off and Hanten
    /// appends the resolved destination's container (see --container). A stated
    /// suffix is never rewritten, so it must be one that destination writes.
    #[arg(short = 'o', long, value_name = "PATH")]
    pub output: PathBuf,
    /// Removed with `simple` reconstruction: density is the only reconstruction.
    /// Hidden, and kept only to emit a migration error — there is no alias.
    #[arg(long, hide = true, value_name = "TYPE")]
    pub reconstruction: Option<String>,
    /// Removed: the pre-reconstruction algorithm selector. Kept hidden only to
    /// emit a migration error (nc is unreleased — no aliases).
    #[arg(long, hide = true, value_name = "NAME")]
    pub algorithm: Option<String>,

    #[command(flatten)]
    pub input_opts: InputOverrides,
    #[command(flatten)]
    pub film_base: FilmBaseOverrides,
    #[command(flatten)]
    pub measure: MeasureOverrides,
    #[command(flatten)]
    pub density: DensityOverrides,
    #[command(flatten)]
    pub dmax: RemovedDmaxFlags,
    #[command(flatten)]
    pub balance: RemovedBalanceFlags,
    #[command(flatten)]
    pub sigmoid: RemovedSigmoidFlags,
    #[command(flatten)]
    pub characteristic: RemovedCharacteristicFlags,
    #[command(flatten)]
    pub anchor: AnchorOverrides,
    #[command(flatten)]
    pub removed_print: RemovedPrintFlags,
    #[command(flatten)]
    pub rendering: RenderingOverrides,
    #[command(flatten)]
    pub roll: RollOverrides,
    #[command(flatten)]
    pub scene: SceneCorrectionOverrides,
    #[command(flatten)]
    pub look: LookOverrides,
    #[command(flatten)]
    pub display: DisplayOverrides,
    #[command(flatten)]
    pub simple: SimpleOverrides,
    #[command(flatten)]
    pub removed_output: RemovedOutputFlags,
    #[command(flatten)]
    pub destination: DestinationOverrides,

    /// Load a JSON recipe; individual `--flag`s override its values.
    #[arg(long = "params", value_name = "JSON")]
    pub recipe_in: Option<PathBuf>,
    /// Write the effective (resolved) parameters to JSON and continue.
    #[arg(long, value_name = "JSON")]
    pub dump_params: Option<PathBuf>,
    /// Treat warnings (clipping, IR-ignored, …) as hard errors.
    #[arg(long)]
    pub strict: bool,
    /// Fix any stochastic step for reproducibility (none in Step 1; reserved).
    #[arg(long, value_name = "N")]
    pub seed: Option<u64>,

    /// Append a telemetry record for this run to the local JSONL log (under the
    /// platform data dir, e.g. `$XDG_DATA_HOME/nc/telemetry.jsonl` or
    /// `~/.local/share/nc/telemetry.jsonl`; override with `NC_TELEMETRY_LOG`).
    /// Operational flag — not a recipe key; never affects the output image.
    #[arg(long)]
    pub telemetry: bool,
    /// Also write this run's telemetry record to `<path>` (`-` = stdout). May be
    /// combined with `--telemetry`. Operational flag — not a recipe key.
    #[arg(long, value_name = "PATH")]
    pub telemetry_file: Option<String>,

    /// Removed: see [`reject_new_flow`].
    #[arg(long = "new-flow", hide = true)]
    pub new_flow: bool,

    #[command(flatten)]
    pub memory: MemoryArgs,
    #[command(flatten)]
    pub report: ReportArgs,
}

/// `hanten roll`: convert a batch of frames from ONE shared, frozen recipe so the
/// whole roll is color-consistent and reproducible (design-spec §8, §12 item 6).
///
/// This is the batch-**apply** half of plan→recipe→apply: it replays a *provided*
/// frozen recipe (hand-authored or `hanten params`/`--dump-params`-produced) over N
/// frames. It deliberately owns no auto-cascade that *generates* the recipe —
/// that is `core/auto-calibration`. Roll-fixed params (the
/// film base) live in the shared `--params` recipe and appear
/// once in the roll report; frame-local params can be overridden per frame via a
/// `--frames` manifest.
///
/// Unlike `convert`'s single `-o <file>`, roll writes per-frame outputs into an
/// `--out-dir` (named `<stem>_positive.<ext>`, the suffix of the frame's resolved
/// destination) plus a roll-level JSON report on stdout, so single-frame `convert`
/// stays byte-for-byte unchanged.
#[derive(Args, Debug)]
pub struct RollArgs {
    /// Input scans: files, directories (expanded to their `.tif`/`.tiff` files),
    /// or shell globs (expanded by the shell). Collected and sorted for a
    /// deterministic frame order. Mutually exclusive with `--frames`.
    #[arg(required_unless_present = "frames", conflicts_with = "frames")]
    pub inputs: Vec<PathBuf>,
    /// A JSON manifest naming the frames explicitly, each with an optional output
    /// path and an optional partial-recipe `params` override applied on top of the
    /// shared recipe for that frame only. Mutually exclusive with positional
    /// `inputs`. Shape: `{ "frames": [ { "input": "…", "output"?: "…",
    /// "params"?: { …partial recipe… } }, … ] }`.
    #[arg(long, value_name = "JSON")]
    pub frames: Option<PathBuf>,
    /// Output directory (created if missing). Per-frame outputs are written here
    /// as `<input-stem>_positive.<ext>` — the suffix of the frame's resolved
    /// destination — unless the manifest gives an explicit output path.
    #[arg(short = 'o', long = "out-dir", value_name = "DIR")]
    pub out_dir: PathBuf,
    /// Shared frozen recipe applied to every frame (the roll-fixed film base,
    /// …). Same JSON shape as `convert --params`.
    #[arg(long = "params", value_name = "JSON")]
    pub recipe_in: Option<PathBuf>,
    /// Treat any frame's warnings as a hard error (after the roll report is
    /// emitted), like `convert --strict`.
    #[arg(long)]
    pub strict: bool,
    /// Removed: see [`reject_new_flow`].
    #[arg(long = "new-flow", hide = true)]
    pub new_flow: bool,
    #[command(flatten)]
    pub memory: MemoryArgs,
    #[command(flatten)]
    pub report: ReportArgs,
}

// --- per-stage override groups (all-Option; presence flags for booleans) ----

/// The destination (`crate::destination`): four axes, or the film master.
///
/// Each axis is optional: one left unset is derived from the destination table (its
/// default where a destination fits, else the one value left), and a combination the
/// table does not have is refused, naming what to change. The accepted values and the
/// help lists come from the axes themselves (`Axis::ALL`).
#[derive(Args, Debug, Default)]
pub struct DestinationOverrides {
    /// The dynamic range to render for: `sdr` (the default) or `hdr` (a 1000 cd/m²
    /// peak over 203 cd/m² reference white). Recipe key `output.display.range`.
    #[arg(long, value_enum, ignore_case = true, value_name = "RANGE")]
    pub range: Option<Range>,
    /// How samples are stored: `native` (the gamut's own display curve — the
    /// default), `linear` (no transfer; a 32-bit float TIFF), `pq` or `hlg` (Rec.2100
    /// signals). Recipe key `output.display.transfer`.
    #[arg(long, value_enum, ignore_case = true, value_name = "TRANSFER")]
    pub transfer: Option<Transfer>,
    /// The primaries to render into: `display-p3` (the default), `adobe-rgb` (SDR
    /// only) or `bt2020` (HDR only). Recipe key `output.display.gamut`.
    #[arg(long, value_enum, ignore_case = true, value_name = "GAMUT")]
    pub gamut: Option<Gamut>,
    /// The file container: `tiff` (the default), `jpeg` (HDR with a gain map) or
    /// `avif` (PQ/HLG only). Destinations: SDR `native` TIFF in Display P3 or Adobe RGB;
    /// HDR BT.2020 as a `linear` float TIFF, or `pq`/`hlg` in a 16-bit TIFF or a 10-bit
    /// AVIF; HDR Display P3 as a JPEG with an ISO 21496-1 gain map (`--range hdr`
    /// alone). An SDR JPEG is not written yet. Recipe key `output.display.container`.
    #[arg(long, value_enum, ignore_case = true, value_name = "CONTAINER")]
    pub container: Option<Container>,
    /// Write the fixed decode's linear ACEScg, unclamped 32-bit float TIFF, with no
    /// rendering stage (recipe `output`: `"film-master"`). Refuses a rendering stage
    /// the recipe or flags ask for (scene correction, the look, fit range), and the
    /// roll flags (`--roll-white-balance`, `--roll-white`), which only a rendering
    /// applies — refused under a recipe's film master too. A recipe's `roll` section is
    /// spared, since a measurement is not a stage asked for.
    #[arg(
        long = "film-master",
        conflicts_with_all = [
            "range", "transfer", "gamut", "container", "roll_white_balance", "roll_white",
        ]
    )]
    pub film_master: bool,
}

impl DestinationOverrides {
    /// Whether any axis flag was passed.
    pub fn any_axis(&self) -> bool {
        self.range.is_some()
            || self.transfer.is_some()
            || self.gamut.is_some()
            || self.container.is_some()
    }
}

/// Input / decode overrides (design-spec §9, stage 1).
///
/// `--input-transfer` and `--input-meaning` are the two **independent** input
/// assertions; each replaces the recipe's value on its own axis (they do not
/// conflict — they describe different facts). The legacy combined
/// `--assume-linear` is kept only to emit a migration error (it asserted both
/// axes at once), and `--input-profile` stays rejected for normal conversion.
#[derive(Args, Debug, Default)]
pub struct InputOverrides {
    /// Transfer-encoding assertion (`auto` | `linear`). Independent of
    /// `--input-meaning`: asserts how samples are encoded, not what they measure.
    #[arg(long = "input-transfer", value_enum, value_name = "TRANSFER")]
    pub input_transfer: Option<TransferAssertion>,
    /// Measurement-meaning assertion (`auto` | `scanner-device` | `colorimetric`).
    /// Only `scanner-device` + a linear transfer enters density; `colorimetric`
    /// is recognized but unsupported.
    #[arg(long = "input-meaning", value_enum, value_name = "MEANING")]
    pub input_meaning: Option<MeaningAssertion>,
    /// Deprecated: the old combined assertion. Kept only to emit a migration error
    /// — it conflated transfer and meaning. Use `--input-transfer` /
    /// `--input-meaning`.
    #[arg(long, hide = true)]
    pub assume_linear: bool,
    /// Reserved for the deferred scanner-profile-before-density experiment; not
    /// supported for normal conversion (rejected loudly). Input-side ICC
    /// application has no validated placement yet.
    #[arg(long, value_name = "ICC")]
    pub input_profile: Option<String>,
    /// Declared film chemistry (`silver` | `chromogenic` | `unknown`). Provenance
    /// only — it does **not** gate IR-assisted film-holder detection: whether IR can
    /// separate the holder from film is measured from the plane itself, because
    /// separability tracks the frame's accumulated density rather than the stock's
    /// chemistry (an unexposed silver frame separates; its own leader does not).
    /// Recipe key `input.film_type`. Kept as a shared input-medium declaration the
    /// black & white `bw-support` task and the separate IR dust-removal task still
    /// need.
    #[arg(long = "film-type", value_enum, value_name = "TYPE")]
    pub film_type: Option<FilmType>,
    /// Write the decoded IR plane to this path (HDRi only).
    #[arg(long, value_name = "PATH")]
    pub export_ir: Option<String>,
}

/// Film-base / Dmin overrides (design-spec §9, stage 2).
///
/// The three source flags are mutually exclusive (clap rejects passing more than
/// one); whichever is given replaces the recipe's `calibration.film_base` entirely.
#[derive(Args, Debug, Default)]
pub struct FilmBaseOverrides {
    /// Explicit per-channel base transmission.
    #[arg(long, value_name = "R,G,B", value_parser = parse_rgb,
          conflicts_with_all = ["base_region", "auto_base"])]
    pub film_base: Option<[f32; 3]>,
    /// Region of the unexposed border to sample.
    #[arg(long, value_name = "X,Y,W,H", value_parser = parse_region,
          conflicts_with = "auto_base")]
    pub base_region: Option<[u32; 4]>,
    /// Detect the unexposed rebate band behind the film holder. Best-effort and
    /// fails loudly when no confident band exists — real scans put a thin inset
    /// rebate *behind* the holder, not at the outer margin. **No longer the
    /// default**: `convert` requires one of these three flags **or** the
    /// `calibration.film_base` recipe key, because `Dmin` is a per-roll calibration
    /// that sets black point and colour balance together, and arriving at it by
    /// omission decided that for you. `roll` requires the same choice but takes
    /// **none of these flags** — it accepts only the recipe key, in the shared
    /// `--params` file. The measurement commands are unaffected,
    /// since they exist to produce a base: `estimate` resolves an unstated source
    /// to this, and `inspect` always runs the detector (it takes no film-base
    /// flags at all).
    #[arg(long)]
    pub auto_base: bool,
}

/// Measurement-region overrides (design-spec §9, `measure`).
#[derive(Args, Debug, Default)]
pub struct MeasureOverrides {
    /// Static border inset for measurements, as a fraction of the shorter edge
    /// (default 0.05). The effective area is two cuts in order: the film holder,
    /// measured from the IR plane where it separates, then this inset. The inset
    /// runs either way — where the holder could not be measured (no IR plane, or
    /// film too IR-opaque, which is routine for B&W) it is the only cut, and the
    /// default is sized for a rebate rather than a holder. Raise it for such a
    /// scan; the report says which case the run was in. Where the holder *was*
    /// measured the applied inset is floored at one holder-probe step (0.5% of the
    /// shorter edge), the cut's own resolution, so a stated value near 0 can report
    /// more than it asked for — `effective_area.inset` is the applied value. Never
    /// crops the image.
    #[arg(long = "measure-inset", value_name = "FRAC")]
    pub measure_inset: Option<f32>,
}

/// The fixed decode's calibration overrides (recipe section `reconstruction`,
/// `algo::fixed::DecodeParams`).
#[derive(Args, Debug, Default)]
pub struct DensityOverrides {
    /// Per-channel density gain (recipe key `reconstruction.scale`).
    #[arg(long, value_name = "R,G,B", value_parser = parse_rgb)]
    pub density_scale: Option<[f32; 3]>,
    /// Per-channel density offset (recipe key `reconstruction.offset`).
    #[arg(long, value_name = "R,G,B", value_parser = parse_rgb)]
    pub density_offset: Option<[f32; 3]>,
    /// The decode's slope: the film's linearization (recipe key
    /// `reconstruction.linearization`, default 1.8) — a calibration; how contrasty the
    /// picture is is `--contrast`.
    #[arg(long)]
    pub density_gamma: Option<f32>,
}

/// The curve selector, the `characteristic` curve's stock and the named bundles that
/// selected it, removed with that curve (`nf-retire/characteristic`). Hidden, and kept
/// only to emit a migration error — there is no alias. Each takes any value, or none,
/// so an old spelling reaches that message instead of clap's generic one.
#[derive(Args, Debug, Default)]
pub struct RemovedCharacteristicFlags {
    #[arg(long = "density-curve", hide = true, value_name = "CURVE", num_args = 0..=1, default_missing_value = "", allow_hyphen_values = true)]
    pub density_curve: Option<String>,
    #[arg(long = "film-stock", hide = true, value_name = "NAME", num_args = 0..=1, default_missing_value = "", allow_hyphen_values = true)]
    pub film_stock: Option<String>,
    #[arg(long = "preset", hide = true, value_name = "NAME", num_args = 0..=1, default_missing_value = "", allow_hyphen_values = true)]
    pub preset: Option<String>,
}

/// The regional balance's flags, removed with it (`nf-retire/regional-balance`).
/// Hidden, and kept only to emit a migration error — there is no alias. The valued
/// ones take any value, or none, so the old spellings (a negative `--shadow-balance
/// -0.05,0,0`, a bare `--balance-range`) reach that message instead of clap's generic
/// one. (A bare one followed by another valued flag still swallows that flag and hits
/// clap's error — loud, exit 2, and never a valid spelling.)
#[derive(Args, Debug, Default)]
pub struct RemovedBalanceFlags {
    #[arg(long = "shadow-balance", hide = true, value_name = "R,G,B", num_args = 0..=1, default_missing_value = "", allow_hyphen_values = true)]
    pub shadow_balance: Option<String>,
    #[arg(long = "highlight-balance", hide = true, value_name = "R,G,B", num_args = 0..=1, default_missing_value = "", allow_hyphen_values = true)]
    pub highlight_balance: Option<String>,
    #[arg(long = "balance-range", hide = true, value_name = "LO,HI", num_args = 0..=1, default_missing_value = "", allow_hyphen_values = true)]
    pub balance_range: Option<String>,
    #[arg(long = "auto-balance-range", hide = true)]
    pub auto_balance_range: bool,
}

/// The reference density's flags and the three anchor placements that read it or
/// pinned black, removed together (`nf-retire/dmax-machinery`). Hidden, and kept only
/// to emit a migration error — there is no alias. The valued ones take any value, or
/// none, so the old spellings (`--d-max`, `--d-max -1.5`) reach that message instead
/// of clap's generic one. (A bare one followed by another valued flag still swallows
/// that flag and hits clap's error — loud, exit 2, and never a valid spelling.)
#[derive(Args, Debug, Default)]
pub struct RemovedDmaxFlags {
    #[arg(long = "d-max", hide = true, value_name = "D", num_args = 0..=1, default_missing_value = "", allow_hyphen_values = true)]
    pub d_max: Option<String>,
    #[arg(long = "fixed-d-max", hide = true)]
    pub fixed_d_max: bool,
    #[arg(long = "auto-d-max", hide = true)]
    pub auto_d_max: bool,
    #[arg(long = "no-d-max", hide = true)]
    pub no_d_max: bool,
    #[arg(long = "anchor-mid-fraction", hide = true, value_name = "F", num_args = 0..=1, default_missing_value = "", allow_hyphen_values = true)]
    pub anchor_mid_fraction: Option<String>,
    #[arg(long = "anchor-white-at-reference", hide = true)]
    pub anchor_white_at_reference: bool,
    #[arg(long = "anchor-black-floor", hide = true, value_name = "FLOOR", num_args = 0..=1, default_missing_value = "", allow_hyphen_values = true)]
    pub anchor_black_floor: Option<String>,
}

/// The sigmoid curve's flags, removed with it (`nf-retire/sigmoid-and-simple`),
/// including the two `--anchor-*` aliases that kept its prefix. Hidden, and kept only
/// to emit a migration error — there is no alias.
#[derive(Args, Debug, Default)]
pub struct RemovedSigmoidFlags {
    #[arg(long, hide = true, value_name = "F")]
    pub sigmoid_contrast: Option<String>,
    #[arg(long, hide = true, value_name = "W")]
    pub sigmoid_toe: Option<String>,
    #[arg(long, hide = true, value_name = "W")]
    pub sigmoid_shoulder: Option<String>,
    #[arg(long, hide = true, value_name = "F")]
    pub sigmoid_mid_fraction: Option<String>,
    #[arg(long, hide = true)]
    pub sigmoid_white_at_d_max: bool,
}

/// The fixed decode's anchor override (recipe key `reconstruction.anchor`,
/// `algo::fixed::AnchorRule`).
#[derive(Args, Debug, Default)]
pub struct AnchorOverrides {
    /// Pin mid-grey (18%) at density D above the film base, letting display white
    /// fall where the slope puts it — the default rule, D 0.62. Reads no roll
    /// reference density, so a leader's roll-to-roll error never reaches the render.
    #[arg(long = "anchor-mid-offset", value_name = "D")]
    pub anchor_mid_offset: Option<f32>,
}

/// Which base the rendering stages start from (recipe key `rendering`,
/// `nf-destinations/direct-preset`).
#[derive(Args, Debug, Default)]
pub struct RenderingOverrides {
    /// `default` applies the roll's measurements (recipe `roll`) and every other default.
    /// `direct` loses as little as possible: no roll, neutral white balance, highlight
    /// desaturation off, and an unset destination the HDR float TIFF (else the lossless
    /// 16-bit TIFF; never a lossy one by default). A stated knob builds on either.
    #[arg(long, value_enum, value_name = "RENDERING")]
    pub rendering: Option<crate::rendering::Rendering>,
}

/// The roll's measurements, as `hanten measure-roll` reports them (recipe section
/// `roll`, `nf-calibration/roll-section`).
#[derive(Args, Debug, Default)]
pub struct RollOverrides {
    /// The roll's white-balance gains `R,G,B`, as `hanten measure-roll` measured them
    /// (recipe key `roll.white_balance`). Multiplied into `--white-balance`, which then
    /// adjusts the roll's balance rather than replacing it.
    #[arg(long, value_name = "R,G,B", value_parser = parse_rgb)]
    pub roll_white_balance: Option<[f32; 3]>,
    /// The roll's white, in scene stops above mid-grey, as `hanten measure-roll`
    /// measured it (recipe key `roll.white_stops`). The look renders it at diffuse white
    /// with mid-grey pinned — a contrast of `log2(1/0.18) / STOPS` — unless `--contrast`
    /// is stated, which wins.
    #[arg(long, value_name = "STOPS")]
    pub roll_white: Option<f32>,
}

impl RollOverrides {
    /// Whether any roll flag was typed.
    pub(crate) fn any(&self) -> bool {
        self.roll_white_balance.is_some() || self.roll_white.is_some()
    }
}

/// Scene-correction overrides (recipe section `scene_correction`).
#[derive(Args, Debug, Default)]
pub struct SceneCorrectionOverrides {
    /// White-balance gains `R,G,B` — a per-channel gain on linear ACEScg, after the
    /// NC film RGB v1 mapping (recipe key `scene_correction.white_balance`). Multiplied
    /// into the roll's gains (`--roll-white-balance`, which `hanten measure-roll`
    /// measures), so it adjusts the roll's balance rather than replacing it.
    #[arg(long, value_name = "R,G,B", value_parser = parse_rgb)]
    pub white_balance: Option<[f32; 3]>,
    /// Exposure in stops (EV) — a scene-referred gain of `2^EV` on every channel,
    /// before the look and the display fit (recipe key `scene_correction.exposure`).
    /// Stops of the reconstructed scene: the look's `--contrast` then expands them with
    /// the rest of the picture.
    #[arg(long, allow_hyphen_values = true)]
    pub exposure: Option<f32>,
}

/// Look overrides (recipe section `look`).
#[derive(Args, Debug, Default)]
pub struct LookOverrides {
    /// Print contrast, pivoted at mid-grey: every ACEScg channel becomes
    /// `0.18 · (v / 0.18)^CONTRAST` (recipe key `look.contrast`; 1 is the identity).
    /// Unstated, it is the roll's (`--roll-white`), else 2.0/1.8 ≈ 1.11, which with the
    /// decode's linearization reproduces the pre-split contrast 2.0. Stated, it wins
    /// over the roll's. Runs after scene correction, so an `--exposure` is expanded
    /// with the rest of the picture.
    #[arg(long, value_name = "CONTRAST", allow_hyphen_values = true)]
    pub contrast: Option<f32>,
    /// The per-channel grade `R,B`: red and blue exponents pivoted at mid-grey, green
    /// fixed at 1, with each pixel's ACEScg luminance restored afterwards — a cast that
    /// grows away from mid in both directions without moving neutral contrast (recipe
    /// key `look.channel_grade`, default `1,1`, the identity). Both positive, with the
    /// spread over `R,1,B` under 1. Runs after `--contrast`.
    #[arg(
        long = "channel-grade",
        value_name = "R,B",
        value_parser = parse_lo_hi,
        allow_hyphen_values = true
    )]
    pub channel_grade: Option<[f32; 2]>,
    /// Highlight desaturation's strength, in [0, 1]: how far a bright, near-neutral
    /// pixel is pulled toward neutral (recipe key
    /// `look.highlight_desaturation.strength`; unstated, 0.8, and off under `--rendering
    /// direct`; 0 is off). It keys on
    /// brightness and on distance from the neutral axis, so coloured highlights keep
    /// their colour; it assumes the roll's white balance (`hanten measure-roll`).
    #[arg(
        long = "highlight-desaturation",
        value_name = "STRENGTH",
        allow_hyphen_values = true
    )]
    pub highlight_desaturation: Option<f32>,
    /// Where highlight desaturation starts, in stops relative to diffuse white
    /// (negative; recipe key `look.highlight_desaturation.start_stops`, default -1).
    #[arg(
        long = "highlight-desaturation-start",
        value_name = "STOPS",
        allow_hyphen_values = true
    )]
    pub highlight_desaturation_start: Option<f32>,
    /// Highlight desaturation's saturation band `S0,S1`: full pull at or below `S0`,
    /// none at or above `S1`, on `log10(max/min)` of the pixel's ACEScg channels over
    /// the whole contrast, `--density-gamma` times `--contrast` (recipe key `look.highlight_desaturation.band`, default
    /// `0.015,0.025`).
    #[arg(
        long = "highlight-desaturation-band",
        value_name = "S0,S1",
        value_parser = parse_lo_hi,
        allow_hyphen_values = true
    )]
    pub highlight_desaturation_band: Option<[f32; 2]>,
}

/// Fit-range overrides (recipe section `fit_range`).
#[derive(Args, Debug, Default)]
pub struct DisplayOverrides {
    /// Specular headroom above reference white for the display tone, in stops, default
    /// 6 (a white point of 64; recipe key `fit_range.headroom_stops`). The tone is
    /// extended Reinhard, which compresses the whole curve against that white point
    /// while holding mid-grey where it is, so raising the headroom changes the
    /// highlights without darkening the midtones. `0` is the exact identity. Rendered
    /// destinations only; the film master applies no display tone.
    // A negative headroom must reach the value check, whose message names this very
    // flag: without this, clap refused `-1` as "unexpected argument".
    #[arg(
        long = "display-tone-headroom",
        value_name = "STOPS",
        allow_hyphen_values = true
    )]
    pub display_tone_headroom: Option<f32>,
    /// Display black: where the film base — the darkest thing on the film — renders,
    /// in stops below mid-grey on the display, or `off` (recipe key
    /// `fit_range.display_black`, default 6, about L* 2.5). Fewer stops give lighter
    /// shadows with more detail, more stops a deeper black. Display stops, not scene
    /// stops: where the base lands before this is set by the look's contrast, and a
    /// base already that deep is left alone. Mid-grey and everything above it do not
    /// move.
    #[arg(
        long = "display-black",
        value_name = "STOPS|off",
        allow_hyphen_values = true
    )]
    pub display_black: Option<crate::pipeline::fit_range::DisplayBlack>,
}

/// The removed chain's print controls (`nf-core/default-flip`), and the two display
/// tones' flags removed before them (`nf-retire/display-tones`). Hidden, and kept only
/// to emit a migration error naming what replaced each — there is no alias. Each takes
/// any value, or none, so an old spelling reaches that message instead of clap's
/// generic one.
#[derive(Args, Debug, Default)]
pub struct RemovedPrintFlags {
    #[arg(long, hide = true, value_name = "EV", num_args = 0..=1, default_missing_value = "", allow_hyphen_values = true)]
    pub print_exposure: Option<String>,
    #[arg(long, hide = true, value_name = "F", num_args = 0..=1, default_missing_value = "", allow_hyphen_values = true)]
    pub black_point: Option<String>,
    #[arg(long = "auto-wb", hide = true, value_name = "MODE", num_args = 0..=1, default_missing_value = "", allow_hyphen_values = true)]
    pub auto_wb: Option<String>,
    #[arg(long = "linear-range", hide = true, value_name = "LOW,HIGH", num_args = 0..=1, default_missing_value = "", allow_hyphen_values = true)]
    pub linear_range: Option<String>,
    #[arg(long = "display-tone", hide = true, value_name = "MODE")]
    pub display_tone: Option<String>,
    #[arg(long, hide = true, value_name = "AMOUNT", allow_hyphen_values = true)]
    pub highlight_compress: Option<String>,
}

/// Removed simple-reconstruction controls. Hidden, and kept only to emit a migration
/// error (nc is unreleased — no aliases).
#[derive(Args, Debug, Default)]
pub struct SimpleOverrides {
    #[arg(long, hide = true, value_name = "R,G,B")]
    pub invert_white_balance: Option<String>,
    #[arg(long, hide = true, value_name = "F")]
    pub clip_low: Option<String>,
    #[arg(long, hide = true, value_name = "F")]
    pub clip_high: Option<String>,
}

/// The removed chain's output selectors: the preset, and the depth, profile and
/// container selectors that retired before it with the `legacy` and `custom` presets.
/// Hidden, and kept only to emit a migration error — there is no alias. A destination
/// is chosen with [`DestinationOverrides`].
#[derive(Args, Debug, Default)]
pub struct RemovedOutputFlags {
    #[arg(long = "output-preset", hide = true, value_name = "PRESET", num_args = 0..=1, default_missing_value = "")]
    pub output_preset: Option<String>,
    #[arg(long = "out-depth", hide = true, value_name = "DEPTH")]
    pub out_depth: Option<String>,
    #[arg(long, hide = true)]
    pub output_hdr: bool,
    #[arg(long, hide = true)]
    pub output_sdr: bool,
    #[arg(long, hide = true, value_name = "PROFILE")]
    pub output_profile: Option<String>,
    #[arg(long, hide = true, value_name = "POLICY")]
    pub bigtiff: Option<String>,
}

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------

/// The reuse-ready forms of a measured film base, kept as one unit so the flag
/// and the recipe value are both-present-or-both-absent — the illegal
/// flag-without-recipe (or recipe-without-flag) state two parallel `Option`s
/// would permit is unrepresentable.
///
/// **The pairing is per measurement, not per section.** The flag half becomes the
/// flat report key `film_base_flag`; the recipe half is copied into the report's
/// [`CalibrationFragment`]. A future calibration value measured across many frames
/// (a roll content white) would have no flag form at all, which a section-wide
/// both-present rule would forbid. Serialize-only.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReuseReady {
    /// Ready-to-paste `--film-base R,G,B` flag for the measured base; the values
    /// round-trip to the exact measured `f32`s.
    #[serde(rename = "film_base_flag")]
    pub flag: String,
    /// The same measurement as the `calibration.film_base` value —
    /// `{"explicit":[r,g,b]}` — ready to merge into a roll recipe. Not serialized
    /// here: it is emitted once, inside [`Report::calibration`].
    #[serde(skip)]
    pub source: FilmBaseSource,
}

/// The report's `calibration` object: the calibration values this invocation
/// resolved, in exactly the recipe shape, so
/// `hanten estimate … | jq '{recipe_version: 2, calibration}' > roll-cal.json` writes
/// a reusable roll calibration with nothing to hand-edit.
///
/// `core/measure-base` replaces this `jq` step with a recipe file the command writes
/// (`docs/design/roll-workflow.md`).
///
/// **Partial by construction, and that is load-bearing.** It is not the recipe's
/// [`recipe::Calibration`]: every member is skipped when absent, so a run that measured
/// nothing pins nothing over a later `--params` layer, and a member is added by adding
/// one field here — the section is open (see [`recipe::Calibration`]), so nothing may
/// assume the set is exactly the one member it has today.
///
/// It reports *what was measured here*, never "the roll's calibration": a complete
/// one may need several invocations, and a future member is measured across many
/// frames rather than from one. Assembling it is `core/measure-base`.
#[derive(Clone, Debug, PartialEq, Default, Serialize)]
pub struct CalibrationFragment {
    /// The measured base as `calibration.film_base` — present exactly when
    /// `film_base_flag` is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub film_base: Option<FilmBaseSource>,
}

impl CalibrationFragment {
    /// Whether anything was measured; an empty fragment is omitted from the report
    /// rather than emitted as `{}`, which would pipe into `--params` as a no-op the
    /// user could mistake for a calibration.
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// What the AVIF encoder coded, for the resolved report. Serialize-only.
///
/// Every field **except `rendering`** is read back out of the produced file rather
/// than restated from the request, so the report is evidence about the artifact and
/// not an echo of the configuration. In particular `profile` records whether the file
/// may claim the AVIF v1.2 Advanced Profile, and `profile_reason` says why not when
/// it may not — a general-brand-only file is a legitimate output, but never a silent
/// one.
///
/// `rendering` is the deliberate exception, and it is nested rather than flattened so
/// the distinction survives: those are the luminance semantics **no** AVIF box can
/// state, so they can only come from the renderer. Keeping them in their own object
/// means a reader can tell at a glance which half of this block is evidence about
/// bytes and which half is declared policy.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AvifResult {
    /// `"advanced"` when the `MA1A` brand was written, else `"general-brand-only"`.
    pub profile: &'static str,
    /// Which published limit put the file outside the Advanced Profile. Absent when
    /// `profile` is `"advanced"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_reason: Option<String>,
    /// Coded bit depth (always 10 in this build).
    pub bit_depth: u8,
    /// AV1 `seq_profile` parsed from the codestream (1 = High, required for 4:4:4).
    pub seq_profile: u8,
    /// AV1 `seq_level_idx` parsed from the codestream. 16 is level 6.0, the
    /// Advanced Profile ceiling.
    pub seq_level_idx: u8,
    /// Human-readable level, e.g. `"2.0"`, derived from `seq_level_idx`.
    pub level: String,
    /// CICP colour primaries / transfer / matrix coefficients as coded.
    pub cicp: [u8; 3],
    /// Whether full-range coding was signalled.
    pub full_range: bool,
    /// Size of the AV1 codestream in bytes, excluding container boxes.
    pub codestream_bytes: usize,
    /// The rendering policy behind the pixels — see the type's own note.
    pub rendering: AvifRenderingResult,
}

/// The luminance and tone semantics of an AVIF rendition, which the container cannot
/// express. Serialize-only.
///
/// CICP names the transfer function but not what diffuse white *is*: PQ's curve is
/// absolute, yet nothing in the file says nc anchors reference white at 203 cd/m² and
/// masters to a 1000 cd/m² peak, and for HLG — display-referred — no box could. So a
/// consumer deciding how to tone-map these files has the same problem the coded and
/// linear TIFF blocks already solve by stating it, and this states it the same way.
///
/// `tone_curve` is the renderer's **pinned identifier**, straight from its metadata —
/// the same pairing the coded- and linear-TIFF blocks carry: two AVIFs with
/// byte-identical `cicp`, `profile` and `level` can hold materially different
/// renditions, and the artifact block should be self-sufficient about which.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct AvifRenderingResult {
    /// Reference white in cd/m² (the binding 203).
    pub reference_white_nits: f32,
    /// Mastering target peak in cd/m² (the binding 1000).
    pub target_peak_nits: f32,
    /// The display-linear value that represents `target_peak_nits` (≈4.926108) — the
    /// largest the renderer produces, before the transfer function encodes it.
    pub linear_headroom: f32,
    /// Which display tone curve produced these pixels, straight from the renderer's
    /// metadata.
    pub tone_curve: &'static str,
    /// Pinned gamut-mapping and linear-domain identifiers, from the renderer.
    pub gamut_mapping: &'static str,
    pub linear_domain: &'static str,
    /// HLG's reference-display assumptions, absent for PQ. Mirrors the coded-TIFF
    /// block; unlike that one, the measured content-light values are omitted here
    /// because for AVIF they are in the file's own `clli` box for PQ, and omitted by
    /// design for HLG.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hlg_system_gamma: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hlg_reference_display_peak_nits: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hlg_reference_display_black_nits: Option<f32>,
}

/// What the `hdr-linear-tiff` encoder wrote, and the luminance semantics the file
/// cannot state for itself. Serialize-only.
///
/// **This block is authoritative for the HDR semantics, and deliberately so.** The
/// embedded ICC profile describes the colorimetry (BT.2020 primaries, D65, a linear
/// TRC) but its PCS stops at the media white, so no v4 profile can express that
/// `1.0` is 203 cd/m² and that highlights legitimately run to
/// `linear_headroom`. Anything consuming these files for luminance must read this,
/// not the profile — `interoperability` says so in the artifact itself rather than
/// leaving it to documentation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct HdrLinearTiffResult {
    /// Stable identifier of the pixel contract
    /// ([`encode::HDR_LINEAR_PIXEL_CONTRACT`]).
    pub pixel_contract: &'static str,
    /// Bits per sample as written (32).
    pub bits_per_sample: u16,
    /// TIFF `SampleFormat` as written (3 = IEEE float).
    pub sample_format: u16,
    /// Whether the file was written as BigTIFF.
    pub bigtiff: bool,
    /// Size of the embedded linear-BT.2020 ICC profile, in bytes.
    pub icc_bytes: usize,
    /// The sample value that represents diffuse reference white, always `1.0`.
    pub reference_white_sample: f32,
    /// Reference white in cd/m² (the binding 203).
    pub reference_white_nits: f32,
    /// Mastering target peak in cd/m² (the binding 1000).
    pub target_peak_nits: f32,
    /// The sample value that represents `target_peak_nits` (≈4.926108) — the
    /// largest value the renderer will produce, and the reason this output cannot
    /// be a 16-bit integer TIFF.
    pub linear_headroom: f32,
    /// Pinned tone-curve / gamut-mapping / linear-domain identifiers, straight from
    /// the renderer's own metadata rather than restated here.
    pub tone_curve: &'static str,
    pub gamut_mapping: &'static str,
    pub linear_domain: &'static str,
    /// This frame's **measured** light levels in cd/m² — peak and frame-average
    /// pixel luminance, not the mastering policy above.
    pub max_cll_nits: u16,
    pub max_fall_nits: u16,
    /// Plain statement of what the file alone does and does not communicate.
    pub interoperability: &'static str,
}

/// What the `hdr-pq-tiff` / `hdr-hlg-tiff` encoder wrote: the signalling contract,
/// the one quantization step's measured cost, and the honest limits of the file.
/// Serialize-only.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct HdrCodedTiffResult {
    /// Stable identifier of the pixel contract (see `io::encode`).
    pub pixel_contract: &'static str,
    /// Bits per sample as written (16).
    pub bits_per_sample: u16,
    /// TIFF `SampleFormat` as written (1 = unsigned integer).
    pub sample_format: u16,
    /// Whether the file was written as BigTIFF.
    pub bigtiff: bool,
    /// Size of the embedded ICC profile in bytes.
    pub icc_bytes: usize,
    /// The CICP triple the embedded profile's `cicp` tag declares, as
    /// `[ColourPrimaries, TransferCharacteristics, MatrixCoefficients]`.
    ///
    /// **MatrixCoefficients is 0 here and 9 in the `avif` block for the same
    /// rendition**, and that is required rather than inconsistent:
    /// ICC.1:2022 §10.3 mandates 0 for an RGB data space, while AVIF stores
    /// Y'CbCr.
    pub cicp: [u8; 3],
    /// Whether full-range coding is signalled (always `true`).
    pub full_range: bool,
    /// Largest quantization error over the frame, in code units. At most `0.5` by
    /// construction — rounding cannot be worse than half a step.
    pub max_quantization_error_codes: f32,
    /// Root-mean-square quantization error over the frame, in code units.
    pub rms_quantization_error_codes: f32,
    /// Reference white in cd/m² (203) and the mastering peak (1000).
    pub reference_white_nits: f32,
    pub target_peak_nits: f32,
    /// Which display tone curve produced these pixels, straight from the renderer's
    /// own metadata — the same identifier `hdr_linear_tiff` reports.
    pub tone_curve: &'static str,
    /// This frame's **measured** peak and average light levels in cd/m², for PQ.
    ///
    /// Present only for PQ, mirroring the `clli` box `io::avif` writes for the same
    /// rendition: the values are absolute luminance, which HLG — being
    /// display-referred — cannot state. TIFF has no `clli` equivalent, so without
    /// these fields the measurement `pipeline::hdr::render_linear` took would be lost
    /// from both the file and the report, leaving a consumer tone-mapping this image
    /// with no way to learn its actual peak.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_cll_nits: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_fall_nits: Option<u16>,
    /// HLG's reference-display assumptions, absent for PQ.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hlg_system_gamma: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hlg_reference_display_peak_nits: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hlg_reference_display_black_nits: Option<f32>,
    /// What the file does and does not establish, stated in the artifact.
    pub interoperability: &'static str,
}

/// What a conversion ran: the fixed decode's resolved parameters, what each stage of
/// the chain applied, and the destination. Serialize-only.
///
/// Every field is a fact read off the resolved chain, never prose: a stage that moved
/// no pixel says `"identity"` rather than being left out.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ChainResult {
    /// The fixed decode's resolved parameters, as the render used them.
    pub decode: fixed::DecodeReport,
    /// Each stage of the chain in order, with what it applied. Empty for the film
    /// master, which runs none.
    pub stages: Vec<StageResult>,
    /// The rendering the stages started from (`--rendering`, `crate::rendering`).
    pub rendering: crate::rendering::Rendering,
    /// The recipe's `roll` section and what the run applied of it; absent when the
    /// section states nothing. The film master and `--rendering direct` apply neither
    /// value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roll: Option<recipe::RollReport>,
    /// Scene correction's values: the white-balance gains and the exposure applied — the
    /// roll's gains included. Absent for the film master.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene_correction: Option<scene_correction::SceneCorrection>,
    /// The look's controls as applied — the contrast, and highlight desaturation's
    /// strength, start and band. Absent for the film master.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub look: Option<look::LookSection>,
    /// Fit range's operator by name, with the headroom, white point and display peak
    /// it ran at — what a non-default `fit_range.headroom_stops` changes. Absent for
    /// the film master.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fit_range: Option<fit_range::FitRange>,
    /// The destination written, every axis resolved — the recipe `output` section that
    /// replays it exactly (`crate::destination`).
    pub destination: OutputSection,
    /// What fitting an HDR rendition to its peak clamped, per sample. Counted into
    /// `loss` too; absent for an SDR destination and the film master.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_clamp: Option<hdr::PeakClamp>,
    /// The gain map the gain-map destination wrote. Absent for every other destination.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gain_map: Option<GainMapResult>,
    /// A sidecar a pre-`pipeline_version` 8 run left beside this output, removed
    /// because it described the image this run replaced. Absent when there was none;
    /// no run writes a sidecar now.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed_sidecar: Option<PathBuf>,
}

/// The gain map a gain-map destination wrote (`nf-destinations/gain-map-destination`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct GainMapResult {
    /// The per-channel gain extent, linear, at full resolution before quantization —
    /// and `flat`: every gain within rounding of `1` (`gain_ratio::FLAT_TOLERANCE_LOG2`,
    /// 1/510 stop), so the map is written inert and the file displays as its SDR base
    /// everywhere. A flat map is a fact about the frame (nothing above diffuse white,
    /// no colour the SDR cube binds), stated here rather than warned about.
    #[serde(flatten)]
    pub range: gain_ratio::GainRange,
    /// The stored map's size: half the frame's, rounded up.
    pub width: u32,
    pub height: u32,
    /// Fit range as the SDR base ran it. `fit_range` beside this block is the HDR
    /// rendition's; the two differ only in the peak.
    pub base_fit_range: fit_range::FitRange,
}

/// One stage of the chain and what it applied under the run's parameters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct StageResult {
    pub stage: StageKind,
    pub applied: &'static str,
}

/// Machine-readable result emitted on stdout (or `--report-file`). One shape
/// serves all three commands; irrelevant fields are `None`/empty and omitted
/// from the JSON (`skip_serializing_if`), so an agent gets a clean object per
/// command. Serialize-only — it embeds the serialize-only `DecodeInfo` /
/// `EncodeReport`, and nothing deserializes a report.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Report {
    /// The subcommand that produced this report (`convert`/`inspect`/`estimate`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<&'static str>,
    /// What produced this output: build identity (`nc_version`, `git_commit`,
    /// `git_dirty`, `target`) and the behavioral `pipeline_version`
    /// (`core/conversion-versioning`). Purely operational provenance: it has no CLI
    /// flag and no recipe key, and never perturbs an output pixel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<Identity>,
    /// Input scan path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<PathBuf>,
    /// Output image path, when one was written (`convert`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<PathBuf>,
    /// The pinned working-space mapping this conversion interprets the
    /// reconstructed film RGB under (`convert`): always `"nc-film-rgb-v1"`
    /// (linear Rec.709/D65 → linear ACEScg/D60; see
    /// `pipeline::working_space`). Provenance only — the mapping is a fixed
    /// constant, not a tunable knob, so it has no CLI flag / recipe key
    /// (design-spec §8). A future mapping is a *new* identifier under
    /// `conversion-versioning`, never a silent change to v1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_mapping: Option<&'static str>,
    /// What the conversion ran (`convert` and each `roll` frame): the decode, each
    /// stage and the destination. See [`ChainResult`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain: Option<ChainResult>,
    /// The resolved recipe (`convert`): what `--dump-params` would write, so it
    /// reloads through `--params` to this run. `identity.params_hash` hashes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipe: Option<Recipe>,
    /// What the AVIF encoder actually coded (a PQ or HLG AVIF destination): the
    /// profile the file may claim and why, the AV1 profile/level read back out of the
    /// codestream, the CICP triple, and the coded size. Absent for every other
    /// destination. Provenance for the conformance claim — an agent can check the
    /// brand decision without re-parsing the container.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avif: Option<AvifResult>,
    /// What the linear HDR TIFF encoder wrote (`--transfer linear`), and the
    /// reference-white / peak / headroom semantics the embedded ICC cannot carry.
    /// Absent for every other destination. **Authoritative** for this output's
    /// luminance meaning — see [`HdrLinearTiffResult`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hdr_linear_tiff: Option<HdrLinearTiffResult>,
    /// What the PQ / HLG TIFF encoder wrote: the CICP signalling, the measured
    /// quantization cost, and the documented interoperability limits. Absent for every
    /// other destination.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hdr_coded_tiff: Option<HdrCodedTiffResult>,
    /// What the decoder found (`inspect`): format, dimensions, channels, bit
    /// depth, IR presence, scanner metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decode: Option<DecodeInfo>,
    /// What the memory preflight decided before this run decoded anything: the
    /// estimated peak with its per-phase breakdown, the budget and where it came
    /// from, the verdict, and the detected RAM the warn tier used
    /// (`pipeline::memory`). Present on every command that decodes a scan.
    /// Operational provenance — the budget is not a recipe key and never
    /// influences the pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryReport>,
    /// Resolved input color semantics (`convert`/`inspect`): the two independent
    /// axes (transfer encoding + measurement meaning) with per-axis evidence,
    /// whether an ICC is embedded plus a safe summary, and whether any transfer
    /// decoding was performed. `convert` only reaches the render once this
    /// resolves to a supported linear + scanner-device input; `inspect` reports it
    /// even when the input is ambiguous or unsupported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_color: Option<InputColorReport>,
    /// Estimated / resolved film base (the `Dmin` anchor).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub film_base: Option<FilmBase>,
    /// The calibration values this invocation measured, in recipe shape
    /// (`estimate`) — see [`CalibrationFragment`]. Absent when nothing measured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calibration: Option<CalibrationFragment>,
    /// How the film base was chosen, as the structured [`FilmBaseSource`]
    /// (`"auto"` / `{"region":[…]}` / `{"explicit":[…]}`) so an agent gets the
    /// sampled rectangle / explicit values without string-parsing a label.
    /// For `estimate --grid` this is the overall rectangle the grid sampled
    /// (`{"region":[…]}`); the `grid` field documents the per-cell method.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub film_base_source: Option<FilmBaseSource>,
    /// Candidate unexposed-rebate bands from the inward-scan detector
    /// (`inspect` only): edge, a rectangle usable verbatim as `--base-region`,
    /// the proposed base, and the measured spread (lower = more uniform). Lets
    /// a user confirm a region instead of measuring one in an image viewer —
    /// and a future UI draws its highlight rectangles from the same data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_candidates: Option<Vec<film_base::RebateCandidate>>,
    /// The declared film chemistry, echoed back. It gates nothing
    /// (`ir-usability-detection`); it is recorded so a declaration a user made is
    /// visible in the artifact the run produced — without it the flag would be parsed
    /// and dropped, accepted-and-ignored, which this project treats as a bug.
    /// `inspect` / `estimate` echo the `--film-type` flag; `convert` (and each `roll`
    /// frame, as `FrameStatus::Ok::film_type`) echo the resolved recipe's
    /// `input.film_type`. Omitted for `unknown` (the default), even when stated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub film_type: Option<FilmType>,
    /// The measured IR usability verdict (`inspect` / `estimate`, only on a scan
    /// carrying an IR plane): the interior IR transmission and whether it clears the bar for
    /// telling the opaque holder from film on **this frame**. Reported so the
    /// verdict — and the threshold behind it — is falsifiable from a run rather
    /// than only from the source (`ir-usability-detection`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ir_separability: Option<film_base::IrSeparability>,
    /// IR film-holder classification per edge (`inspect`, on a scan whose IR plane
    /// is marker-verified and measures usable): which along-edge
    /// segments the opaque holder occludes (dark in IR) vs actual film (bright).
    /// Holder segments are excluded from the rebate search; a fully-film or
    /// fully-holder edge is the all-segments-agree case. RGB alone cannot make
    /// this call — holder and dense film are both dark in RGB.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holder_mask: Option<Vec<film_base::EdgeHolderMask>>,
    /// The resolved **effective measurement area** (every command that decodes —
    /// `inspect`, `estimate`, `convert`, and each `roll` frame via
    /// `FrameStatus::Ok`): the rectangle a measurement may be read over, after the
    /// IR-measured holder cut and the static inset. Reported so both cuts are
    /// falsifiable from a run —
    /// `holder: null` means the holder was *not measured* (no IR, shape-only, or
    /// not separable here), while all-zero depths mean it was measured and there is
    /// none. `inset` is the **applied** inset, which the holder cut's resolution
    /// floors at one probe step wherever the march ran. The image is never cropped;
    /// this is where statistics are read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_area: Option<film_base::EffectiveArea>,
    /// Reuse-ready forms of the measured base (`estimate`): a ready-to-paste
    /// `--film-base R,G,B` flag and the matching `film_base` recipe fragment, so
    /// the calibrate-once → reuse workflow (design-spec §8) is copy-paste smooth.
    /// Both forms are present together or both absent — the pair only exists when
    /// the measurement is usable as an explicit base (each channel in `(0, 1]`),
    /// so a single [`ReuseReady`] (both-or-neither) replaces two parallel
    /// `Option`s that could encode the illegal flag-without-recipe state. Flattened
    /// so the flag stays a flat top-level key (`film_base_flag`) on the wire; the
    /// recipe half is emitted inside [`Self::calibration`]; `None` emits neither.
    #[serde(flatten)]
    pub reuse: Option<ReuseReady>,
    /// Grid-sampling result (`estimate --grid`): the per-cell values, their
    /// per-channel spread, the agreement tolerance and verdict. Disagreement
    /// additionally lands in `warnings` (and fails under `--strict`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid: Option<film_base::GridEstimate>,
    /// Path the IR plane was exported to, when `--export-ir` was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ir_exported: Option<PathBuf>,
    /// Encode-time sample loss (clipped / non-finite counts), for `convert`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loss: Option<EncodeReport>,
    /// Per-channel mean of the samples as written (`convert`) — the comparison
    /// basis `nctool compare` diffs across two builds (per-channel mean ΔRGB is
    /// the difference of these means). Report-only, like `loss`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_stats: Option<OutputStats>,
    /// Non-fatal warnings (clipping, IR-ignored, BigTIFF auto-promote, …).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Wall-clock time in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<f64>,
}

// ---------------------------------------------------------------------------
// Value parsers (comma lists)
// ---------------------------------------------------------------------------

/// Parse `R,G,B` into three `f32`s.
fn parse_rgb(s: &str) -> std::result::Result<[f32; 3], String> {
    let v = parse_floats::<3>(s)?;
    Ok(v)
}

/// Parse `LO,HI` into two `f32`s.
fn parse_lo_hi(s: &str) -> std::result::Result<[f32; 2], String> {
    parse_floats::<2>(s)
}

/// Parse `X,Y,W,H` into four `u32`s.
fn parse_region(s: &str) -> std::result::Result<[u32; 4], String> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 4 {
        return Err(format!(
            "expected X,Y,W,H (4 comma-separated integers), got `{s}`"
        ));
    }
    let mut out = [0u32; 4];
    for (i, p) in parts.iter().enumerate() {
        out[i] = p
            .trim()
            .parse()
            .map_err(|_| format!("`{}` is not a non-negative integer in `{s}`", p.trim()))?;
    }
    Ok(out)
}

/// Parse exactly `N` comma-separated floats.
fn parse_floats<const N: usize>(s: &str) -> std::result::Result<[f32; N], String> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != N {
        return Err(format!("expected {N} comma-separated numbers, got `{s}`"));
    }
    let mut out = [0f32; N];
    for (i, p) in parts.iter().enumerate() {
        out[i] = p
            .trim()
            .parse()
            .map_err(|_| format!("`{}` is not a number in `{s}`", p.trim()))?;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Recipe load / merge / validate (pure, unit-tested without the pipeline)
// ---------------------------------------------------------------------------

/// A loaded recipe, and the `pipeline_version` an enveloped document records for the
/// build that produced it.
#[derive(Debug)]
struct LoadedRecipe {
    recipe: Recipe,
    /// `meta.pipeline_version` from an envelope, when the loaded file carried one.
    /// Provenance only — never applied, only compared (see
    /// [`pipeline_version_warning`]).
    meta_pipeline_version: Option<u32>,
}

/// The read side of the envelope `{ "meta": {…identity…}, "params": {…recipe…} }` a
/// sidecar carries: identity beside the recipe, never inside it, since every recipe
/// struct is `deny_unknown_fields`. `meta` is kept as a raw `Value` on purpose: it is
/// provenance, so an older build must not reject a newer build's extra `meta` fields,
/// and nothing in it may influence the conversion. `params` is likewise raw here so the
/// *identical* body checks (migration errors, the typed `deny_unknown_fields` parse)
/// apply to an enveloped and a bare recipe alike. `deny_unknown_fields` at this level
/// keeps a third sibling key from being silently ignored.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SidecarEnvelopeIn {
    #[serde(default)]
    meta: Option<serde_json::Value>,
    params: serde_json::Value,
}

/// Load a recipe file, or the defaults when no recipe is given. A read failure or
/// invalid/unknown-key JSON is a usage error; a document written for the removed chain,
/// and keys that retired before it, get migration errors naming where each knob went
/// ([`recipe::check_body`]) rather than opaque serde messages.
///
/// Accepts **both** shapes: the envelope `{ "meta": …, "params": {…recipe…} }` —
/// identity read for provenance and otherwise ignored — and a bare recipe object (a
/// hand-written recipe, or `--dump-params` output). The two are told apart by the
/// presence of a top-level `params` key, which is not (and must never become) a recipe
/// key.
fn load_recipe(path: Option<&Path>) -> Result<LoadedRecipe> {
    let Some(p) = path else {
        return Ok(LoadedRecipe {
            recipe: Recipe::default(),
            meta_pipeline_version: None,
        });
    };
    let txt = std::fs::read_to_string(p)
        .map_err(|e| NcError::Usage(format!("cannot read recipe {}: {e}", p.display())))?;
    // Parse to a raw Value first to pick the shape and to run the migration checks on
    // the recipe *body*; the typed parse below still owns shape and unknown-key
    // validation. Unparseable JSON falls through to the typed parse's error (its
    // message names the recipe).
    let value: Option<serde_json::Value> = serde_json::from_str(&txt).ok();
    let context = format!("recipe {}", p.display());
    // A recipe (or an envelope) is an OBJECT. serde's derived visitor accepts a
    // sequence for a struct, so a bare `[]` would otherwise reach the typed parse with
    // a message about a sequence rather than about the document.
    if let Some(v) = &value
        && !v.is_object()
    {
        return Err(NcError::Usage(format!(
            "{context}: a recipe must be a JSON object, got {}",
            json_kind(v)
        )));
    }
    let (envelope_body, meta_pipeline_version) = match split_envelope(value.as_ref(), &context)? {
        Some((body, meta_version)) => (Some(body), meta_version),
        None => (None, None),
    };
    // The recipe *body*: an envelope's `params`, else the whole document.
    if let Some(v) = envelope_body.as_ref().or(value.as_ref()) {
        recipe::check_body(v, true, &context)?;
    }
    let usage = |e| NcError::Usage(format!("invalid recipe {}: {e}", p.display()));
    // Bare recipes parse straight from the file text, so their (line/column-bearing)
    // serde diagnostics survive.
    let recipe = match envelope_body {
        Some(v) => serde_json::from_value(v).map_err(usage)?,
        None => serde_json::from_str(&txt).map_err(usage)?,
    };
    Ok(LoadedRecipe {
        recipe,
        meta_pipeline_version,
    })
}

/// Split a loaded document into `(recipe body JSON, meta.pipeline_version)` when it
/// is a sidecar envelope; `None` when it is a bare recipe (a hand-written or
/// `--dump-params` document) and the caller should use the file text as-is.
///
/// A document carrying `meta` but no `params` is a *malformed* envelope, not a bare
/// recipe: it gets a pointed error rather than the opaque `unknown field 'meta'`
/// serde would produce.
///
/// `params` must be a JSON **object**. serde's derived visitor happily accepts a
/// *sequence* for a struct, so `{"params": []}` would otherwise reach the typed parse
/// with a message about a sequence rather than about the envelope — a truncated or
/// mis-generated document should be named as one.
fn split_envelope(
    value: Option<&serde_json::Value>,
    context: &str,
) -> Result<Option<(serde_json::Value, Option<u32>)>> {
    let Some(obj) = value.and_then(|v| v.as_object()) else {
        return Ok(None);
    };
    if !obj.contains_key("params") {
        if obj.contains_key("meta") {
            return Err(NcError::Usage(format!(
                "{context}: has a `meta` block but no `params` — a sidecar envelope \
                 is `{{\"meta\": {{…}}, \"params\": {{…recipe…}}}}`; a bare recipe \
                 object must not contain `meta`"
            )));
        }
        return Ok(None);
    }
    let envelope: SidecarEnvelopeIn = serde_json::from_value(value.unwrap().clone())
        .map_err(|e| NcError::Usage(format!("{context}: invalid sidecar envelope: {e}")))?;
    if !envelope.params.is_object() {
        return Err(NcError::Usage(format!(
            "{context}: sidecar `params` must be a recipe OBJECT, got {}. A non-object \
             `params` would convert with all-default parameters instead of the recipe \
             this file claims to carry",
            json_kind(&envelope.params)
        )));
    }
    // `meta`, when the document has the key at all, must be an OBJECT. Checked
    // against the raw JSON rather than `envelope.meta`, because serde folds
    // `"meta": null` into the same `None` an omitted key produces — and an omitted
    // `meta` is legal (a bare `--dump-params` recipe wrapped by hand).
    //
    // Without this, a corrupt *container* is silently softer than a corrupt *field*:
    // `Value::get` on a non-object returns `None`, which this path reads as "records
    // no pipeline_version", so `"meta": null` / `"x"` / `[]` replayed with **no skew
    // check at all**, while `{"pipeline_version": "1"}` inside a well-formed `meta`
    // is a loud exit 2. Malformed provenance must be as loud as an unreadable field.
    // (Unknown *fields* inside a well-formed `meta` stay lenient on purpose — that is
    // the forward-compatibility contract: an older build must tolerate a newer
    // build's extra provenance.)
    if let Some(meta) = obj.get("meta")
        && !meta.is_object()
    {
        return Err(NcError::Usage(format!(
            "{context}: sidecar `meta` must be an object, got {}. A malformed `meta` \
             carries no readable provenance, and treating it as absent would silently \
             skip the pipeline_version skew check this envelope exists to enable — \
             omit `meta` entirely if the recipe has no provenance to record",
            json_kind(meta)
        )));
    }
    let meta_pipeline_version = meta_pipeline_version(envelope.meta.as_ref(), context)?;
    Ok(Some((envelope.params, meta_pipeline_version)))
}

/// The `pipeline_version` recorded in a sidecar's `meta`, when present.
///
/// Present-but-unreadable is a **loud error**, not `None`. `None` means "this file
/// records no version" and suppresses the skew check entirely, so silently mapping a
/// `1.0`, a `"1"`, or a negative number onto it would disable the very warning the
/// label exists to raise — a sidecar round-tripped through a tool that emits `1.0`
/// would then replay on a later build and produce different pixels in silence. The
/// range check matters for the same reason in the other direction: `as u32`
/// truncation turns `4294967297` into `1`, which *matches* this build and suppresses
/// the warning by pretending to agree with it.
fn meta_pipeline_version(meta: Option<&serde_json::Value>, context: &str) -> Result<Option<u32>> {
    let Some(raw) = meta.and_then(|m| m.get("pipeline_version")) else {
        return Ok(None);
    };
    let bad = || {
        NcError::Usage(format!(
            "{context}: `meta.pipeline_version` is {raw}, which is not a pipeline version — it \
             must be a non-negative integer no larger than {}. A value Hanten cannot read would be \
             indistinguishable from an absent one and would silently disable the \
             pipeline_version skew warning",
            u32::MAX
        ))
    };
    let n = raw.as_u64().ok_or_else(bad)?;
    Ok(Some(u32::try_from(n).map_err(|_| bad())?))
}

/// A JSON value's kind, for error messages that need to name what was found.
fn json_kind(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
}

/// The warning for replaying a recipe captured under a **different** behavioral
/// `pipeline_version` than this build implements: the recipe still applies, but the
/// default render it was captured under has changed, so the pixels will not match
/// the original output. Loud (and `--strict`-promotable) rather than silent — that
/// mismatch is exactly what `pipeline_version` exists to make visible.
fn pipeline_version_warning(loaded_version: Option<u32>) -> Option<String> {
    let recorded = loaded_version?;
    (recorded != version::PIPELINE_VERSION).then(|| {
        format!(
            "the loaded recipe was produced by pipeline_version {recorded}, but this build is \
             pipeline_version {} — the parameters still apply, but the default conversion \
             behavior changed between them, so the output will not match the original",
            version::PIPELINE_VERSION
        )
    })
}

/// Map the (clap-mutually-exclusive) film-base flags to a [`FilmBaseSource`],
/// or `None` when none was passed. Shared by `convert`'s merge ([`recipe::merge`]) and
/// `estimate`, so they resolve the source identically.
pub(crate) fn film_base_source_override(o: &FilmBaseOverrides) -> Option<FilmBaseSource> {
    if let Some(v) = o.film_base {
        Some(FilmBaseSource::Explicit(v))
    } else if let Some(v) = o.base_region {
        Some(FilmBaseSource::Region(v))
    } else if o.auto_base {
        Some(FilmBaseSource::Auto)
    } else {
        None
    }
}

/// Validate that an explicit film base is a per-channel transmission in `(0, 1]`
/// — the one invariant that must hold wherever an explicit base enters (a recipe
/// via [`validate_shared`], or the `--film-base` flag on `estimate`). Non-positive /
/// non-finite would divide into inf/NaN downstream; a value above 1.0 (e.g. a
/// "90" typo for "0.90") would render every real sample above white.
fn validate_explicit_film_base(base: &[f32; 3]) -> Result<()> {
    if base.iter().any(|v| !v.is_finite() || *v <= 0.0 || *v > 1.0) {
        return Err(NcError::Usage(format!(
            "--film-base channels are transmissions in (0, 1] (got {base:?})"
        )));
    }
    Ok(())
}

/// Refuse typed roll flags when nothing will apply them: a recipe's film master (a typed
/// `--film-master` conflicts at the parser) or a `direct` rendering. The film master is
/// named first. A presence rule, so it runs before `recipe::validate`, whose value rules
/// would otherwise refuse first with remedies that cannot work. A recipe's `roll` section
/// is spared.
fn reject_roll_flags_nothing_applies(args: &ConvertArgs, r: &Recipe) -> Result<()> {
    let direct = r.rendering == crate::rendering::Rendering::Direct;
    if !args.roll.any() || (r.output != OutputSection::FilmMaster && !direct) {
        return Ok(());
    }
    let typed = [
        (
            "--roll-white-balance",
            args.roll.roll_white_balance.is_some(),
        ),
        ("--roll-white", args.roll.roll_white.is_some()),
    ]
    .into_iter()
    .filter_map(|(flag, typed)| typed.then_some(flag))
    .collect::<Vec<&str>>()
    .join(" and ");
    let (them, apply) = if args.roll.roll_white_balance.is_some() && args.roll.roll_white.is_some()
    {
        ("them", "apply")
    } else {
        ("it", "applies")
    };
    let message = if r.output == OutputSection::FilmMaster {
        // Under `direct` too, either remedy alone would meet the film master + `direct`
        // refusal next, so each carries `--rendering default`.
        let (drop, rendered) = if direct {
            (
                format!("drop {typed} and pass --rendering default"),
                "choose a rendered destination (--range, --transfer, --gamut or --container) \
                 with --rendering default",
            )
        } else {
            (
                format!("drop {typed}"),
                "choose a rendered destination (--range, --transfer, --gamut or --container)",
            )
        };
        format!(
            "{typed} {apply} the roll's measurements through the rendering stages, but the \
             recipe's `output` is \"film-master\", which writes the fixed decode's linear \
             ACEScg with no rendering stage and would ignore {them}. Either {drop}, or \
             {rendered}"
        )
    } else {
        format!(
            "{typed} {apply} the roll's measurements, but the rendering is `direct` \
             (--rendering, recipe `rendering`), which leaves the roll out and would ignore \
             {them}. Either drop {typed}, or pass --rendering default, which applies the \
             roll"
        )
    };
    Err(NcError::Usage(message))
}

/// The `convert` **value** gate, after the flags merged: the recipe's own stage and
/// decode rules ([`recipe::validate`]), the output path's suffix, then the shared
/// sections ([`validate_shared`]) — whose last rule, no film base chosen, is the least
/// specific diagnosis there is.
///
/// It is not the whole gate: the flag-presence rule [`reject_roll_flags_nothing_applies`]
/// runs before it, in `run_convert`, since it must diagnose a typed roll flag before any
/// value rule here can refuse with a remedy that cannot work.
///
/// The suffix is a property of *this invocation*, so it outranks the shared value
/// rules, and specifically the missing-base rule: without that ordering, `-o out.jpg`
/// with no base demands a base first and only then mentions the suffix, making the
/// user fix two things in series.
///
/// `roll` composes the same pieces itself (`resolve_frames`): it has no `-o`, and it
/// names knobs by recipe key alone.
pub fn validate_convert(r: &Recipe, args: &ConvertArgs) -> Result<()> {
    recipe::validate(r, KnobNames::FlagAndKey)?;
    resolve_output_path(
        &args.output,
        OutputTarget::resolve(r, KnobNames::FlagAndKey, args.destination.film_master)?,
        SuffixContext::Convert,
    )?;
    validate_shared(r, FilmBaseRemedy::Flags)
}

/// Where the output path came from, which is what a suffix diagnosis must vary on:
/// its remedies name what the reader's command line can reach.
#[derive(Clone, Copy, Debug)]
enum SuffixContext<'a> {
    /// `convert`'s `-o`.
    Convert,
    /// A `roll` frame whose manifest entry supplied the path.
    RollFrame(&'a Path),
}

/// The path nc will actually write: the path as given when it states a suffix the
/// resolved container accepts, or that path with the container's canonical suffix
/// appended when it states none.
///
/// The rule in one line: **a suffix is never rewritten, only completed.** A
/// trailing dot-segment counts as a suffix only when it is a spelling *some*
/// container accepts, so `out.jpg` under a TIFF destination is still a usage error,
/// while `out.v2` and `roll-1.2` are stems and keep their dot (`out.v2.tiff`). **No
/// byte that decides *which file* is named is ever altered or dropped**: nc
/// *completes* a path, it never renames one. Not quite byte-for-byte, and the gap is
/// deliberate: [`Path::with_file_name`] normalises redundant separators and interior
/// `.` segments, so `out//x` resolves to `out/x.tiff`. Those denote the same file, so
/// the claim is about the file, not the spelling. Where a dropped byte *would* change
/// the file — a path naming a directory — the answer is a refusal, never a quiet
/// rewrite; see [`Unappendable`].
///
/// Shared by `convert` and by `roll`'s **explicit** manifest paths, so the two
/// cannot grow parallel rules. Roll's **derived** names do not come through here:
/// they are built from [`Container::canonical`] and are correct by construction.
fn resolve_output_path(
    given: &Path,
    target: OutputTarget,
    context: SuffixContext<'_>,
) -> Result<PathBuf> {
    let container = target.container();
    match given.extension() {
        // A spelling some container claims is a container request, so it is judged.
        Some(ext) if given_container(given).is_some() => {
            if accepts(container, ext) {
                // Honoured exactly as typed, including a non-canonical spelling and
                // its case: `out.jpeg` stays `.jpeg`, `out.TIF` stays `.TIF`.
                Ok(given.to_path_buf())
            } else {
                Err(suffix_mismatch_error(target, given, context))
            }
        }
        // No dot-segment, or one no container claims: the whole path is the stem.
        _ => append_suffix(given, container.canonical(), context),
    }
}

/// The container a path's stated suffix names, if it names one — the test that
/// tells a suffix from a dotted stem.
fn given_container(given: &Path) -> Option<Container> {
    let ext = given.extension()?;
    Container::ALL.iter().copied().find(|c| accepts(*c, ext))
}

/// Whether `container` accepts this spelling, in any case.
fn accepts(container: Container, ext: &OsStr) -> bool {
    container
        .accepted()
        .iter()
        .any(|want| ext.eq_ignore_ascii_case(want))
}

/// Why a path has no file name to append a suffix to. Two shapes, because the
/// path helpers hide them in *different* ways and only one is self-announcing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unappendable {
    /// `.`, `..`, `/`, `dir/..` — [`Path::file_name`] itself returns `None`.
    NamesNoFile,
    /// `dir/`, `dir//`, `dir/.`, `dir/./` — the path's last *meaningful* component
    /// is a directory, but `file_name()` normalises the trailing separator or `.`
    /// away and hands back the directory's own name, so appending would silently
    /// write that directory's **sibling**.
    NamesADirectory,
}

impl Unappendable {
    /// The clause naming the fault, shared by every context's message.
    fn what(self) -> &'static str {
        match self {
            Self::NamesNoFile => "names no file",
            Self::NamesADirectory => "names a directory rather than a file",
        }
    }

    /// Which shape `given` is, or `None` when it has a file name to append to.
    ///
    /// **Syntactic and pure on purpose** — no `is_dir()` probe, because this runs
    /// inside `validate_convert` *and* roll's planner, where a filesystem stat would
    /// make the answer timing- and platform-dependent. A path that merely *happens*
    /// to name an existing directory (`-o positives`) is therefore still a stem:
    /// `positives.jpg` is unambiguous.
    ///
    /// The directory test reads the string form because `Path` has already thrown
    /// the evidence away by the time `file_name()` answers. Lossy conversion can
    /// neither add nor remove a trailing ASCII separator or `.`, so it is exact for
    /// non-UTF-8 paths too, and `is_separator` covers `\` on Windows. It must match
    /// a trailing `.` **only** when a separator precedes it: `out.` is the
    /// documented degenerate stem (`out..jpg`), not a directory.
    fn of(given: &Path) -> Option<Self> {
        if given.file_name().is_none() {
            return Some(Self::NamesNoFile);
        }
        let shown = given.to_string_lossy();
        let names_dir = shown.ends_with(std::path::is_separator)
            || shown
                .strip_suffix('.')
                .is_some_and(|head| head.ends_with(std::path::is_separator));
        names_dir.then_some(Self::NamesADirectory)
    }
}

/// The diagnosis for a path with nothing to append a suffix to. Like
/// [`suffix_mismatch_error`], every arm names a remedy the *reader's* command line
/// can actually reach — a bare message told a `roll` user to "use `hanten roll
/// --out-dir`" while they were running exactly that, and never said which of 40
/// manifest entries to fix.
fn unappendable_error(
    reason: Unappendable,
    given: &Path,
    ext: &str,
    context: SuffixContext<'_>,
) -> NcError {
    NcError::Usage(match context {
        // A manifest entry named this path, so attribute the frame and name the two
        // remedies *that entry* has. Never `--out-dir`: it is a whole-roll flag the
        // reader has already passed, and it cannot fix one entry.
        SuffixContext::RollFrame(input) => format!(
            "frame {}: the manifest's explicit output {} {}, so Hanten has nothing to \
             append a `.{ext}` suffix to — give the entry's `output` a file name, or drop \
             its `output` key to take the derived name inside the out-dir",
            input.display(),
            given.display(),
            reason.what()
        ),
        // `convert`, either provenance: the remedy is the path in `-o`. The directory
        // case also points at roll, because `--out-dir positives/` is where the
        // trailing separator comes from in the first place.
        _ => {
            let mut msg = format!(
                "the output path {} {}, so Hanten has nothing to append a `.{ext}` \
                 suffix to — give a path ending in a file name",
                given.display(),
                reason.what()
            );
            if reason == Unappendable::NamesADirectory {
                msg.push_str(
                    ", or use `hanten roll --out-dir` to write a whole roll into a directory",
                );
            }
            msg
        }
    })
}

/// `given` with `.<ext>` appended to its file name.
///
/// Appended, never [`PathBuf::set_extension`]: that *replaces*, so it would turn
/// `out.v2` into `out.jpg` and eat a stem the user typed.
///
/// Refused for either [`Unappendable`] shape rather than completed — completing a
/// directory path writes its sibling, which on `roll` puts the whole roll outside
/// the `--out-dir` the user named, at exit 0, with the report agreeing.
fn append_suffix(given: &Path, ext: &str, context: SuffixContext<'_>) -> Result<PathBuf> {
    if let Some(reason) = Unappendable::of(given) {
        return Err(unappendable_error(reason, given, ext, context));
    }
    let mut completed = given
        .file_name()
        .expect("Unappendable::of rejects a path with no file name")
        .to_os_string();
    completed.push(".");
    completed.push(ext);
    Ok(given.with_file_name(completed))
}

/// The diagnosis for a stated suffix the resolved destination does not write. Every
/// arm names a remedy the *reader's* command line can actually reach.
fn suffix_mismatch_error(
    target: OutputTarget,
    output: &Path,
    context: SuffixContext<'_>,
) -> NcError {
    let list = required_extensions(target)
        .iter()
        .map(|e| format!(".{e}"))
        .collect::<Vec<_>>()
        .join(" or ");
    let OutputTarget {
        destination,
        stated: stated_axes,
        defaults,
        film_master_flag,
    } = target;
    // A roll takes no conversion flags, so its frame names the recipe keys.
    let (frame, names) = match context {
        SuffixContext::RollFrame(input) => {
            (format!("frame {}: ", input.display()), KnobNames::KeyOnly)
        }
        SuffixContext::Convert => (String::new(), KnobNames::FlagAndKey),
    };
    // A destination that writes the stated suffix is offered only when a ready one
    // does, derived from the table (`destination::writing`) — never a container nothing
    // can write yet — and as the axes to state **over** what the run stated, since a
    // flag overrides a recipe's axis but never removes it. The film master states no
    // axes; its offer replaces it. Dropping the suffix is the one remedy that always
    // works — changing an axis may not.
    let offers = given_container(output)
        .map(|c| crate::destination::writing(c, &stated_axes, &defaults))
        .unwrap_or_default();
    // Only a typed `--film-master` needs dropping (it conflicts with the axis flags at
    // the parser); a recipe's is replaced by the axis flags themselves, which start
    // from no stated axes (`recipe::merge`).
    let lead = match (destination, names) {
        (recipe::Destination::FilmMaster, KnobNames::FlagAndKey) if film_master_flag => {
            "or drop --film-master and state a destination that writes it"
        }
        (recipe::Destination::FilmMaster, KnobNames::FlagAndKey) => {
            "or state a destination that writes it — these flags replace the recipe's \
             `output` \"film-master\""
        }
        (recipe::Destination::FilmMaster, KnobNames::KeyOnly) => {
            "or replace `output` \"film-master\" with a destination that writes it"
        }
        (recipe::Destination::Display(_), _) => "or state a destination that writes it",
    };
    let instead = match offers.as_slice() {
        [] => String::new(),
        list => format!(
            ", {lead}: {}",
            list.iter()
                .map(|a| recipe::complete_destination(names, a))
                .collect::<Vec<_>>()
                .join("; ")
        ),
    };
    let suffix = output
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    NcError::Usage(format!(
        "{frame}the output path {} does not end in {list}: the destination is {}, which \
         writes {list}. Hanten never renames a suffix you state — drop {suffix} and the \
         path is completed for you{instead}",
        output.display(),
        recipe::destination_label(destination, names),
    ))
}

/// What an output path is judged against: the resolved destination
/// (`crate::destination`); the axes the run stated (after flags merged over the
/// recipe), which a suffix remedy has to work on top of; and whether the film master
/// came from a typed `--film-master` rather than the recipe, which decides how the
/// remedy says to leave it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OutputTarget {
    destination: recipe::Destination,
    stated: DisplayAxes,
    /// The rendering's axis defaults, which a suffix remedy is derived under.
    defaults: crate::destination::Defaults,
    film_master_flag: bool,
}

impl OutputTarget {
    /// The target a run's output is judged against. `film_master_flag` is whether
    /// `--film-master` was typed (always `false` on `roll`, which takes no conversion
    /// flags).
    fn resolve(r: &Recipe, names: KnobNames, film_master_flag: bool) -> Result<Self> {
        Ok(Self {
            destination: recipe::destination(r, names)?,
            stated: match r.output {
                OutputSection::Display(axes) => axes,
                OutputSection::FilmMaster => DisplayAxes::default(),
            },
            defaults: r.base().axes,
            film_master_flag,
        })
    }

    /// The file container the destination's encoder writes.
    fn container(self) -> Container {
        match self.destination {
            recipe::Destination::Display(d) => d.container,
            recipe::Destination::FilmMaster => Container::Tiff,
        }
    }
}

/// Output-path extensions the destination's container accepts.
fn required_extensions(target: OutputTarget) -> &'static [&'static str] {
    target.container().accepted()
}

/// How the calling command can state a film base — the one thing the
/// missing-base diagnosis must vary on, because the remedies are disjoint.
///
/// `convert` has all three film-base flags; `roll` has **none** of them
/// (`RollArgs` flattens only `MemoryArgs`/`ReportArgs`), so telling a `roll` user
/// to "pass `--auto-base`" is advice they cannot follow — the flag exits 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilmBaseRemedy {
    /// `convert` — `--film-base` / `--base-region` / `--auto-base`, or the recipe.
    Flags,
    /// `roll` — the shared `--params` recipe only.
    SharedRecipe,
}

impl FilmBaseRemedy {
    /// The remedy available to the command named in a report's `command` field.
    fn for_command(command: &str) -> Self {
        match command {
            "roll" => Self::SharedRecipe,
            _ => Self::Flags,
        }
    }
}

/// The **one** spelling of "no film base was stated", so the two places that can
/// report it ([`validate_shared`] and [`convert_frame`]'s totality guard) cannot drift
/// into two differently-worded diagnoses of the same condition.
///
/// Command-aware by [`FilmBaseRemedy`]: the requirement is identical, but what the
/// user can do about it is not.
pub fn missing_film_base_message(remedy: FilmBaseRemedy) -> String {
    match remedy {
        FilmBaseRemedy::Flags => "no film base selected: pass --film-base R,G,B (a Dmin measured \
             once per roll, e.g. with `hanten estimate`), --base-region X,Y,W,H to sample an \
             unexposed border, or --auto-base to detect the rebate band (best-effort: real scans \
             put a thin inset rebate behind the holder, so it can fail). Recipe key: \
             `calibration.film_base`."
            .to_string(),
        // `roll` deliberately does not repeat the flag names as an option: it has
        // none of them, and the first version of this message sent users to flags
        // that exit 2.
        FilmBaseRemedy::SharedRecipe => "no film base selected: `roll` takes no film-base flags, \
             so set `calibration.film_base` in the shared --params recipe. Measuring once per roll is \
             the intended workflow: run `hanten estimate --base-region X,Y,W,H <reference-scan>` on \
             one frame and paste the reported `calibration` object straight in \
             (`\"calibration\": {\"film_base\": {\"explicit\": [R, G, B]}}`) — that is \
             also the only source that keeps every frame on one frozen Dmin. \
             `\"auto\"` and `{\"region\": [X, Y, W, H]}` are accepted \
             too, but re-estimate per frame, so the roll is not colour-consistent."
            .to_string(),
    }
}

/// Validate the recipe's **shared** sections — the input, the film base and the
/// measurement region, which the decode and the film-base stage read before any
/// rendering stage — at the CLI boundary, so the pure stages can trust their inputs.
/// Every failure is a [`NcError::Usage`] (exit 2).
///
/// Shared verbatim by `convert` and `roll` (and each `roll` per-frame override); the
/// stage sections' rules are [`recipe::validate`]'s. The caller states which remedy its
/// users have for an unstated film base: only the wording of that one diagnosis
/// differs.
pub fn validate_shared(r: &Recipe, remedy: FilmBaseRemedy) -> Result<()> {
    // Film base: an explicit base is a per-channel transmission in (0, 1] — the
    // decoded scan is [0, 1]-normalized, so a value above 1 (e.g. a "90" typo for
    // "0.90") would silently render every real sample denser than the base; a
    // sampled region must have non-zero extent; auto needs nothing.
    // The *unstated* case is deliberately not handled here — it is the last rule in
    // this function. "You have not chosen a film base" is the least specific
    // diagnosis there is, so letting it run first would pre-empt every rule below
    // (and `reject_roll_unsupported*`) on a config that has both problems, reporting
    // the vaguer one. Flag-shape first.
    match r.calibration.film_base {
        Some(FilmBaseSource::Explicit(b)) => validate_explicit_film_base(&b)?,
        Some(FilmBaseSource::Region([_, _, w, h])) if w == 0 || h == 0 => {
            return Err(NcError::Usage(
                "--base-region width and height must be > 0".into(),
            ));
        }
        Some(FilmBaseSource::Region(_)) | Some(FilmBaseSource::Auto) | None => {}
    }

    // Measurement region: a *value* rule, so `roll` and every per-frame override reach
    // it too — a stage-only check would let a whole roll decode before failing per
    // frame. The bound itself is `types::check_measure_inset`, the one definition
    // `film_base::effective_area` also calls.
    check_measure_inset(r.measure.inset)?;

    // Last, deliberately: `calibration.film_base` has no default, and `Dmin` is the
    // divisor of the density conversion, so falling into auto-detection by omission
    // decided the most consequential parameter for the user. `--auto-base` is still
    // one flag away — the requirement is that the choice be *stated*, not that it be
    // explicit.
    if r.calibration.film_base.is_none() {
        return Err(NcError::Usage(missing_film_base_message(remedy)));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Output helpers
// ---------------------------------------------------------------------------

/// Serialize a value as pretty JSON to a file; an I/O failure is a write error.
///
/// Staged and committed immediately, so a failure mid-write cannot leave a truncated
/// document at `path` — but *not* held back to join a conversion's artifact set
/// (`io/transactional-output-writes`). Both callers are deliberately outside it:
/// `--dump-params` is written before anything is decoded, and `--report-file` must
/// land even when `--strict` subsequently fails the run — and in `roll` it is a
/// roll-level artifact that no single frame's set could hold.
fn write_json<T: Serialize>(path: &Path, value: &T, log: &Log) -> Result<()> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| NcError::Other(format!("serializing JSON: {e}")))?;
    // Promotion notes (currently: a hard-linked target whose aliases keep the old bytes)
    // go to stderr here rather than into `report.warnings`. These are *operational*
    // artifacts — `--dump-params`, `--report-file` — and folding them into the conversion's
    // warning set would let a hard-linked report file fail a `--strict` render, which is not
    // what `--strict` is about.
    //
    // `warn_always`, not `warn`: kept out of the JSON report *and* quiet-gated would mean
    // no channel carries it under `--quiet`, leaving the stranded alias entirely silent —
    // exactly the defect this reporting exists to close. That combination is what
    // `warn_always` is for (see its doc comment; fail-soft telemetry uses it for the same
    // reason).
    for note in staged::stage_bytes(path, json.as_bytes())?.commit()? {
        log.warn_always(&note);
    }
    Ok(())
}

/// Emit a report as JSON to stdout (kept clean) or `--report-file`. `none`
/// suppresses it entirely.
fn emit_report(
    report: &Report,
    format: ReportFormat,
    file: Option<&Path>,
    log: &Log,
) -> Result<()> {
    emit_json(report, format, file, log)
}

/// Emit any serializable report as JSON to stdout (kept clean) or a file. `none`
/// suppresses it entirely. Shared by the per-command [`Report`] and the roll-level
/// [`RollReport`].
fn emit_json<T: Serialize>(
    value: &T,
    format: ReportFormat,
    file: Option<&Path>,
    log: &Log,
) -> Result<()> {
    if format == ReportFormat::None {
        return Ok(());
    }
    match file {
        Some(p) => write_json(p, value, log),
        None => {
            let json = serde_json::to_string_pretty(value)
                .map_err(|e| NcError::Other(format!("serializing report: {e}")))?;
            println!("{json}");
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// lcms2 runtime-error handler
// ---------------------------------------------------------------------------

/// Set when lcms2 reports a runtime error through the process-global handler.
static CMS_ERROR: AtomicBool = AtomicBool::new(false);

/// lcms2 error callback. Records that a color-management error occurred and
/// echoes it to stderr (stdout stays report-only). `cmsDoTransform` (under
/// `Transform::transform_in_place`) is infallible and Little CMS's *default*
/// handler silently discards errors, so this hook is the only way a runtime
/// transform/profile fault in `pipeline::color` becomes visible.
unsafe extern "C" fn cms_error_handler(
    _ctx: lcms2_sys::Context,
    code: u32,
    text: *const std::os::raw::c_char,
) {
    CMS_ERROR.store(true, Ordering::SeqCst);
    let msg = if text.is_null() {
        std::borrow::Cow::Borrowed("(no message)")
    } else {
        // SAFETY: lcms2 passes a NUL-terminated C string for the message text.
        unsafe { std::ffi::CStr::from_ptr(text) }.to_string_lossy()
    };
    eprintln!("hanten: lcms2 error [{code}]: {msg}");
}

/// Install the process-global lcms2 error handler at startup. `pipeline::color`
/// builds its profiles/transforms on lcms2's global context, and the safe `lcms2`
/// wrapper exposes the handler only per-`ThreadContext`, so we set the global one
/// through the `lcms2-sys` FFI directly.
fn install_cms_error_handler() {
    // SAFETY: `cms_error_handler` matches lcms2's LogErrorHandlerFunction ABI and
    // only touches an atomic + stderr, so it is sound to call from C on any thread.
    unsafe { lcms2_sys::cmsSetLogErrorHandler(Some(cms_error_handler)) }
}

/// Take and clear the "lcms2 logged an error" flag. The orchestrator checks it
/// right after the color transform runs, which the infallible
/// `transform_in_place` cannot report through its return value.
fn cms_error_occurred() -> bool {
    CMS_ERROR.swap(false, Ordering::SeqCst)
}

// ---------------------------------------------------------------------------
// stderr logging (never touches stdout — that stays report-only)
// ---------------------------------------------------------------------------

/// Verbosity-gated stderr logger. `--quiet` silences everything below an error;
/// `-v`/`-vv` enable progress `info` lines. Warnings always go to the JSON
/// report (via [`push_warning`]); this only controls the stderr echo.
struct Log {
    verbose: u8,
    quiet: bool,
}

impl Log {
    fn new(args: &ReportArgs) -> Self {
        Self {
            verbose: args.verbose,
            quiet: args.quiet,
        }
    }

    /// Progress line — only shown with `-v` (and never when `--quiet`).
    fn info(&self, msg: impl Display) {
        if !self.quiet && self.verbose >= 1 {
            eprintln!("hanten: {msg}");
        }
    }

    /// Warning line — shown unless `--quiet` (the report keeps it either way).
    fn warn(&self, msg: &str) {
        if !self.quiet {
            eprintln!("hanten: warning: {msg}");
        }
    }

    /// Warning line shown *regardless* of `--quiet`. For fail-soft telemetry
    /// failures, which are deliberately kept out of the JSON report (so `--strict`
    /// can't promote them) and would otherwise vanish entirely under `--quiet` —
    /// an opted-in feature failing must never be silent. Ordinary warnings use
    /// [`warn`](Self::warn), which `--quiet` suppresses since the report still
    /// records them.
    fn warn_always(&self, msg: &str) {
        eprintln!("hanten: warning: {msg}");
    }
}

/// Record a warning into the report and echo it to stderr in one step, so the
/// two never drift.
fn push_warning(report: &mut Report, log: &Log, msg: String) {
    log.warn(&msg);
    report.warnings.push(msg);
}

/// Like [`push_warning`], but into a caller-owned buffer instead of a [`Report`].
/// [`convert_frame`] accumulates here so a frame that warns and *then* fails still
/// hands its warnings back to the caller (the report only rides out on success).
fn push_warning_buf(warnings: &mut Vec<String>, log: &Log, msg: String) {
    log.warn(&msg);
    warnings.push(msg);
}

// ---------------------------------------------------------------------------
// Entry point + dispatch
// ---------------------------------------------------------------------------

/// Parse arguments and run the requested subcommand. The single entry point the
/// binary's `main` calls. clap handles `--help`/`--version` and usage errors with
/// its own (exit-2-compatible) codes; everything else flows through [`NcError`].
pub fn run() -> Result<()> {
    // Install once at startup so any lcms2 runtime fault in `pipeline::color`
    // surfaces instead of being silently swallowed by the default no-op handler.
    install_cms_error_handler();
    let cli = Cli::parse();
    match cli.command {
        Command::Params(args) => run_params(&args),
        Command::Convert(args) => run_convert(args),
        Command::Roll(args) => run_roll(args),
        Command::Inspect(args) => run_inspect(args),
        Command::Estimate(args) => run_estimate(args),
        Command::MeasureRoll(args) => run_measure_roll(args),
    }
}

/// `hanten params` — print the full default recipe as JSON to stdout.
fn run_params(args: &ParamsArgs) -> Result<()> {
    reject_new_flow(args.new_flow)?;
    let json = serde_json::to_string_pretty(&Recipe::default())
        .map_err(|e| NcError::Other(format!("serializing params: {e}")))?;
    println!("{json}");
    Ok(())
}

/// Best-effort stable key for path-collision checks. Canonicalize the path when
/// it exists (resolves symlinks and `..`); for a not-yet-created write target,
/// canonicalize its parent directory instead (`tmp/sub/../out.tiff` and
/// `tmp/out.tiff` must compare equal — `std::path::absolute` alone keeps the
/// `..` and would let them slip past the check), re-attaching the file name.
/// When even the parent doesn't exist, fall back to a lexical normalization of
/// the absolute form. A guard against accidental self-clobbering, not
/// adversarial links. Casing is preserved here; [`keys_collide`] applies the
/// case-insensitive comparison so a not-yet-created `out.tiff`/`OUT.TIFF` pair
/// (which can't be canonicalized to a shared casing) still collides.
fn collision_key(path: &Path) -> PathBuf {
    if let Ok(c) = std::fs::canonicalize(path) {
        return c;
    }
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
        let parent = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        if let Ok(p) = std::fs::canonicalize(parent) {
            return p.join(name);
        }
    }
    lexical_absolute(path)
}

/// Absolute form with `.`/`..` components removed lexically (no filesystem
/// access). Last-resort key for paths whose parent doesn't exist yet; lexical
/// `..` removal can disagree with the filesystem across symlinked directories,
/// which is acceptable for an accident guard.
fn lexical_absolute(path: &Path) -> PathBuf {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Whether two collision keys refer to the same write target. Compares exactly
/// **or** ignoring ASCII case: on a case-insensitive filesystem (macOS/Windows
/// default) `out.tiff` and `OUT.TIFF` are the same file, but when neither exists
/// yet [`collision_key`] can't canonicalize them to a shared casing, so a
/// case-sensitive `==` would wrongly let one write clobber the other. Detecting
/// per-volume case sensitivity portably isn't cheap, so we **conservatively
/// over-reject**: this is an accident guard, and false-rejecting `out.tiff` vs
/// `OUT.TIFF` in a single invocation (a harmless annoyance) is the right trade
/// against false-accepting and silently overwriting the just-written output.
fn keys_collide(a: &Path, b: &Path) -> bool {
    a == b
        || a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
}

/// Reject write targets that would clobber the input scan or one another —
/// e.g. `-o` equal to the input (destroys the negative), or `--report-file`
/// equal to the output or the IR export (truncates a just-written artifact) — all of
/// which would otherwise "succeed" with exit 0. Fail loudly up front instead.
/// Comparison is case-insensitivity-aware (see [`keys_collide`]) so a
/// case-only difference can't slip a second write onto the same file on a
/// case-insensitive filesystem.
fn ensure_write_targets_distinct(input: &Path, targets: &[(&str, &Path)]) -> Result<()> {
    let input_key = collision_key(input);
    let mut seen: Vec<(&str, PathBuf)> = Vec::with_capacity(targets.len());
    for (label, path) in targets {
        let key = collision_key(path);
        if keys_collide(&key, &input_key) {
            return Err(NcError::Usage(format!(
                "{label} ({}) would overwrite the input scan",
                path.display()
            )));
        }
        if let Some((other, _)) = seen.iter().find(|(_, k)| keys_collide(k, &key)) {
            return Err(NcError::Usage(format!(
                "{label} ({}) collides with {other}",
                path.display()
            )));
        }
        seen.push((label, key));
    }
    Ok(())
}

/// Reject the deprecated input-color CLI flags loudly before merge/convert.
///
/// `--assume-linear` (the old *combined* assertion) is a hard usage error with
/// migration guidance — it must never silently assert both axes. `--input-profile`
/// stays rejected for normal conversion (input-side ICC application has no
/// validated placement; it is reserved for the deferred
/// scanner-profile-before-density experiment). `convert`-only; `roll` takes its
/// input axes from the shared recipe, whose retired `input.color` key is refused at
/// load by [`recipe::check_body`].
fn reject_deprecated_input_flags(o: &InputOverrides) -> Result<()> {
    if o.assume_linear {
        return Err(NcError::Usage(
            "--assume-linear was removed: it asserted transfer encoding AND measurement \
             meaning at once. Assert them independently — `--input-transfer linear` (transfer) \
             and, for raw scanner data, `--input-meaning scanner-device` (meaning)."
                .into(),
        ));
    }
    if let Some(p) = &o.input_profile {
        return Err(NcError::Unsupported(format!(
            "--input-profile {p}: input-side ICC application is not supported for normal \
             conversion; it is reserved for the deferred scanner-profile-before-density \
             experiment. SilverFast scans are decoded as linear scanner measurements."
        )));
    }
    Ok(())
}

/// Migration errors for every removed conversion flag. nc is unreleased, so these flags
/// survive only as hidden args that emit actionable guidance, never as aliases. The
/// recipe-side mirror is [`recipe::check_body`].
fn reject_removed_flags(args: &ConvertArgs) -> Result<()> {
    reject_new_flow(args.new_flow)?;
    if let Some(name) = &args.algorithm {
        return Err(NcError::Usage(format!(
            "--algorithm {name} was removed: there is one fixed decode, so no algorithm to \
             choose (recipe `reconstruction`). Drop the flag."
        )));
    }
    if let Some(message) = removed_characteristic_message(&args.characteristic) {
        return Err(NcError::Usage(message));
    }
    if let Some(value) = &args.reconstruction {
        return Err(NcError::Usage(format!(
            "--reconstruction {value}: {REMOVED_SIMPLE_RECONSTRUCTION}."
        )));
    }
    if let Some((flag, remedy)) = removed_sigmoid_flag(&args.sigmoid) {
        return Err(NcError::Usage(format!(
            "{flag} was removed with the sigmoid curve: {remedy}."
        )));
    }
    if let Some((flag, what)) = removed_dmax_flag(&args.dmax) {
        return Err(NcError::Usage(removed_dmax_message(flag, what)));
    }
    if let Some(flag) = removed_balance_flag(&args.balance) {
        return Err(NcError::Usage(format!(
            "{flag} was removed with the regional balance: {REGIONAL_BALANCE_RETIRED}. \
             Drop the flag."
        )));
    }
    if let Some(message) = removed_print_message(&args.removed_print) {
        return Err(NcError::Usage(message));
    }
    if let Some(message) = removed_output_message(args) {
        return Err(NcError::Usage(message));
    }
    // The removed simple-reconstruction controls, and the controls that replaced them,
    // which retired with the print stage in turn.
    for (flag, present, replacement) in [
        (
            "--invert-white-balance",
            args.simple.invert_white_balance.is_some(),
            "--white-balance R,G,B (recipe `scene_correction.white_balance`)",
        ),
        (
            "--clip-low",
            args.simple.clip_low.is_some(),
            "nothing yet: the affine levels remap that replaced it has no home \
             (`nf-scene-correction/levels-knob`)",
        ),
        (
            "--clip-high",
            args.simple.clip_high.is_some(),
            "nothing yet: the affine levels remap that replaced it has no home \
             (`nf-scene-correction/levels-knob`)",
        ),
    ] {
        if present {
            return Err(NcError::Usage(format!(
                "{flag} was removed with the `simple` reconstruction; its counterpart is \
                 {replacement}."
            )));
        }
    }
    Ok(())
}

/// The migration error for a removed print control, or `None` when none was passed.
/// The print stage retired with the chain that ran it (`nf-core/default-flip`), and its
/// knobs went to different stages — which is why each names its own replacement.
fn removed_print_message(flags: &RemovedPrintFlags) -> Option<String> {
    if flags.print_exposure.is_some() {
        return Some(
            "--print-exposure was removed with the print stage: exposure is scene \
             correction's, `--exposure` in stops (recipe `scene_correction.exposure`), \
             applied before the look and the display fit. There is no alias."
                .into(),
        );
    }
    if flags.black_point.is_some() {
        return Some(
            "--black-point was removed with the print stage: it was one linear subtraction \
             on film RGB. Where black renders is now display black, `--display-black` \
             (recipe `fit_range.display_black`), which places the film base in stops below \
             mid-grey. There is no alias."
                .into(),
        );
    }
    if flags.auto_wb.is_some() {
        return Some(
            "--auto-wb was removed: a per-frame estimate reads a sunset as the cast and \
             removes it, so white balance is measured once per roll. Run `hanten \
             measure-roll`, then pass the gains it reports as `--roll-white-balance R,G,B` \
             (recipe `roll.white_balance`)."
                .into(),
        );
    }
    if flags.linear_range.is_some() {
        return Some(
            "--linear-range was removed with the print stage: the affine levels remap has \
             no home yet (`nf-scene-correction/levels-knob`, which may retire it). Drop the \
             flag."
                .into(),
        );
    }
    // The retired display tones.
    if let Some(value) = &flags.display_tone {
        let why = match value.as_str() {
            "reinhard" => "extended Reinhard is now the only display tone and is always \
                           applied, so there is nothing to select — drop the flag"
                .to_string(),
            "none" => {
                "the nearest to `none` is the identity, `--display-tone-headroom 0`".to_string()
            }
            "shoulder" => "`shoulder` has no replacement — drop the flag for the default \
                           tone"
                .to_string(),
            other => format!("`{other}` was never a tone — drop the flag"),
        };
        return Some(format!(
            "--display-tone was removed with the `shoulder` and `none` tones: {why}. The \
             tone's one parameter is `--display-tone-headroom` (recipe \
             `fit_range.headroom_stops`). There is no alias."
        ));
    }
    if flags.highlight_compress.is_some() {
        return Some(
            "--highlight-compress was removed with the `shoulder` display tone, whose knee \
             it placed. The remaining tone, extended Reinhard, has no knee: its shape is \
             `--display-tone-headroom` (recipe `fit_range.headroom_stops`). There is no \
             alias."
                .into(),
        );
    }
    None
}

/// The migration error for a removed output selector, or `None` when none was passed.
fn removed_output_message(args: &ConvertArgs) -> Option<String> {
    let flags = &args.removed_output;
    const AXES: &str = "a destination is four separate knobs — --range, --transfer, \
                        --gamut, --container (recipe `output.display`) — or --film-master";
    if let Some(name) = &flags.output_preset {
        return Some(format!(
            "--output-preset was removed with the chain its presets named: {AXES}. {} \
             There is no alias.",
            preset_counterpart(name, args)
        ));
    }
    for (flag, present) in [
        ("--out-depth", flags.out_depth.is_some()),
        ("--output-hdr", flags.output_hdr),
        ("--output-sdr", flags.output_sdr),
        ("--output-profile", flags.output_profile.is_some()),
    ] {
        if present {
            return Some(format!(
                "{flag} was removed: every destination resolves its own depth and embeds \
                 the profile its pixels are in; {AXES} (a float TIFF is `--transfer linear` \
                 or `--film-master`). Drop the flag."
            ));
        }
    }
    if flags.bigtiff.is_some() {
        return Some(
            "--bigtiff was removed: BigTIFF is always decided automatically — a file too \
             large for classic TIFF is written as BigTIFF, and the report says so. Drop the \
             flag."
                .into(),
        );
    }
    None
}

/// The removed presets a destination replaces, and the flags that write it. Each set
/// names **one** destination under either rendering, since this refusal runs before the
/// recipe says which (`the_preset_counterparts_resolve_the_same_under_every_rendering`).
const PRESET_COUNTERPARTS: &[(&str, &str)] = &[
    ("display-p3", "--gamut display-p3"),
    ("film-master", "--film-master"),
    ("hdr-linear-tiff", "--transfer linear"),
    ("hdr-pq-tiff", "--transfer pq"),
    ("hdr-hlg-tiff", "--transfer hlg"),
    ("hdr-pq", "--transfer pq --container avif"),
    ("hdr-hlg", "--transfer hlg --container avif"),
    // Neither is the same file: the map is per-channel and ISO-only, so a reader that
    // knows only the Ultra HDR v1 XMP shows the SDR base.
    ("gain-map-hdr", "--range hdr --container jpeg"),
    ("ultra-hdr-v1", "--range hdr --container jpeg"),
];

/// What replaces a removed output preset, as a sentence — for a name with no counterpart
/// (`legacy`, `custom`, which retired before the chain did, or a typo), how to choose.
fn preset_counterpart(name: &str, args: &ConvertArgs) -> String {
    let Some(&(_, flags)) = PRESET_COUNTERPARTS.iter().find(|(n, _)| *n == name) else {
        return if name == "compatibility" {
            "`compatibility`'s sRGB has no destination yet \
             (`nf-destinations/easy-destination-rows`); the SDR gamuts written are \
             --gamut display-p3 and --gamut adobe-rgb."
                .into()
        } else {
            "Drop the flag, or state the axes you want.".into()
        };
    };
    match name {
        "gain-map-hdr" | "ultra-hdr-v1" => format!(
            "For `{name}`, the nearest is {flags}: its gain map is per-channel and carries \
             ISO 21496-1 metadata only, without the Ultra HDR v1 XMP."
        ),
        // `direct` is refused beside the film master: name the way back when it may be in
        // play (typed, or from a recipe this refusal runs too early to read).
        "film-master" => {
            let direct = args.rendering.rendering == Some(crate::rendering::Rendering::Direct);
            let rendering = if direct {
                ", with --rendering default in place of --rendering direct"
            } else if args.recipe_in.is_some() {
                " (with --rendering default if the recipe states `rendering`: \"direct\")"
            } else {
                ""
            };
            format!("For `{name}`, pass {flags}{rendering}.")
        }
        _ => format!("For `{name}`, pass {flags}."),
    }
}

/// The first retired sigmoid flag the user passed, with its remedy.
fn removed_sigmoid_flag(flags: &RemovedSigmoidFlags) -> Option<(&'static str, &'static str)> {
    const KNEE: &str = "the exponential has no knees, and highlight roll-off belongs to \
                        the display tone (`--display-tone-headroom`)";
    const PLACEMENT: &str = "reference-based placement retired with the reference \
                             density; the exponential's one placement is \
                             `--anchor-mid-offset`, mid-grey a stated density above the \
                             film base";
    [
        (
            "--sigmoid-contrast",
            flags.sigmoid_contrast.is_some(),
            "the decode's slope is `--density-gamma`, the film's linearization, and \
             the picture's contrast is `--contrast`",
        ),
        ("--sigmoid-toe", flags.sigmoid_toe.is_some(), KNEE),
        ("--sigmoid-shoulder", flags.sigmoid_shoulder.is_some(), KNEE),
        (
            "--sigmoid-mid-fraction",
            flags.sigmoid_mid_fraction.is_some(),
            PLACEMENT,
        ),
        (
            "--sigmoid-white-at-d-max",
            flags.sigmoid_white_at_d_max,
            PLACEMENT,
        ),
    ]
    .into_iter()
    .find(|(_, present, _)| *present)
    .map(|(flag, _, remedy)| (flag, remedy))
}

/// The first retired reference-density or anchor flag the user passed, with what it
/// did.
fn removed_dmax_flag(flags: &RemovedDmaxFlags) -> Option<(&'static str, &'static str)> {
    const REFERENCE: &str = "set the roll reference density";
    [
        ("--d-max", flags.d_max.is_some(), REFERENCE),
        ("--fixed-d-max", flags.fixed_d_max, REFERENCE),
        ("--auto-d-max", flags.auto_d_max, REFERENCE),
        ("--no-d-max", flags.no_d_max, REFERENCE),
        (
            "--anchor-white-at-reference",
            flags.anchor_white_at_reference,
            "pinned display white at the reference density",
        ),
        (
            "--anchor-mid-fraction",
            flags.anchor_mid_fraction.is_some(),
            "pinned mid-grey at a fraction of the reference density",
        ),
        (
            "--anchor-black-floor",
            flags.anchor_black_floor.is_some(),
            "pinned the film base to an output floor",
        ),
    ]
    .into_iter()
    .find(|(_, present, _)| *present)
    .map(|(flag, _, what)| (flag, what))
}

/// The migration error for a retired reference-density or anchor flag — shared by
/// `convert` and `estimate --d-max-region` so they say the same thing. The remedy is
/// "drop the flag", which holds everywhere.
fn removed_dmax_message(flag: &str, what: &str) -> String {
    format!(
        "{flag} was removed: it {what}. The roll reference density and the placements \
         that read it are gone. Drop the flag: the anchor is placed from the film base, \
         mid-grey `--anchor-mid-offset D` density above it (default 0.62, slope \
         `--density-gamma`)."
    )
}

/// The migration error for a flag retired with the `characteristic` curve, or `None`
/// when none was passed. Every remedy is to drop the flag.
fn removed_characteristic_message(flags: &RemovedCharacteristicFlags) -> Option<String> {
    const REFERENCE: &str = "The old render is reproducible only from the reference build \
                             (`scripts/reference-snapshot/`)";
    if let Some(value) = &flags.density_curve {
        let why = match value.trim().to_ascii_lowercase().as_str() {
            "characteristic" => format!(
                " The `characteristic` curve inverted a film stock's published curve; the \
                 decode is stock-agnostic now. {REFERENCE}."
            ),
            "sigmoid" => " The `sigmoid` retired before it; highlight roll-off belongs to \
                          the display tone (`--display-tone-headroom`)."
                .to_string(),
            _ => String::new(),
        };
        return Some(format!(
            "--density-curve was removed: the exponential is the only density curve (recipe \
             `reconstruction.curve`), so there is nothing to select — drop the flag.{why}"
        ));
    }
    if flags.film_stock.is_some() {
        return Some(format!(
            "--film-stock was removed with the `characteristic` curve it chose a stock for: \
             the decode is stock-agnostic. Drop the flag. {REFERENCE}."
        ));
    }
    if flags.preset.is_some() {
        return Some(format!(
            "--preset was removed: its bundles (`characteristic-generic`, `-stock`, `-aim`, \
             and earlier `sigmoid-knees` / `-flat`) set retired curves with an exposure \
             calibrated to them, and no bundle replaces them. Drop the flag, and set a knob \
             you want directly — `--exposure`, `--contrast`, `--channel-grade`, \
             `--highlight-desaturation` — or collect them in a `--params` recipe. {REFERENCE}."
        ));
    }
    None
}

/// The first removed regional-balance flag present, if any.
fn removed_balance_flag(flags: &RemovedBalanceFlags) -> Option<&'static str> {
    [
        ("--shadow-balance", flags.shadow_balance.is_some()),
        ("--highlight-balance", flags.highlight_balance.is_some()),
        ("--balance-range", flags.balance_range.is_some()),
        ("--auto-balance-range", flags.auto_balance_range),
    ]
    .into_iter()
    .find_map(|(flag, present)| present.then_some(flag))
}

/// Why the regional balance retired and what replaces it. The substantive difference
/// is the measurement, not the spelling: say so rather than presenting the grade as a
/// rename.
const REGIONAL_BALANCE_RETIRED: &str = "per-channel density offsets ramped between a \
     shadow and a highlight density, whose `auto` range was measured on every frame (so \
     a roll stayed consistent only by measuring once and replaying the range), and \
     which nothing bounded, so a large enough difference between its ends folded two \
     densities onto one. Its successor is the look's per-channel grade, `--channel-grade \
     R,B` (recipe `look.channel_grade`): pivoted at a fixed mid-grey, so it measures \
     nothing, and bounded so it stays monotone. It acts on the working space's channels \
     rather than on film density, so the old values do not carry over";

/// Which input axes were asserted via a **CLI flag** (vs the recipe) — threaded
/// into [`convert_frame`] so the resolver records literal CLI-vs-recipe
/// provenance. `roll` has no per-frame input flags, so it passes
/// [`InputFromCli::none`].
#[derive(Clone, Copy, Debug, Default)]
struct InputFromCli {
    transfer: bool,
    meaning: bool,
}

impl InputFromCli {
    /// No CLI input assertions (the recipe-driven `roll` case).
    fn none() -> Self {
        Self::default()
    }
}

/// Build the resolver's [`ContainerColorFacts`] from what the decoder parsed.
///
/// `io::decode` accepts *any* 3-channel 16-bit chunky RGB TIFF, not only genuine
/// SilverFast scans, so raw-mode provenance is derived from the authoritative
/// SilverFast **XMP mode metadata** ([`DecodeInfo::is_silverfast_raw_mode`],
/// `Company=LaserSoft Imaging` + `HDRScan=Yes`) rather than assumed or keyed on a
/// spoofable `Software` string / IR-plane presence: a generic / colorimetric /
/// processed RGB16 TIFF gets `raw_mode: None`, so its meaning resolves `Unknown`
/// and `convert` rejects it (unless the user explicitly asserts the axes). The
/// XMP `Gamma` feeds the descriptive-transfer axis — `Gamma≈1` corroborates
/// linear; a non-linear gamma on a raw-mode scan makes the transfer ambiguous
/// (contradiction → `Unknown` → rejected). `embedded_icc` is passed through for
/// inspection.
fn container_color_facts(info: &DecodeInfo) -> ContainerColorFacts {
    ContainerColorFacts {
        raw_mode: info
            .is_silverfast_raw_mode()
            .then_some(RawMode::SilverFastHdr),
        gamma: info
            .silverfast_xmp
            .as_ref()
            .map(|x| x.gamma.clone())
            .unwrap_or_default(),
        embedded_icc: info.embedded_icc.clone(),
    }
}

/// Reject a SilverFast **positive-mode** scan (`Negative=No`) loudly. Such a scan
/// is still raw linear scanner data, so it passes the transfer/meaning gate — but
/// converting it as a *negative* is silently wrong. This is a small,
/// clearly-scoped check (distinct from the transfer/meaning resolution) so it is
/// easy to lift when positive-mode support lands. `inspect` never calls it (it
/// reports the `Negative` flag via `decode.silverfast_xmp` instead).
fn reject_positive_mode(info: &DecodeInfo) -> Result<()> {
    if info.is_silverfast_positive_mode() {
        return Err(NcError::Unsupported(
            "input is a SilverFast positive-mode scan (XMP Negative=No); converting it as a \
             negative would be silently wrong. Positive-mode scans are not yet supported \
             (follow-up); scan in negative mode, or convert a negative scan."
                .into(),
        ));
    }
    Ok(())
}

/// The merged input assertions plus their CLI/recipe provenance, for the resolver.
fn input_assertions(input: &InputParams, from_cli: InputFromCli) -> InputAssertions {
    InputAssertions {
        transfer: input.transfer,
        meaning: input.meaning,
        transfer_from_cli: from_cli.transfer,
        meaning_from_cli: from_cli.meaning,
    }
}

/// Everything one frame's pipeline produced, for the orchestrator to emit or
/// aggregate. `convert` reads the report and `loss` (for its telemetry event); `roll`
/// reads only `report` (telemetry is `convert`-only, design-spec §9).
struct ConvertedFrame {
    report: Report,
    loss: EncodeReport,
}

/// What a frame learned before it ended, owned by the caller so a frame that fails
/// still hands it back: `roll` reports the preflight's decision, and `convert`'s
/// telemetry event the decode's facts and the stages' times.
#[derive(Default)]
struct FrameFacts {
    memory: Option<MemoryReport>,
    info: Option<DecodeInfo>,
    /// The stages' wall clocks; `total` stays `0.0` for the orchestrator to fill.
    clock: telemetry::StageTimer,
}

/// Run the memory preflight for one input and fold its outcome into the run:
/// probe the file's shape from headers alone, size the run for `profile` +
/// `sampling`, reject loudly when it would exceed `budget` (exit 6), and push the
/// RAM-pressure warning when it fits the budget but not the machine.
///
/// Shared by `convert`/`roll` and by `inspect`/`estimate` so the four commands
/// gate identically — each with its own profile, since `inspect`/`estimate` stop
/// after decode and must not be judged on a render they never run, and its own
/// [`SamplePlan`], since the film-base phase's cost depends on which rectangles the
/// run samples.
///
/// `total_ram` is a parameter rather than a `detect_total_ram()` call inside, so
/// the warn tier — the one piece of this gate that is environment-dependent, and
/// therefore the one that can make `--strict` exit differently on two machines — is
/// reachable from a test through the real wiring. Production callers pass
/// [`memory::detect_total_ram`].
fn preflight_memory(
    input: &Path,
    profile: RunProfile,
    sampling: SamplePlan,
    budget: memory::Budget,
    total_ram: Option<u64>,
    log: &Log,
    warnings: &mut Vec<String>,
) -> Result<MemoryReport> {
    let shape = probe(input)?;
    let mem = memory::preflight(&shape, profile, sampling, budget, total_ram)?;
    log.info(format_args!(
        "memory preflight: estimated peak {} bytes, budget {} bytes ({:?})",
        mem.estimate.estimated_peak_bytes, mem.budget_bytes, mem.budget_source
    ));
    if let Some(msg) = memory::warn_message(&mem) {
        push_warning_buf(warnings, log, msg);
    }
    Ok(mem)
}

/// The film-base sampling a resolved [`FilmBaseSource`] will perform, for the
/// memory model's film-base phase: an explicit base reads no pixels, a region
/// materializes exactly its rectangle, and `auto` materializes the frame interior
/// (`film_base::auto_interior_pixels`, resolved inside the model against the probed
/// shape). `estimate --grid` substitutes its own rectangle.
fn sample_plan(source: &FilmBaseSource) -> SamplePlan {
    match source {
        FilmBaseSource::Explicit(_) => SamplePlan::none(),
        FilmBaseSource::Region([_, _, w, h]) => SamplePlan::rect(*w as u64 * *h as u64),
        FilmBaseSource::Auto => SamplePlan::auto(),
    }
}

/// The per-frame conversion core: **stage-0 memory preflight** → decode →
/// film-base estimate → render → optional IR export → encode. Pure of the operational
/// concerns the callers layer on top (`--strict` gating, report emission, telemetry),
/// so `convert` and `roll` share one byte-for-byte identical frame path.
///
/// The one operational concern it *does* own is the memory gate
/// ([`preflight_memory`]), and deliberately: it must run per frame and
/// immediately before this function's own `decode_within`, which needs the same
/// budget anyway. Do not "tidy" it up into the orchestrators — that would split
/// the run's validation across two layers and leave the budget threaded here
/// regardless.
///
/// The caller must have already validated the recipe ([`validate_convert`], or
/// `roll`'s composition of the same rules), rejected the deprecated input flags
/// ([`reject_deprecated_input_flags`]), and checked write-target collisions; apart
/// from the memory gate above, `convert_frame` assumes a sound recipe and a safe
/// `output` path. It resolves and gates the input color semantics itself (transfer +
/// meaning, [`input_semantics`]) after decode, before the render. It never writes to
/// stdout (the report rides back in [`ConvertedFrame`]); progress and warnings go to
/// stderr via `log`.
///
/// Warnings are accumulated into the caller-owned `warnings` buffer (echoed to
/// stderr as they occur) so they survive an early failure: on success they are
/// also moved into the returned report, but on the `Err` path they stay in the
/// caller's buffer — the roll orchestrator attaches them to a failed frame's
/// report. The caller decides whether `--strict` promotes them. `facts` carries the
/// preflight's decision, the decode's facts and the stage clock back the same way
/// and for the same reason: a frame that passed the gate and then failed later is
/// exactly where a reader wants the estimate, so it must not be lost with the
/// returned report.
// One over clippy's argument cap: the orchestration core legitimately threads the
// frame identity, the recipe, the run's memory budget, and the two report-provenance
// out-params; a struct wrapping a handful of one-off values would only obscure the
// call sites.
#[allow(clippy::too_many_arguments)]
fn convert_frame(
    command: &'static str,
    input: &Path,
    output: &Path,
    recipe: &Recipe,
    input_from_cli: InputFromCli,
    // Files this run *read* besides the scan (`--params`, a roll's `--frames`), so a
    // cleanup never removes one — see `render_frame`.
    read_inputs: &[&Path],
    budget: memory::Budget,
    facts: &mut FrameFacts,
    log: &Log,
    warnings: &mut Vec<String>,
) -> Result<ConvertedFrame> {
    // `calibration.film_base` has no default, and the gate rejects `None` before any
    // frame runs (`validate_shared`, for `convert` and each `roll` frame). Restating it
    // here keeps this function total rather than relying on an `unwrap` whose safety
    // lives in another module — sharing `missing_film_base_message` so this unreachable
    // spelling cannot drift into a second, thinner diagnosis of the same condition.
    let base_source = recipe.calibration.film_base.clone().ok_or_else(|| {
        NcError::Usage(missing_film_base_message(FilmBaseRemedy::for_command(
            command,
        )))
    })?;
    // Validated before anything was decoded; resolved again here from the same recipe,
    // so what renders is what was checked.
    let destination = recipe::destination(recipe, KnobNames::FlagAndKey)?;

    let mut report = Report {
        command: Some(command),
        identity: Some(Identity::new().with_params_hash(recipe.params_hash())),
        input: Some(input.to_path_buf()),
        output: Some(output.to_path_buf()),
        film_base_source: Some(base_source.clone()),
        ..Report::default()
    };

    // Stage 0 — memory preflight, on a metadata-only header probe. This must run
    // *before* decode allocates: the whole point is to reject an oversized frame
    // while the heap is still empty (a check after decode would OOM on exactly the
    // inputs it exists to catch). Over budget ⇒ loud exit 6; within budget but
    // most of the machine's RAM ⇒ a `--strict`-promotable warning. Operational
    // gate — it never touches a pixel, so the output stays deterministic.
    let mem = preflight_memory(
        input,
        run_profile(destination, recipe.input.export_ir.is_some()),
        sample_plan(&base_source),
        budget,
        memory::detect_total_ram(),
        log,
        warnings,
    )?;
    // Out-param first, so the diagnostic survives a later failure on this frame:
    // the roll orchestrator attaches it to the frame report either way (a frame that
    // passed the gate and then failed is exactly where a reader wants the estimate).
    facts.memory = Some(mem);
    report.memory = Some(mem);

    // Stage 1 — decode. The stage clock (`StageTimer`) feeds telemetry only and never
    // touches the image; it runs whether or not telemetry is enabled, so the render
    // path is uniform.
    // `decode_for_roll_white` (`measure-roll`) repeats this front half — preflight,
    // decode, input semantics, positive-mode refusal, effective area and its warnings —
    // up to the chain; a gate added here belongs there too.
    let clock = &mut facts.clock;
    let (image, info) = clock.time(StageKind::Decode, || decode_within(input, budget.bytes()))?;
    let info: &DecodeInfo = facts.info.insert(info);
    log.info(format_args!(
        "decoded {:?} {}x{} (ir={})",
        info.format, info.width, info.height, info.ir_present
    ));
    for w in &info.warnings {
        push_warning_buf(warnings, log, w.clone());
    }

    // Stage 1b — resolve input color semantics (transfer + measurement meaning as
    // independent axes) and gate: only a supported linear transfer + scanner-device
    // meaning may enter Dmin/density. An explicit assertion contradicting container
    // structure is a usage error here; an ambiguous/unsupported input is a loud
    // unsupported error — never a quietly-wrong image. The resolution rides into
    // the report (with evidence + a safe ICC summary) regardless.
    let input_meta = input_semantics::resolve(
        &container_color_facts(info),
        &input_assertions(&recipe.input, input_from_cli),
    )?;
    let input_report = InputColorReport::from_metadata(&input_meta);
    if input_report.icc_unparsable() {
        push_warning_buf(
            warnings,
            log,
            "embedded ICC profile present but could not be parsed for a summary".into(),
        );
    }
    input_semantics::require_convertible(&input_meta)?;
    report.input_color = Some(input_report);

    // A SilverFast positive-mode scan passes the transfer/meaning gate (it is raw
    // linear scanner data) but must not be converted as a negative — reject it
    // loudly with a distinct message rather than silently misconvert.
    reject_positive_mode(info)?;

    // `--export-ir` on a scan with no IR plane can't be honored: fail fast,
    // before writing any output, rather than after the main encode.
    let export_ir = recipe.input.export_ir.as_deref().map(PathBuf::from);
    if export_ir.is_some() && !info.ir_present {
        return Err(NcError::Unsupported(
            "--export-ir requested but the input has no IR plane (HDRi input only)".into(),
        ));
    }
    // Whether film-base estimation will actually consume the IR plane for holder
    // detection: on a scan carrying a **marker-verified** IR plane that measures
    // usable on this frame, and only when the base is being auto-detected (an
    // explicit `--film-base` / `--base-region` runs no detection at all). A
    // shape-only IR plane (unverified provenance) is not trusted, so it degrades to
    // RGB-only. These govern the two fallback notes below; whether the plane was
    // *actually* consumed is read off stage 2 afterwards, not predicted here.
    let auto_base = matches!(base_source, FilmBaseSource::Auto);
    let ir_shape_only = info.ir_present && !image.ir_verified;
    // Whether the IR plane can separate holder from film **on this frame**. A
    // measurement, not the `--film-type chromogenic` declaration that used to gate
    // this (`ir-usability-detection`): chemistry mispredicts separability in both
    // directions, so the plane is asked directly. `None` when there is no IR plane.
    //
    // Measured here even though `ir_holder_mask` measures again inside stage 2: the
    // fallback note below must fire even when `estimate` then *errors* — auto
    // refusing to find a rebate is exactly when "the IR plane could not help" is
    // worth reading — and a failed stage 2 returns no `BaseEstimate` to carry it.
    // Both calls are bounded strided samples, so the duplication is ~100k reads.
    let ir_separability = clock.time(StageKind::FilmBase, || {
        Ok(film_base::ir_separability(&image))
    })?;
    let ir_usable = ir_separability.is_some_and(|s| s.usable);

    // When holder detection wanted the IR mask but the plane is shape-only, it
    // silently degraded to RGB-only — say so (a `--strict`-promotable warning, like
    // the no-IR note). Emitting it here means the generic "carried but unused" note
    // below is skipped for the same plane, so only one IR note fires. Suppressed
    // under `--export-ir` for the same reason that note is: the user is taking the
    // plane themselves, and failing their `--strict` run over a detection path that
    // fell back to the one every 48-bit scan uses silently would be wrong.
    let shape_only_holder_note = auto_base && ir_shape_only && export_ir.is_none();
    if shape_only_holder_note {
        push_warning_buf(
            warnings,
            log,
            "an IR plane is present, but it is identified by shape alone (no \
             NewSubfileType=4 marker) and not trusted for holder detection; using \
             RGB-only film-holder detection for the film base"
                .into(),
        );
    }

    // Trusted IR that still cannot do the job: this frame's own film is too opaque
    // to tell from the holder (a fully-exposed frame on silver stock, say). Say so
    // with the measurement, so the fallback is diagnosable rather than silent.
    let ir_unusable_note =
        auto_base && info.ir_present && image.ir_verified && !ir_usable && export_ir.is_none();
    if ir_unusable_note {
        push_warning_buf(
            warnings,
            log,
            format!(
                "the IR plane cannot separate the film holder on this frame (interior \
                 IR transmission {:.4}); using RGB-only film-holder detection for the \
                 film base",
                ir_separability.map_or(0.0, |s| s.interior_median)
            ),
        );
    }

    // Stage 2 — film-base estimate. Resolved before the render so its quality
    // warnings (non-uniform region, cross-edge disagreement) are pushed — and so
    // echoed to stderr — *before* the fallible render runs, and ride out in the
    // JSON report on a successful run. (A hard render failure propagates its error
    // and exit code like every other error path and emits no report; the stderr
    // warnings still stand.)
    let base = clock.time(StageKind::FilmBase, || {
        film_base::estimate(&image, &base_source)
    })?;
    report.film_base = Some(base.base);
    for w in base.warnings {
        push_warning_buf(warnings, log, w);
    }

    // The effective measurement area, resolved **unconditionally** and always
    // reported: that is what makes `--measure-inset` observable rather than
    // accepted-and-ignored, which the project forbids. The march costs less than
    // run-to-run noise even on an 18.7 MP frame, so there is nothing to save by
    // skipping it.
    //
    // Nothing in a `convert` measures over it today — its one per-frame consumer, the
    // auto reference density, retired (`nf-retire/dmax-machinery`), and the roll's
    // white balance is measured over it by `hanten measure-roll` instead —
    // so an **empty** region is a warning rather than a refusal, with no
    // `report.effective_area`, because there is no region to report. A consumer added
    // here must decide whether an empty region becomes fatal for it.
    // Timed as a success whatever it returns: an empty region is a warning, not the
    // run's failure.
    let area = clock.time(StageKind::FilmBase, || {
        Ok(film_base::effective_area(&image, recipe.measure.inset))
    })?;
    match area {
        Ok(area) => {
            report.effective_area = Some(area);
            for w in film_base::effective_area_warnings(&area) {
                push_warning_buf(warnings, log, w);
            }
        }
        // Rebuilt from the error's own text rather than `Display`, which prefixes the
        // kind (`usage: …`) — a warning must not carry it.
        Err(e) => {
            push_warning_buf(
                warnings,
                log,
                format!(
                    "{} Nothing in this conversion measures over the region, so the \
                     render is unaffected and the report omits `effective_area` \
                     (--measure-inset has no effect on this run).",
                    e.message()
                ),
            );
        }
    }

    // Note an IR plane that's carried but not consumed. Keyed on what each stage
    // actually did, never on a prediction from the inputs: a marker-verified plane
    // that measures usable can still produce no mask (the all-holder fallback), and
    // predicting consumption silently suppressed this warning — and so `--strict` —
    // on exactly that case.
    //
    // The measurement area never suppresses it: nothing in the render reads the region,
    // so a marched holder (`effective_area.holder_applied`) reaches no pixel. That is
    // also why the wording is about the **conversion** and names `effective_area`:
    // `convert` resolves the area unconditionally, so a run legitimately reports a
    // measured `holder_applied: true` beside this note.
    //
    // Not emitted when the plane is being exported
    // (`--export-ir` is the user handling it, so warning — and failing under
    // `--strict` — would be wrong; keeps `--strict --export-ir` usable on the
    // primary HDRi format), and not when one of the two fallback notes above
    // already covered this plane.
    if info.ir_present
        && export_ir.is_none()
        && !base.ir_mask_applied
        && !shape_only_holder_note
        && !ir_unusable_note
    {
        push_warning_buf(
            warnings,
            log,
            "input carries an IR plane; it is preserved but not used in the \
             conversion — no rendered pixel depends on it, whatever \
             `effective_area.holder` measured (use --export-ir to write it out)"
                .into(),
        );
    }

    render_frame(
        DecodedFrame {
            recipe,
            destination,
            image,
            base: base.base,
            export_ir,
            output,
            report,
            read_inputs,
        },
        clock,
        log,
        warnings,
    )
}

/// Fold an encode's loss and output statistics into the report, warning on any
/// loss. Shared by both flows' encode sites so the loss is described one way.
fn report_encode_outcome(
    report: &mut Report,
    outcome: &EncodeOutcome,
    log: &Log,
    warnings: &mut Vec<String>,
) {
    let loss = outcome.loss;
    report.loss = Some(loss);
    // Report-only statistics of the samples as written — the numeric basis a
    // cross-version `compare` diffs (per-channel mean ΔRGB). Measured *after* the
    // pixels are final, from the same data the encoder wrote.
    report.output_stats = Some(outcome.stats);
    if loss.any_loss() {
        push_warning_buf(
            warnings,
            log,
            format!(
                "output lost {} clipped and {} non-finite of {} samples ({:.2}%)",
                loss.clipped_total(),
                loss.non_finite,
                loss.total_samples,
                loss.loss_fraction() * 100.0,
            ),
        );
    }
    // A non-finite sample is a numerical fault, not routine gamut clipping — make
    // sure it is never fully silenced (the `--quiet --report none` combination
    // would otherwise suppress both channels of the warning above).
    if loss.non_finite > 0 && log.quiet {
        eprintln!(
            "hanten: warning: {} non-finite (NaN/inf) output sample(s) — numerical fault",
            loss.non_finite
        );
    }
}

/// The AVIF report block, and the brand-downgrade warning. It reads only what
/// the AVIF encoder resolved.
fn report_avif(
    report: &mut Report,
    summary: &avif::AvifSummary,
    log: &Log,
    warnings: &mut Vec<String>,
) {
    // A general-brand-only file is valid but is never advertised as Advanced
    // Profile, and the downgrade is surfaced (and `--strict`-promotable) rather
    // than left for someone to discover by inspecting brands.
    let profile_reason = match &summary.profile {
        avif::AvifProfile::Advanced => None,
        avif::AvifProfile::GeneralOnly { reason } => {
            push_warning_buf(
                warnings,
                log,
                format!(
                    "AVIF written without the MA1A brand (not AVIF v1.2 Advanced \
                     Profile): {reason}"
                ),
            );
            Some(reason.clone())
        }
    };
    report.avif = Some(AvifResult {
        profile: match summary.profile {
            avif::AvifProfile::Advanced => "advanced",
            avif::AvifProfile::GeneralOnly { .. } => "general-brand-only",
        },
        profile_reason,
        bit_depth: summary.bit_depth,
        seq_profile: summary.seq_profile,
        seq_level_idx: summary.seq_level_idx,
        level: avif::level_name(summary.seq_level_idx),
        cicp: [summary.cicp.0, summary.cicp.1, summary.cicp.2],
        full_range: summary.full_range,
        codestream_bytes: summary.codestream_bytes,
        rendering: AvifRenderingResult {
            reference_white_nits: summary.metadata.linear.reference_white_nits,
            target_peak_nits: summary.metadata.linear.target_peak_nits,
            linear_headroom: summary.metadata.linear.linear_headroom,
            tone_curve: summary.metadata.linear.tone_curve,
            gamut_mapping: summary.metadata.linear.gamut_mapping,
            linear_domain: summary.metadata.linear.linear_domain,
            hlg_system_gamma: summary.metadata.hlg_system_gamma,
            hlg_reference_display_peak_nits: summary.metadata.hlg_reference_display_peak_nits,
            hlg_reference_display_black_nits: summary.metadata.hlg_reference_display_black_nits,
        },
    });
}

/// The coded-HDR TIFF report block, and the BigTIFF note. It reads only what the
/// encoder resolved.
fn report_hdr_coded_tiff(
    report: &mut Report,
    summary: &encode::HdrCodedTiffSummary,
    log: &Log,
    warnings: &mut Vec<String>,
) {
    // Same reason as the linear TIFF below: the profile is built inside the
    // encode arm, so the `auto` BigTIFF promotion is reported from what the
    // encoder resolved rather than predicted before it.
    if summary.bigtiff {
        push_warning_buf(
            warnings,
            log,
            "output promoted to BigTIFF (would exceed the classic 4 GiB TIFF limit)".into(),
        );
    }
    let metadata = summary.metadata;
    report.hdr_coded_tiff = Some(HdrCodedTiffResult {
        pixel_contract: summary.pixel_contract,
        bits_per_sample: summary.bits_per_sample,
        sample_format: summary.sample_format,
        bigtiff: summary.bigtiff,
        icc_bytes: summary.icc_bytes,
        // Deliberately **not** `metadata.cicp_matrix_coefficients`: that is the
        // AVIF value (9, Y'CbCr). An RGB ICC profile requires 0, and the profile
        // this file embeds writes 0 — so the report states what the artifact
        // carries, not what the renderer declared for a different container.
        cicp: [metadata.cicp_color_primaries, metadata.cicp_transfer, 0],
        full_range: metadata.full_range,
        max_quantization_error_codes: summary.max_quantization_error_codes,
        rms_quantization_error_codes: summary.rms_quantization_error_codes,
        reference_white_nits: metadata.linear.reference_white_nits,
        target_peak_nits: metadata.linear.target_peak_nits,
        tone_curve: metadata.linear.tone_curve,
        // PQ only, for the same reason `io::avif` omits `clli` on HLG: HLG is
        // display-referred, so absolute content-light values would be a false
        // claim rather than a missing one.
        max_cll_nits: match metadata.transfer {
            hdr::HdrTransfer::Pq => Some(metadata.content_light.max_cll_nits),
            hdr::HdrTransfer::Hlg => None,
        },
        max_fall_nits: match metadata.transfer {
            hdr::HdrTransfer::Pq => Some(metadata.content_light.max_fall_nits),
            hdr::HdrTransfer::Hlg => None,
        },
        hlg_system_gamma: metadata.hlg_system_gamma,
        hlg_reference_display_peak_nits: metadata.hlg_reference_display_peak_nits,
        hlg_reference_display_black_nits: metadata.hlg_reference_display_black_nits,
        interoperability: "16-bit is TIFF's quantization, not one of BT.2100's specified bit \
             depths (10 and 12): the file carries BT.2100's transfer function at \
             TIFF's precision. The stored code values are exact and the single \
             quantization step is reported above. Automatic HDR presentation is \
             not claimed — TIFF has no CICP tag of its own, so the signalling \
             lives in the embedded ICC profile's `cicp` tag, which only a \
             CICP-aware colour-managed reader honours; treat this as \
             limited-interoperability interchange rather than a display-ready \
             deliverable, and see the AVIF or gain-map destinations for delivery",
    });
}

/// The linear-HDR TIFF report block, and the BigTIFF note. It reads only what the
/// encoder resolved.
fn report_hdr_linear_tiff(
    report: &mut Report,
    summary: &encode::HdrLinearTiffSummary,
    log: &Log,
    warnings: &mut Vec<String>,
) {
    // Reported after the write, from what the encoder resolved: its ICC is built
    // inside the encode arm, so the file's size is known only there.
    if summary.bigtiff {
        push_warning_buf(
            warnings,
            log,
            "output promoted to BigTIFF (would exceed the classic 4 GiB TIFF limit)".into(),
        );
    }
    let linear = summary.linear;
    report.hdr_linear_tiff = Some(HdrLinearTiffResult {
        pixel_contract: summary.pixel_contract,
        bits_per_sample: summary.bits_per_sample,
        sample_format: summary.sample_format,
        bigtiff: summary.bigtiff,
        icc_bytes: summary.icc_bytes,
        reference_white_sample: 1.0,
        reference_white_nits: linear.reference_white_nits,
        target_peak_nits: linear.target_peak_nits,
        linear_headroom: linear.linear_headroom,
        tone_curve: linear.tone_curve,
        gamut_mapping: linear.gamut_mapping,
        linear_domain: linear.linear_domain,
        max_cll_nits: summary.content_light.max_cll_nits,
        max_fall_nits: summary.content_light.max_fall_nits,
        interoperability: "the embedded ICC profile states the BT.2020/D65 \
                           primaries and the linear transfer only; its PCS stops \
                           at the media white, so the reference-white, peak and \
                           headroom values in this block — not the profile — \
                           define the luminance semantics of these samples",
    });
}

/// Whether `path` holds one of nc's sidecars, recognised by its provenance rather
/// than by key names: the `{meta, params}` envelope with `params` an object and
/// `meta` carrying the identity every sidecar stamps (`nc_version`,
/// `pipeline_version`, `target`). Deleting is destructive, so anything short of that
/// — missing, unreadable, or a different file that merely shares the shape — is not
/// ours to remove.
fn is_nc_sidecar(path: &Path) -> bool {
    let Some(doc) = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
    else {
        return false;
    };
    let meta = &doc["meta"];
    doc.as_object().is_some_and(|o| o.len() == 2)
        && doc["params"].is_object()
        && meta["nc_version"].is_string()
        && meta["pipeline_version"].is_u64()
        && meta["target"].is_string()
}

/// The memory profile a destination is sized with: one per shape of buffers
/// it holds, not one per destination (`nf-destinations/memory-profiles` measures them).
fn run_profile(destination: recipe::Destination, export_ir: bool) -> RunProfile {
    match destination {
        recipe::Destination::FilmMaster => RunProfile::F32Tiff { export_ir },
        recipe::Destination::Display(d) => match d.encoding {
            Encoding::SdrTiff | Encoding::HdrCodedTiff(_) => RunProfile::U16Tiff { export_ir },
            Encoding::HdrLinearTiff => RunProfile::F32Tiff { export_ir },
            Encoding::HdrAvif(_) => RunProfile::Avif { export_ir },
            Encoding::GainMapJpeg => RunProfile::GainMapJpeg { export_ir },
        },
    }
}

/// The chain's render for its destination, before the encode.
enum DestinationRender {
    /// The fixed decode's linear ACEScg, and the profile naming it.
    FilmMaster { image: LinearImage, icc: Vec<u8> },
    /// Through the chain: what it applied, and the pixels its destination encodes.
    /// Boxed: the account and the HDR metadata dwarf the film master's two handles.
    Rendered {
        rendered: Box<ChainAccount>,
        pixels: DestinationPixels,
    },
}

/// What the chain applied, kept once its image has moved to the encoder — the report's
/// account of the run.
struct ChainAccount {
    applied: [(StageKind, &'static str); 4],
    scene_correction: scene_correction::SceneCorrection,
    look: look::LookSection,
    fit_range: fit_range::FitRange,
}

/// A rendered destination's pixels, in its encoder's input type. One arm per
/// `destination::Encoding`, matched exhaustively on both sides.
enum DestinationPixels {
    /// The destination's display curve applied, with its ICC profile.
    Sdr { image: LinearImage, icc: Vec<u8> },
    /// Display-linear BT.2020, clamped to the peak.
    HdrLinear(hdr::LinearBt2020Hdr, hdr::PeakClamp),
    /// A Rec.2100 signal for the 16-bit TIFF.
    HdrCoded(hdr::RenderedHdr, hdr::PeakClamp),
    /// A Rec.2100 signal for the AVIF.
    HdrAvif(hdr::RenderedHdr, hdr::PeakClamp),
    /// The SDR base with the display curve applied and its ICC profile, and the gain
    /// map to the HDR rendition clamped to `headroom`.
    GainMap {
        base: LinearImage,
        icc: Vec<u8>,
        map: gain_encode::GainMapImage,
        headroom: f32,
        clamp: hdr::PeakClamp,
        report: GainMapResult,
    },
}

impl DestinationRender {
    /// What the HDR hand-off clamped to the peak, for an HDR destination.
    fn peak_clamp(&self) -> Option<hdr::PeakClamp> {
        match self {
            Self::Rendered {
                pixels:
                    DestinationPixels::HdrLinear(_, clamp)
                    | DestinationPixels::HdrCoded(_, clamp)
                    | DestinationPixels::HdrAvif(_, clamp)
                    | DestinationPixels::GainMap { clamp, .. },
                ..
            } => Some(*clamp),
            Self::Rendered {
                pixels: DestinationPixels::Sdr { .. },
                ..
            }
            | Self::FilmMaster { .. } => None,
        }
    }

    /// The measured content light of an HDR rendition, for the "nothing above
    /// reference white" warning.
    fn hdr_content_light(&self) -> Option<hdr::ContentLightLevel> {
        match self {
            Self::Rendered { pixels, .. } => match pixels {
                DestinationPixels::HdrLinear(h, _) => Some(h.content_light()),
                DestinationPixels::HdrCoded(r, _) | DestinationPixels::HdrAvif(r, _) => {
                    Some(r.metadata().content_light)
                }
                // A gain map's deliverable is an SDR base plus a map, so an HDR
                // rendition near reference white makes a flat map, not a mislabelled
                // container: `gain_map.flat` states it (a flat map is no mislabelled container).
                DestinationPixels::Sdr { .. } | DestinationPixels::GainMap { .. } => None,
            },
            Self::FilmMaster { .. } => None,
        }
    }

    /// Samples of a second rendition whose clamps [`peak_clamp`](Self::peak_clamp)
    /// counts: the gain map's HDR rendition, beside the SDR base the encoder counts.
    /// Zero for a single-rendition destination, whose clamp and encode count the same
    /// samples.
    fn second_rendition_samples(&self) -> u64 {
        match self {
            Self::Rendered {
                pixels: DestinationPixels::GainMap { base, .. },
                ..
            } => base.rgb.len() as u64,
            Self::Rendered { .. } | Self::FilmMaster { .. } => 0,
        }
    }

    /// What the gain-map destination wrote, for the report.
    fn gain_map(&self) -> Option<GainMapResult> {
        match self {
            Self::Rendered {
                pixels: DestinationPixels::GainMap { report, .. },
                ..
            } => Some(*report),
            Self::Rendered { .. } | Self::FilmMaster { .. } => None,
        }
    }

    /// The depth an `--export-ir` plane is written at: the primary's — f32 beside a
    /// float TIFF, u16 otherwise.
    fn ir_depth(&self) -> OutDepth {
        match self {
            Self::FilmMaster { .. }
            | Self::Rendered {
                pixels: DestinationPixels::HdrLinear(..),
                ..
            } => OutDepth::F32,
            Self::Rendered { .. } => OutDepth::U16,
        }
    }
}

/// Render the fixed decode's ACEScg for `destination`: nothing for the film master; the
/// chain, then the destination's transfer, for a rendered one. A rendered destination
/// also decodes the film base itself, one pixel through the same decode and 3×3 —
/// display black's reference (`chain::render`); the film master never reads it.
fn render_destination(
    aces: AcesCgImage,
    base: &FilmBase,
    recipe: &Recipe,
    destination: recipe::Destination,
    clock: &mut impl StageClock,
) -> Result<DestinationRender> {
    let d = match destination {
        recipe::Destination::FilmMaster => {
            // Profile only — no transform: the tag names the space the pixels are in.
            let icc = clock.time(StageKind::Destination, || {
                color::icc_profile(&color::OutputSpace::AcesCg)
            })?;
            return Ok(DestinationRender::FilmMaster {
                image: aces.into_linear(),
                icc,
            });
        }
        recipe::Destination::Display(d) => d,
    };
    let film_base = clock.time(StageKind::Reconstruction, || {
        fixed::decode_film_base(base, &recipe.reconstruction).map(working_space::map_nc_film_rgb_v1)
    })?;
    // One match on the encoding, so a new row cannot reach an encoder it was not written
    // for: the gain map renders a pair, every other destination one rendition.
    match d.encoding {
        Encoding::GainMapJpeg => render_gain_map(aces, film_base, recipe, d, clock),
        Encoding::SdrTiff => render_one(aces, film_base, recipe, d, clock, |r| {
            let (image, icc) = color::encode_display_linear(r.linear, r.gamut)?;
            Ok(DestinationPixels::Sdr { image, icc })
        }),
        Encoding::HdrLinearTiff => render_one(aces, film_base, recipe, d, clock, |r| {
            let (hdr, clamp) = r.bt2020()?;
            Ok(DestinationPixels::HdrLinear(hdr, clamp))
        }),
        Encoding::HdrCodedTiff(transfer) => render_one(aces, film_base, recipe, d, clock, |r| {
            let (hdr, clamp) = r.bt2020()?;
            Ok(DestinationPixels::HdrCoded(
                hdr::encode_transfer(hdr, transfer)?,
                clamp,
            ))
        }),
        Encoding::HdrAvif(transfer) => render_one(aces, film_base, recipe, d, clock, |r| {
            let (hdr, clamp) = r.bt2020()?;
            Ok(DestinationPixels::HdrAvif(
                hdr::encode_transfer(hdr, transfer)?,
                clamp,
            ))
        }),
    }
}

/// One rendition out of the chain, as a single-rendition encoder receives it.
struct OneRendition {
    linear: LinearImage,
    gamut: DestinationGamut,
    /// Fit range's and fit gamut's `applied`, which an HDR report names.
    tone_curve: &'static str,
    gamut_mapping: &'static str,
}

impl OneRendition {
    /// The HDR hand-off. Every single-rendition HDR encoding is a BT.2020 one; the
    /// destination table pairs them, and this names the break rather than encoding
    /// other primaries under a BT.2020 tag.
    fn bt2020(self) -> Result<(hdr::LinearBt2020Hdr, hdr::PeakClamp)> {
        if self.gamut != DestinationGamut::Bt2020 {
            return Err(NcError::Other(format!(
                "an HDR destination reached its encoder in {} rather than BT.2020",
                self.gamut.name()
            )));
        }
        hdr::from_new_chain(self.linear, self.tone_curve, self.gamut_mapping)
    }
}

/// Render one rendition through the chain for `d`, then `encode` it into its encoder's
/// input — the destination stage.
fn render_one(
    aces: AcesCgImage,
    film_base: AcesCgImage,
    recipe: &Recipe,
    d: Resolved,
    clock: &mut impl StageClock,
    encode: impl FnOnce(OneRendition) -> Result<DestinationPixels>,
) -> Result<DestinationRender> {
    let params = recipe.chain_params(d.range.peak()?, d.gamut.destination());
    let chain::Rendered {
        image,
        applied,
        scene_correction,
        look,
        fit_range,
    } = chain::render(aces, film_base, &params, clock)?;
    let (linear, gamut) = image.into_parts();
    let pixels = clock.time(StageKind::Destination, || {
        encode(OneRendition {
            linear,
            gamut,
            tone_curve: fit_range.applied(),
            gamut_mapping: applied[3].1,
        })
    })?;
    Ok(DestinationRender::Rendered {
        rendered: Box::new(ChainAccount {
            applied,
            scene_correction,
            look,
            fit_range,
        }),
        pixels,
    })
}

/// Render the gain-map destination: one graded image split into an SDR and an HDR
/// rendition (`chain::render_pair`), the HDR clamped to the destination's peak and
/// counted, the per-channel gains ratioed against the SDR base as the JPEG stores it,
/// and the map quantized at half resolution.
///
/// The report's chain account is the HDR rendition's — the destination's range — and
/// the SDR base's fit range rides in `gain_map.base_fit_range`. Each full-frame
/// buffer is dropped as soon as the next one is built from it (`RunProfile::GainMapJpeg`).
fn render_gain_map(
    aces: AcesCgImage,
    film_base: AcesCgImage,
    recipe: &Recipe,
    d: Resolved,
    clock: &mut impl StageClock,
) -> Result<DestinationRender> {
    let peak = d.range.peak()?;
    // Neither JPEG stores the IR plane (`--export-ir` reads the decoded image), so it is
    // dropped before the pair splits the graded image and copies it.
    let chain::RenderedPair { sdr, hdr } = chain::render_pair(
        aces.without_ir(),
        film_base,
        &recipe.shared_params(),
        d.gamut.destination(),
        peak,
        clock,
    )?;
    let base_fit_range = sdr.fit_range;
    let (sdr_linear, gamut) = sdr.image.into_parts();
    let (mut hdr_linear, _) = hdr.image.into_parts();
    let (base, icc, map, clamp, report) = clock.time(StageKind::Destination, || {
        let clamp = hdr::clamp_to_peak(&mut hdr_linear.rgb)?;
        let ratios = gain_ratio::between(&sdr_linear, &hdr_linear, gain_encode::OFFSET)?;
        drop(hdr_linear);
        let map = gain_encode::encode(&ratios)?;
        let report = GainMapResult {
            range: ratios.range(),
            width: map.width,
            height: map.height,
            base_fit_range,
        };
        drop(ratios);
        let (base, icc) = color::encode_display_linear(sdr_linear, gamut)?;
        Ok::<_, NcError>((base, icc, map, clamp, report))
    })?;
    Ok(DestinationRender::Rendered {
        rendered: Box::new(ChainAccount {
            applied: hdr.applied,
            scene_correction: hdr.scene_correction,
            look: hdr.look,
            fit_range: hdr.fit_range,
        }),
        pixels: DestinationPixels::GainMap {
            base,
            icc,
            map,
            headroom: peak.value(),
            clamp,
            report,
        },
    })
}

/// Encode a render into its container, filling the report block its encoder
/// owns. Consumes the render, so no encoder stages a second full-frame buffer.
fn encode_render(
    render: DestinationRender,
    output: &Path,
    report: &mut Report,
    log: &Log,
    warnings: &mut Vec<String>,
) -> Result<(staged::Staged, EncodeOutcome)> {
    let bigtiff_note = |big: bool, warnings: &mut Vec<String>| {
        if big {
            push_warning_buf(
                warnings,
                log,
                "output promoted to BigTIFF (would exceed the classic 4 GiB TIFF limit)".into(),
            );
        }
    };
    let pixels = match render {
        DestinationRender::FilmMaster { image, icc } => {
            let (staged, outcome, big) = encode::encode_f32(&image, &icc, output)?;
            bigtiff_note(big, warnings);
            return Ok((staged, outcome));
        }
        DestinationRender::Rendered { pixels, .. } => pixels,
    };
    Ok(match pixels {
        DestinationPixels::Sdr { image, icc } => {
            let (staged, outcome, big) = encode::encode_u16(&image, &icc, output)?;
            bigtiff_note(big, warnings);
            (staged, outcome)
        }
        DestinationPixels::HdrLinear(hdr, _) => {
            let icc = color::hdr_linear_bt2020_icc()?;
            let (staged, outcome, summary) = encode::encode_hdr_linear(hdr, &icc, output)?;
            report_hdr_linear_tiff(report, &summary, log, warnings);
            (staged, outcome)
        }
        DestinationPixels::HdrCoded(rendered, _) => {
            // Keyed off the transfer the render applied, so the profile and the codes
            // cannot disagree.
            let icc = match rendered.metadata().transfer {
                hdr::HdrTransfer::Pq => color::hdr_pq_tiff_icc()?,
                hdr::HdrTransfer::Hlg => color::hdr_hlg_tiff_icc()?,
            };
            let (staged, outcome, summary) = encode::encode_hdr_coded(rendered, &icc, output)?;
            report_hdr_coded_tiff(report, &summary, log, warnings);
            (staged, outcome)
        }
        DestinationPixels::HdrAvif(rendered, _) => {
            let (staged, outcome, summary) = avif::encode(rendered, output)?;
            report_avif(report, &summary, log, warnings);
            (staged, outcome)
        }
        DestinationPixels::GainMap {
            base,
            icc,
            map,
            headroom,
            ..
        } => iso_gain_map::encode(&base, &icc, &map, headroom, output)?,
    })
}

/// What [`convert_frame`] has resolved by the time the render takes over: everything
/// up to and including the film base.
struct DecodedFrame<'a> {
    /// What the decode and the chain read.
    recipe: &'a Recipe,
    /// The recipe's `output`, resolved once and checked before anything was decoded.
    destination: recipe::Destination,
    image: LinearImage,
    base: FilmBase,
    export_ir: Option<PathBuf>,
    output: &'a Path,
    report: Report,
    read_inputs: &'a [&'a Path],
}

/// The render, encode and commit for one frame: the fixed decode (`algo::fixed`) → NC
/// film RGB v1 → `pipeline::chain` → the destination the recipe's `output` resolves to
/// (`crate::destination`), or straight to the film master.
///
/// The optional IR export and the primary are staged, then committed together with the
/// primary last. No sidecar is written.
fn render_frame(
    frame: DecodedFrame<'_>,
    // The run's stage clock, holding the decode's and the film base's times.
    clock: &mut telemetry::StageTimer,
    log: &Log,
    warnings: &mut Vec<String>,
) -> Result<ConvertedFrame> {
    let DecodedFrame {
        recipe,
        destination,
        image,
        base,
        export_ir,
        output,
        mut report,
        read_inputs,
    } = frame;
    let decode_params = recipe.reconstruction;

    // Reconstruction: the fixed decode, then the pinned NC film RGB v1 3×3.
    let (aces, decoded) = clock.time(StageKind::Reconstruction, || {
        fixed::decode(&image, &base, &decode_params)
            .map(|(film, decoded)| (working_space::map_nc_film_rgb_v1(film), decoded))
    })?;

    // The NC film RGB v1 3×3 into ACEScg runs on this flow too, so the pinned
    // interpretation is a fact about the run.
    report.working_mapping = Some(working_space::WORKING_MAPPING_ID);
    // The resolved chemistry declaration, echoed so it survives into the report: the
    // report is the only artifact a convert run keeps beside its output.
    report.film_type =
        (recipe.input.film_type != FilmType::Unknown).then_some(recipe.input.film_type);

    // The chain, then the destination's transfer. Clear any stale lcms2 flag first so
    // only a fault from *this* transform is counted.
    let _ = cms_error_occurred();
    let render = render_destination(aces, &base, recipe, destination, clock)?;
    if cms_error_occurred() {
        return Err(NcError::Other(
            "color management (lcms2) reported a runtime error; see stderr".into(),
        ));
    }
    if let Some(content_light) = render.hdr_content_light()
        && let Some(message) = hdr::sdr_range_warning(content_light)
    {
        push_warning_buf(warnings, log, message);
    }
    let peak_clamp = render.peak_clamp();
    let second_rendition_samples = render.second_rendition_samples();
    let rendered = match &render {
        DestinationRender::Rendered { rendered, .. } => Some(rendered),
        DestinationRender::FilmMaster { .. } => None,
    };
    if let Some(message) = rendered.and_then(|r| r.fit_range.display_black.warning()) {
        push_warning_buf(warnings, log, message);
    }
    report.chain = Some(ChainResult {
        decode: decoded,
        stages: rendered.map_or_else(Vec::new, |r| {
            r.applied
                .map(|(stage, applied)| StageResult { stage, applied })
                .to_vec()
        }),
        rendering: recipe.rendering,
        roll: recipe.roll_report(rendered.is_some()),
        scene_correction: rendered.map(|r| r.scene_correction),
        look: rendered.map(|r| r.look),
        fit_range: rendered.map(|r| r.fit_range),
        destination: match destination {
            recipe::Destination::FilmMaster => OutputSection::FilmMaster,
            recipe::Destination::Display(d) => OutputSection::Display(d.axes()),
        },
        peak_clamp,
        gain_map: render.gain_map(),
        removed_sidecar: None,
    });

    // The IR export reads the *decoded* image and is staged before the primary, at
    // the destination's depth (f32 for a float TIFF, else u16).
    let mut pending: Vec<staged::Staged> = Vec::new();
    if let Some(path) = &export_ir {
        let depth = render.ir_depth();
        pending.push(clock.time(StageKind::IrExport, || {
            encode::export_ir(&image, depth, path)
        })?);
        report.ir_exported = Some(path.clone());
    }

    let (primary, mut outcome) = clock.time(StageKind::Encode, || {
        encode_render(render, output, &mut report, log, warnings)
    })?;
    if cms_error_occurred() {
        return Err(NcError::Other(
            "color management (lcms2) reported a runtime error; see stderr".into(),
        ));
    }
    // What the HDR hand-off clamped to the peak is lost range like the encoder's own
    // clip, so it is counted there: the warning, the report's `loss` and `--strict` all
    // see it.
    // The gain map's clamp counts its HDR rendition, a second set of samples beside the
    // SDR base the encoder counted, so the total grows with it: a fraction over one
    // rendition's samples could exceed 100 %.
    if let Some(clamp) = peak_clamp {
        outcome.loss.clipped_high += clamp.above_peak;
        outcome.loss.clipped_low += clamp.below_zero;
        outcome.loss.total_samples += second_rendition_samples;
    }
    report_encode_outcome(&mut report, &outcome, log, warnings);

    // The primary goes last: its presence is what reads as success.
    pending.push(primary);
    for note in staged::commit_all(std::mem::take(&mut pending))? {
        push_warning_buf(warnings, log, note);
    }
    if let Some(path) = &export_ir {
        log.info(format_args!("wrote IR plane {}", path.display()));
    }
    log.info(format_args!("wrote {}", output.display()));

    // A sidecar an earlier run wrote beside this path now describes an image that no
    // longer exists — reloading it would reproduce a different picture. Removed only
    // after the new image is committed, and only when it is recognisably one of nc's
    // sidecars, so a user's own file that happens to share the name is left alone.
    let stale = encode::sidecar_path(output);
    let stale_key = collision_key(&stale);
    let is_read_input = read_inputs
        .iter()
        .any(|input| keys_collide(&collision_key(input), &stale_key));
    if is_read_input && is_nc_sidecar(&stale) {
        // This run's own recipe (or manifest) sits where the old sidecar would: it is
        // an input, not a stale artifact, so it stays — loudly, since it still pairs
        // by name with an image it no longer describes.
        push_warning_buf(
            warnings,
            log,
            format!(
                "{} pairs by name with this output but describes the image an earlier run \
                 wrote there; it was left in place because this run read it (--params / \
                 --frames)",
                stale.display()
            ),
        );
    } else if is_nc_sidecar(&stale) {
        // A warning, not an error: the new image is already committed, and failing
        // the run here would discard the report that describes it.
        match std::fs::remove_file(&stale) {
            Ok(()) => {
                log.info(format_args!("removed stale sidecar {}", stale.display()));
                if let Some(chain) = report.chain.as_mut() {
                    chain.removed_sidecar = Some(stale);
                }
            }
            Err(e) => push_warning_buf(
                warnings,
                log,
                format!(
                    "could not remove the stale sidecar {} (it describes the image this \
                     run replaced): {e}",
                    stale.display()
                ),
            ),
        }
    }

    report.warnings = std::mem::take(warnings);
    Ok(ConvertedFrame {
        report,
        loss: outcome.loss,
    })
}

/// `hanten convert` — the full pipeline: decode → film-base → fixed decode → the
/// chain → encode (+ optional IR export). Warnings are
/// collected into the report and echoed to stderr; `--strict` promotes any of
/// them to a non-zero exit.
///
/// Telemetry (opt-in) is emitted here, once the run's outcome is fixed: a success
/// event, or a failure event carrying what [`ConvertAttempt`] had learned.
fn run_convert(args: ConvertArgs) -> Result<()> {
    let started = Instant::now();
    let log = Log::new(&args.report);
    // Read once, so the guarded and the written log path are the same.
    let telemetry_log = if args.telemetry {
        telemetry::default_log_path()
    } else {
        None
    };
    let mut attempt = ConvertAttempt::default();
    let result = convert_attempt(&args, &log, started, telemetry_log.as_deref(), &mut attempt);
    if telemetry_requested(&args) {
        emit_telemetry(
            &args,
            &log,
            started,
            &attempt,
            result.as_ref().err(),
            telemetry_log.as_deref(),
        );
    }
    result
}

/// Where a `convert` is, for a failure event's `stage`.
#[derive(Clone, Copy, Debug, Default)]
enum ConvertPhase {
    /// Before the frame: recipe, validation, output path, the write-target guard.
    #[default]
    Setup,
    /// Inside [`convert_frame`]; its stage clock says where.
    Frame,
    /// After the frame: the report and the `--strict` gate.
    Finalize(FinishedFrame),
}

/// What a frame that succeeded hands the rest of the run's telemetry.
#[derive(Clone, Copy, Debug)]
struct FinishedFrame {
    loss: EncodeReport,
    /// The report's warning count.
    warnings: usize,
    /// The run's time up to the report.
    total_ms: f64,
}

/// What a `convert` learned as it ran, filled in as each fact becomes known, so a
/// failure event carries exactly what the run reached. Its `warnings` are the run's
/// own buffer; the rest is read only by telemetry.
#[derive(Default)]
struct ConvertAttempt {
    phase: ConvertPhase,
    frame: FrameFacts,
    /// Accumulated as they are raised; moved into the report on success.
    warnings: Vec<String>,
    /// The resolved output path and the recipe's `--export-ir` path, once known — the
    /// paths a failure event's sinks must not land on.
    output: Option<PathBuf>,
    export_ir: Option<PathBuf>,
    /// Set once the destination resolved.
    conversion: Option<telemetry::ConversionInfo>,
    /// The write-target guard passed, telemetry's sinks included.
    guarded: bool,
    /// The run failed on `--strict`, not on an error of its own.
    strict: bool,
}

impl ConvertAttempt {
    /// The stage a failure happened in.
    fn failed_stage(&self) -> telemetry::EventStage {
        match self.phase {
            ConvertPhase::Setup => telemetry::EventStage::Setup,
            ConvertPhase::Frame => self.frame.clock.failed_stage(),
            ConvertPhase::Finalize(_) => telemetry::EventStage::Finalize,
        }
    }

    fn finished(&self) -> Option<FinishedFrame> {
        match self.phase {
            ConvertPhase::Finalize(finished) => Some(finished),
            _ => None,
        }
    }
}

/// The body of [`run_convert`], recording what it learns into `attempt`.
fn convert_attempt(
    args: &ConvertArgs,
    log: &Log,
    started: Instant,
    telemetry_log: Option<&Path>,
    attempt: &mut ConvertAttempt,
) -> Result<()> {
    reject_deprecated_input_flags(&args.input_opts)?;
    // Removed flags run first, so a retired spelling is diagnosed as retired before any
    // rule reasons about the values the recipe resolves.
    reject_removed_flags(args)?;
    // A recipe written for the removed chain is refused inside the load, by name.
    let loaded = load_recipe(args.recipe_in.as_deref())?;
    // The flags win over the recipe.
    let recipe = recipe::merge(loaded.recipe, args);
    attempt.export_ir = recipe.input.export_ir.as_deref().map(PathBuf::from);
    // A flag-presence rule, ahead of every value rule that could refuse first.
    reject_roll_flags_nothing_applies(args, &recipe)?;
    validate_convert(&recipe, args)?;

    // The path nc actually writes: `-o out` under the default becomes `out.tiff`.
    // Resolved **here**, before anything derives from it — the write-target guard, the
    // report's `output` and telemetry's `output_bytes` must all see the completed path,
    // never the stem. (`validate_convert` ran the same rule and discarded the value;
    // this is the one call that keeps it.)
    let target =
        OutputTarget::resolve(&recipe, KnobNames::FlagAndKey, args.destination.film_master)?;
    let output = resolve_output_path(&args.output, target, SuffixContext::Convert)?;
    attempt.output = Some(output.clone());
    attempt.conversion = Some(conversion_info(&recipe, target.destination));
    if output != args.output {
        log.info(format!(
            "output path completed from the destination {}: writing {}",
            recipe::destination_label(target.destination, KnobNames::FlagAndKey),
            output.display()
        ));
    }

    // Guard every write target against the input and against each other before
    // anything is decoded or written — telemetry's sinks included, so an odd
    // `NC_TELEMETRY_LOG` or `--telemetry-file` can't append into (and corrupt) the
    // input scan or an artifact. A collision is a config error, distinct from a
    // telemetry *write* failure, which is fail-soft.
    ensure_write_targets_distinct(
        &args.input,
        &write_targets(args, &output, attempt.export_ir.as_deref(), telemetry_log),
    )?;
    if let Some(msg) =
        telemetry_sink_collision(args, &output, attempt.export_ir.as_deref(), telemetry_log)
    {
        return Err(NcError::Usage(msg));
    }
    attempt.guarded = true;

    // The resolved recipe, which reloads through `--params` to the same run.
    if let Some(path) = &args.dump_params {
        write_json(path, &recipe, log)?;
    }
    // `--seed` is reserved (no stochastic step in Step 1) but accepted so the
    // documented flag isn't rejected; nothing consumes it yet.
    let _ = args.seed;

    // Replaying a sidecar captured under a *different* behavioral `pipeline_version`
    // still applies its parameters, but the default render has changed underneath
    // them — so the pixels won't match the original. Loud and `--strict`-promotable
    // rather than a silently-different image: exposing exactly that mismatch is why
    // `pipeline_version` exists. Pushed before the conversion so it is on stderr before
    // any work happens; note that on a *failed* frame no report is emitted, so stderr
    // is the only place it appears there. (`roll` differs: it records per-frame
    // failures and still emits its report, so the roll-level warning survives a bad
    // frame.)
    if let Some(msg) = pipeline_version_warning(loaded.meta_pipeline_version) {
        push_warning_buf(&mut attempt.warnings, log, msg);
    }
    // What the run's recipe falls back on, or states that nobody may have chosen — a fact
    // about the run's recipe, not the frame; a typed flag never warns.
    for msg in recipe.recipe_warnings(recipe::TypedStyle::of(args)) {
        push_warning_buf(&mut attempt.warnings, log, msg);
    }

    // The per-frame pipeline core (decode → film-base → render → encode), shared
    // byte-for-byte with `roll`. Operational concerns the two orchestrators layer
    // differently — report emission, `--strict` gating, telemetry — stay out here.
    attempt.phase = ConvertPhase::Frame;
    let frame = convert_frame(
        "convert",
        &args.input,
        &output,
        &recipe,
        InputFromCli {
            transfer: args.input_opts.input_transfer.is_some(),
            meaning: args.input_opts.input_meaning.is_some(),
        },
        &args
            .recipe_in
            .iter()
            .map(PathBuf::as_path)
            .collect::<Vec<_>>(),
        args.memory.budget(),
        &mut attempt.frame,
        log,
        &mut attempt.warnings,
    );

    // A failure here drops the report, and with it every warning accumulated before
    // the failure point — including the memory preflight's RAM-pressure note, whose
    // whole point is to explain a run the OS may kill. `log.warn` already echoed them,
    // but `--quiet` suppresses that, so under `--quiet` they would be lost on *both*
    // channels. Re-emit unconditionally (the `warn_always` treatment clipping already
    // gets) before propagating. `roll` honours this via `frame_report_err`.
    let ConvertedFrame { mut report, loss } = match frame {
        Ok(frame) => frame,
        Err(e) => {
            // Only the warnings `--quiet` swallowed: `push_warning_buf` already echoed
            // each one through `log.warn` as it was raised, so re-emitting
            // unconditionally would double-print them on a normal run.
            if log.quiet {
                for w in &attempt.warnings {
                    log.warn_always(w);
                }
            }
            return Err(e);
        }
    };
    let total_ms = elapsed_ms(started);
    report.elapsed_ms = Some(total_ms);
    report.recipe = Some(recipe);
    attempt.phase = ConvertPhase::Finalize(FinishedFrame {
        loss,
        warnings: report.warnings.len(),
        total_ms,
    });

    // Emit the report before the `--strict` gate so the machine-readable record lands
    // even when a warning then fails the run. (A hard I/O error above returns earlier —
    // its exit code and stderr message are the signal there.)
    emit_report(
        &report,
        args.report.report,
        args.report.report_file.as_deref(),
        log,
    )?;

    // `--strict` promotes any present warning to a non-zero exit — a failure event of
    // its own kind, `strict`, not a successful conversion.
    if args.strict && !report.warnings.is_empty() {
        attempt.strict = true;
        return Err(NcError::Other(format!(
            "--strict: {} warning(s) present (see report)",
            report.warnings.len()
        )));
    }
    Ok(())
}

/// Every path a `convert` writes, labelled for [`ensure_write_targets_distinct`]:
/// the output, `--dump-params`, `--report-file`, `--export-ir`, and telemetry's
/// sinks (`--telemetry-file` unless it is `-`, and the resolved log).
fn write_targets<'a>(
    args: &'a ConvertArgs,
    output: &'a Path,
    export_ir: Option<&'a Path>,
    telemetry_log: Option<&'a Path>,
) -> Vec<(&'static str, &'a Path)> {
    let mut targets: Vec<(&str, &Path)> = vec![("--output", output)];
    if let Some(p) = &args.dump_params {
        targets.push(("--dump-params", p));
    }
    if let Some(p) = args.report.report_file.as_deref() {
        targets.push(("--report-file", p));
    }
    if let Some(p) = export_ir {
        targets.push(("--export-ir", p));
    }
    if let Some(p) = telemetry_file_target(args) {
        targets.push(("--telemetry-file", p));
    }
    if let Some(p) = telemetry_log {
        targets.push(("the telemetry log", p));
    }
    targets
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Roll (batch) — plan → recipe → apply, the batch-apply scaffold
// ---------------------------------------------------------------------------

/// A `--frames` manifest: an explicit list of frames to convert, each optionally
/// carrying its own output path and a partial-recipe override. `deny_unknown_fields`
/// so a typo'd top-level key is a loud error, not a silently-ignored frame list.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RollManifest {
    frames: Vec<ManifestFrame>,
}

/// One frame in a `--frames` manifest. `params` is a *partial* recipe (any subset of
/// the [`Recipe`] shape) deep-merged onto the shared recipe for this frame only — the
/// frame-local override mechanism. `deny_unknown_fields` guards the entry keys; the
/// merged `params` are validated when deserialized back to a `Recipe`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFrame {
    input: PathBuf,
    #[serde(default)]
    output: Option<PathBuf>,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

/// A frame resolved for conversion: where to read and write, and the frame's own
/// recipe (the shared one with any per-frame manifest override merged on top), which
/// the decode and the chain read.
#[derive(Debug)]
struct PlannedFrame {
    input: PathBuf,
    output: PathBuf,
    recipe: Recipe,
    /// The per-frame override applied (manifest `params`), echoed into the roll
    /// report so a reader sees exactly what differed for this frame; `None` when
    /// the frame ran the shared recipe unchanged.
    overrides: Option<serde_json::Value>,
}

/// The roll-level JSON report emitted on stdout (or `--report-file`): any roll-level
/// warnings, the per-frame status list, and a summary. Each frame reports the
/// *resolved* base it used (a redundant echo when the recipe pins an explicit base,
/// meaningful under an `auto`/`region` base that resolves per frame).
#[derive(Debug, Serialize)]
struct RollReport {
    command: &'static str,
    /// What produced this batch: build identity + the behavioral `pipeline_version`.
    /// No `params_hash`: each frame's own identity hashes the recipe that frame ran
    /// (the shared recipe with its overrides). Operational provenance only — no CLI
    /// flag, no recipe key, no effect on a single output pixel.
    identity: Identity,
    /// Roll-level warnings not tied to a single frame (e.g. the film base is not
    /// frozen because the shared recipe's `calibration.film_base` is not `explicit`).
    /// Echoed to stderr and, like per-frame warnings, promoted to a failing exit
    /// by `--strict`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
    frames: Vec<FrameReport>,
    summary: RollSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    elapsed_ms: Option<f64>,
}

/// Per-frame entry inside a [`RollReport`]. The per-frame *identity*
/// (`input`/`output`/`warnings`/`overrides`) lives here; the ok-vs-failed
/// *payload* is the data-carrying [`FrameStatus`] enum, so an "ok" frame can't
/// carry an `error` and a "failed" frame can't carry a film base — states the old
/// `status: &str` + all-`Option` layout could encode. `warnings` is common to
/// both outcomes: a frame that warns and *then* fails still reports its warnings
/// (they are echoed to stderr as they occur and preserved here regardless).
#[derive(Debug, Serialize)]
struct FrameReport {
    input: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<PathBuf>,
    /// The outcome payload, flattened so its `status` discriminator and fields
    /// serialize as flat sibling keys (`"status":"ok"`, `film_base`, … / `error`).
    #[serde(flatten)]
    status: FrameStatus,
    /// What the memory preflight decided for *this* frame — mirrors the
    /// single-frame `Report` field. Per-frame rather than roll-level because
    /// frames may differ in dimensions (and so in estimated peak) even though
    /// they share one budget; the gate runs per frame too.
    ///
    /// Common to both outcomes, like `warnings`: the gate runs before anything
    /// else, so a frame that passed it and then failed still has a decision to
    /// report — and that is precisely the frame whose estimate a reader wants.
    #[serde(skip_serializing_if = "Option::is_none")]
    memory: Option<MemoryReport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
    /// The per-frame recipe override applied (manifest `params`), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    overrides: Option<serde_json::Value>,
}

/// The ok-vs-failed payload of a [`FrameReport`], each variant carrying only the
/// fields legal for that outcome. Internally tagged (`#[serde(tag = "status")]`)
/// and flattened into `FrameReport`, so it serializes the flat
/// `"status":"ok"`/`"failed"` discriminator with the payload as sibling keys —
/// the same wire shape the old `status: &str` + `error`/payload `Option`s
/// produced, minus the illegal combinations.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum FrameStatus {
    /// A converted frame: the resolved values it used (mirrors the relevant
    /// single-frame [`Report`] fields). Each is `None`/omitted when the settings
    /// didn't produce it.
    Ok {
        #[serde(skip_serializing_if = "Option::is_none")]
        film_base: Option<FilmBase>,
        /// The resolved effective measurement area — mirrors the single-frame
        /// `Report` field. Per-frame rather than roll-level: one shared
        /// `measure.inset` meets a holder depth that is genuinely this frame's
        /// own, and a per-frame `params` override can change the inset too. It is
        /// also what makes `measure.inset` observable on `roll` at all, the same
        /// reason `convert` reports it unconditionally.
        ///
        /// Boxed for the same reason as `input_color` below — between them they
        /// are what would otherwise make `Ok` dwarf `Failed`
        /// (`clippy::large_enum_variant`); `Box` serializes transparently.
        #[serde(skip_serializing_if = "Option::is_none")]
        effective_area: Option<Box<film_base::EffectiveArea>>,
        /// Resolved input color semantics (transfer + meaning + evidence + ICC
        /// summary) the frame ran on — mirrors the single-frame `Report` field so a
        /// roll frame reports the same input semantics `convert` does. Boxed like
        /// `effective_area` above: these are the two large fields, and unboxed they
        /// make `Ok` dwarf `Failed` (`clippy::large_enum_variant`); `Box`
        /// serializes transparently.
        #[serde(skip_serializing_if = "Option::is_none")]
        input_color: Option<Box<InputColorReport>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        loss: Option<EncodeReport>,
        /// Per-channel mean of the samples as written — mirrors the single-frame
        /// `Report` field. Without it a roll's frames carry no comparison basis, so
        /// `nctool compare` (and the docs' claim that a roll is comparable) would
        /// have nothing to diff frame-to-frame.
        #[serde(skip_serializing_if = "Option::is_none")]
        output_stats: Option<OutputStats>,
        /// The frame's declared film chemistry — mirrors the single-frame `Report`
        /// field (omitted when `unknown`).
        #[serde(skip_serializing_if = "Option::is_none")]
        film_type: Option<FilmType>,
        /// This frame's own identity, beside the roll's, with the hash of the recipe
        /// the frame ran. Boxed with `chain` below (`clippy::large_enum_variant`).
        #[serde(skip_serializing_if = "Option::is_none")]
        identity: Option<Box<Identity>>,
        /// What the frame ran — mirrors the single-frame `Report` field.
        /// Boxed like `effective_area` (`clippy::large_enum_variant`).
        #[serde(skip_serializing_if = "Option::is_none")]
        chain: Option<Box<ChainResult>>,
        /// What the encoder wrote, for an HDR destination — mirrors the single-frame
        /// `Report` fields. Boxed (`clippy::large_enum_variant`).
        #[serde(skip_serializing_if = "Option::is_none")]
        avif: Option<Box<AvifResult>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        hdr_linear_tiff: Option<Box<HdrLinearTiffResult>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        hdr_coded_tiff: Option<Box<HdrCodedTiffResult>>,
    },
    /// A frame that failed to convert: the failure message. The roll records it
    /// and continues (the loud non-zero exit is the batch-level signal).
    Failed { error: String },
}

/// Roll totals — a quick machine-readable tally alongside the per-frame list.
#[derive(Debug, Serialize)]
struct RollSummary {
    total: usize,
    succeeded: usize,
    failed: usize,
}

/// Whether a path has a `.tif`/`.tiff` extension (case-insensitive) — the filter
/// for expanding a directory argument into frames.
fn has_tiff_ext(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("tif") || e.eq_ignore_ascii_case("tiff"))
        .unwrap_or(false)
}

/// Expand one positional input into frame paths: a directory yields its
/// `.tif`/`.tiff` files (sorted for determinism); anything else passes through
/// verbatim (a missing file surfaces later as a per-frame decode error).
fn expand_input(path: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_dir() {
        let read_dir = std::fs::read_dir(path).map_err(|e| {
            NcError::Usage(format!(
                "cannot read input directory {}: {e}",
                path.display()
            ))
        })?;
        // Propagate a per-entry read error rather than dropping it: a silently
        // skipped entry would shorten the batch without a word (fail-loud
        // violation). Same usage-error class (exit 2) as failing to open the dir.
        let mut entries: Vec<PathBuf> = Vec::new();
        for entry in read_dir {
            let entry = entry.map_err(|e| {
                NcError::Usage(format!(
                    "cannot read an entry in input directory {}: {e}",
                    path.display()
                ))
            })?;
            let p = entry.path();
            if p.is_file() && has_tiff_ext(&p) {
                entries.push(p);
            }
        }
        entries.sort();
        out.extend(entries);
    } else {
        out.push(path.to_path_buf());
    }
    Ok(())
}

/// Default per-frame output name in the out-dir: `<input-stem>_positive.<ext>`, where
/// the suffix comes from the **frame's own resolved destination** rather than a
/// hardcoded `tiff` — a manifest's per-frame `params` override may change `output`, and
/// the name has to describe the bytes actually written.
fn default_output_name(input: &Path, out_dir: &Path, target: OutputTarget) -> PathBuf {
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "frame".to_string());
    out_dir.join(format!(
        "{stem}_positive.{}",
        target.container().canonical()
    ))
}

/// Resolve a frame's output path: a manifest's explicit path (absolute used
/// verbatim, relative joined onto the out-dir) or the derived
/// `<stem>_positive.<ext>`.
///
/// An explicit manifest path goes through the **same** rule `convert` uses, so it
/// is judged when it states a suffix and *completed* when it does not. A derived name
/// is correct by construction and is not re-checked.
fn resolve_frame_output(
    explicit: Option<&Path>,
    input: &Path,
    out_dir: &Path,
    target: OutputTarget,
) -> Result<PathBuf> {
    let path = match explicit {
        Some(o) if o.is_absolute() => o.to_path_buf(),
        Some(o) => out_dir.join(o),
        None => return Ok(default_output_name(input, out_dir, target)),
    };
    resolve_output_path(&path, target, SuffixContext::RollFrame(input))
}

/// Deep-merge `overlay` into `base`: JSON objects merge key-by-key (recursively),
/// any other value replaces. Layers a per-frame partial-recipe override onto the
/// shared recipe's JSON before it is deserialized back to a validated [`Recipe`]
/// — a partial override keeps the shared values it doesn't
/// mention (a plain `serde` deserialize of the partial would reset them to
/// defaults instead).
///
/// Switching a multi-variant enum via an override is safe, not silent: the merged
/// value must still deserialize as that enum. An **externally tagged** one (e.g.
/// [`FilmBaseSource`]) is a one-key map (`{"region":[…]}`), and flipping it to another
/// variant (`{"explicit":[…]}`) must *replace* the whole map — a key-by-key merge would
/// union the tags into `{"region":…, "explicit":…}`, which no externally-tagged enum can
/// deserialize, turning an override that should apply into a confusing `from_value`
/// rejection. [`is_variant_switch`] catches exactly that signature (both sides
/// single-key objects with *different* keys). No recipe section is internally tagged
/// any more: the `reconstruction` selector went with `simple` and the curve's with
/// `characteristic`, so an overlay's retired `type` merges in as a key and the
/// deserializer strips or refuses it.
///
/// A malformed override is still rejected loudly by the `from_value` in
/// [`resolve_frames`], never applied half-merged.
fn merge_json(base: &mut serde_json::Value, overlay: &serde_json::Value) {
    if is_variant_switch(base, overlay) {
        *base = overlay.clone();
        return;
    }
    match (base, overlay) {
        (serde_json::Value::Object(b), serde_json::Value::Object(o)) => {
            for (k, v) in o {
                merge_json(b.entry(k.clone()).or_insert(serde_json::Value::Null), v);
            }
        }
        (b, o) => *b = o.clone(),
    }
}

/// The externally-tagged-enum-variant-switch signature: `base` and `overlay` are
/// both single-key objects with *different* keys (e.g. `{"region":[…]}` vs
/// `{"explicit":[…]}`). Deep-merging such a pair would leave a two-tag object that
/// no externally-tagged enum deserializes, so [`merge_json`] replaces it wholesale
/// instead. A unit variant serializes as a bare string (`"auto"`), not an object,
/// so switching to/from it never reaches here — the plain replace arm handles it.
///
/// **Not a struct of optional fields that happens to state one key.** The recipe's
/// `output.display` serializes only its stated axes, so a shared `{"transfer": "pq"}`
/// and a per-frame `{"container": "avif"}` have the same shape as a variant switch;
/// replacing would drop the roll's transfer. Two keys that are both destination axes
/// ([`crate::destination::AXIS_KEYS`]) are therefore merged field by field. No
/// externally tagged enum in either recipe has a variant of those names.
fn is_variant_switch(base: &serde_json::Value, overlay: &serde_json::Value) -> bool {
    let axis =
        |k: Option<&String>| k.is_some_and(|k| crate::destination::AXIS_KEYS.contains(&k.as_str()));
    match (base, overlay) {
        (serde_json::Value::Object(b), serde_json::Value::Object(o)) => {
            b.len() == 1
                && o.len() == 1
                && b.keys().next() != o.keys().next()
                && !(axis(b.keys().next()) && axis(o.keys().next()))
        }
        _ => false,
    }
}

/// Load a `--frames` manifest. A read failure or invalid/unknown-key JSON is a
/// usage error (a config mistake), like [`load_recipe`].
fn load_manifest(path: &Path) -> Result<RollManifest> {
    let txt = std::fs::read_to_string(path).map_err(|e| {
        NcError::Usage(format!(
            "cannot read --frames manifest {}: {e}",
            path.display()
        ))
    })?;
    serde_json::from_str(&txt)
        .map_err(|e| NcError::Usage(format!("invalid --frames manifest {}: {e}", path.display())))
}

/// Roll mode writes one output per frame into a shared directory, so a single
/// `input.export_ir` path — which every frame would overwrite — is nonsensical.
/// Reject it loudly rather than silently clobbering one IR file N times.
fn reject_roll_unsupported(r: &Recipe) -> Result<()> {
    // Do not reintroduce a destination list here: every destination is roll-capable,
    // because a derived name takes its suffix from the frame's own destination and an
    // explicit manifest path goes through `convert`'s `resolve_output_path` rule.
    if r.input.export_ir.is_some() {
        return Err(NcError::Usage(
            "input.export_ir (--export-ir) is not supported in roll mode: it names a \
             single path that every frame would overwrite; export the IR plane per \
             frame with `hanten convert` instead"
                .into(),
        ));
    }
    Ok(())
}

/// Roll pre-flight: reject an input assertion that can never yield a convertible
/// frame **before** decoding the first (100+ MB) scan (the per-file gate lives inside
/// `convert_frame`, after the decode).
///
/// Only `input.meaning = colorimetric` is unconditionally unsupported regardless
/// of the file (colorimetric/encoded negatives have no inverse-transfer /
/// reconstruction path, so `require_convertible` rejects them for every frame).
/// The other axes' convertibility depends on per-file structural evidence
/// (unknown until decode), so they stay gated per frame. Applied to both the
/// shared recipe and each resolved per-frame override.
fn reject_roll_unsupported_input(r: &Recipe) -> Result<()> {
    if r.input.meaning == MeaningAssertion::Colorimetric {
        return Err(NcError::Unsupported(
            "input.meaning = colorimetric is unsupported for every frame: colorimetric / \
             encoded negatives have no inverse-transfer/reconstruction path yet. Remove it \
             or assert a scanner-device meaning."
                .into(),
        ));
    }
    Ok(())
}

/// `roll`'s whole gate for one recipe — the shared one, or a frame's after its override
/// merged: the roll-specific rejections first, then the stage sections' rules, then the
/// shared sections, whose missing-base rule is the least specific diagnosis there is.
/// `context` prefixes a stage rule's message, which names recipe keys only (`roll`
/// accepts no conversion flags).
fn validate_roll_recipe(r: &Recipe, context: &str) -> Result<()> {
    reject_roll_unsupported(r)?;
    reject_roll_unsupported_input(r)?;
    recipe::validate(r, KnobNames::KeyOnly)
        .map_err(|e| NcError::Usage(format!("{context}: {}", e.message())))?;
    validate_shared(r, FilmBaseRemedy::SharedRecipe)
}

/// A value every frame of a roll shares ([`ROLL_WIDE`]).
struct RollWide {
    key: &'static str,
    /// What differs about the frame when the value does.
    breaks: &'static str,
    compare: fn(frame: &Recipe, roll: &Recipe) -> Result<Option<Difference>>,
}

/// The frame's value and the roll's, as a roll warning prints them.
type Difference = (String, String);

/// A recipe value as a roll warning prints it.
fn roll_value_text<T: Serialize>(v: &T) -> Result<String> {
    // To text directly: through a `Value` an f32 widens and prints its f64 digits.
    match serde_json::to_string(v) {
        Ok(s) if s == "null" => Ok("unset".into()),
        Ok(s) => Ok(s),
        Err(e) => Err(NcError::Other(format!("serializing a recipe value: {e}"))),
    }
}

/// Both values as text when they differ, compared as values (`-0.0` is `0.0`).
fn changed<T: PartialEq + Serialize>(frame: &T, roll: &T) -> Result<Option<Difference>> {
    if frame == roll {
        return Ok(None);
    }
    Ok(Some((roll_value_text(frame)?, roll_value_text(roll)?)))
}

/// A resolved destination as a roll warning prints it.
fn destination_text(d: recipe::Destination) -> Result<String> {
    match d {
        recipe::Destination::FilmMaster => roll_value_text(&OutputSection::FilmMaster),
        recipe::Destination::Display(d) => roll_value_text(&d.axes()),
    }
}

const DECODED_APART: &str = "this frame's densities decode differently from the rest of the roll's";

/// The roll-wide values. A frame's `params` override that resolves one differently from
/// the shared recipe is applied and warned about (`--strict` refuses it). Every other key
/// is frame-local: `roll.white_stops` is how `measure-roll`'s `reuse.frames` states a
/// clamped frame's white, and `input` describes each file, not the roll.
/// `roll_classifies_every_recipe_key` holds the split.
const ROLL_WIDE: &[RollWide] = &[
    RollWide {
        key: "calibration.film_base",
        breaks: "this frame's Dmin differs from the rest of the roll's, breaking colour \
                 consistency",
        compare: |f, r| changed(&f.calibration.film_base, &r.calibration.film_base),
    },
    RollWide {
        key: "roll.white_balance",
        breaks: "this frame is balanced with other gains than the roll's",
        // Only gains that reach both: a frame that leaves them out entirely is the
        // `rendering` or `output` row's.
        compare: |f, r| {
            if !(f.applies_roll_white_balance() && r.applies_roll_white_balance()) {
                return Ok(None);
            }
            changed(&f.roll.white_balance, &r.roll.white_balance)
        },
    },
    RollWide {
        key: "reconstruction.scale",
        breaks: DECODED_APART,
        compare: |f, r| changed(&f.reconstruction.scale, &r.reconstruction.scale),
    },
    RollWide {
        key: "reconstruction.offset",
        breaks: DECODED_APART,
        compare: |f, r| changed(&f.reconstruction.offset, &r.reconstruction.offset),
    },
    RollWide {
        key: "reconstruction.linearization",
        breaks: DECODED_APART,
        compare: |f, r| {
            changed(
                &f.reconstruction.linearization,
                &r.reconstruction.linearization,
            )
        },
    },
    RollWide {
        key: "reconstruction.anchor",
        breaks: DECODED_APART,
        compare: |f, r| changed(&f.reconstruction.anchor, &r.reconstruction.anchor),
    },
    RollWide {
        key: "rendering",
        breaks: "the rendering decides whether the roll's measurement applies at all, and \
                 derives an unset destination",
        // Names the destination too when the rendering moved it: the `output` row is
        // silent then.
        compare: |f, r| {
            let Some((own, roll)) = changed(&f.rendering, &r.rendering)? else {
                return Ok(None);
            };
            let (df, dr) = (
                recipe::destination(f, KnobNames::KeyOnly)?,
                recipe::destination(r, KnobNames::KeyOnly)?,
            );
            if df == dr {
                return Ok(Some((own, roll)));
            }
            Ok(Some((
                format!("{own} (destination {})", destination_text(df)?),
                format!("{roll} (destination {})", destination_text(dr)?),
            )))
        },
    },
    RollWide {
        key: "output",
        breaks: "this frame is a different image from the rest of the roll's",
        // Only when the frame states another `output`: a destination derived from a
        // `rendering` change is that row's.
        compare: |f, r| {
            if f.output == r.output {
                return Ok(None);
            }
            let (df, dr) = (
                recipe::destination(f, KnobNames::KeyOnly)?,
                recipe::destination(r, KnobNames::KeyOnly)?,
            );
            if df == dr {
                return Ok(None);
            }
            Ok(Some((destination_text(df)?, destination_text(dr)?)))
        },
    },
];

/// One warning per [`ROLL_WIDE`] value `frame` resolves differently from `shared`.
fn roll_wide_breaks(input: &Path, frame: &Recipe, shared: &Recipe) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    for w in ROLL_WIDE {
        if let Some((own, roll)) = (w.compare)(frame, shared)? {
            warnings.push(format!(
                "frame {}: its `params` override resolves `{}` to {own}, where the roll's is \
                 {roll} — {}. Drop what changes it from this frame's `params` to keep the \
                 roll consistent.",
                input.display(),
                w.key,
                w.breaks,
            ));
        }
    }
    Ok(warnings)
}

/// Build the per-frame plan from the `--frames` manifest or the positional inputs,
/// resolving each frame's recipe (shared recipe + any per-frame override) and output
/// path. Config errors (a bad override, an unsupported knob) fail loudly here, before
/// any frame is converted; runtime errors (a bad decode, a degenerate base) surface
/// per frame during conversion. A per-frame override that changes a [`ROLL_WIDE`] value
/// is not rejected — it is applied, with a roll-level warning pushed to `roll_warnings`,
/// so a deliberate per-frame value stays possible while the break is surfaced and
/// `--strict`-promotable.
fn resolve_frames(
    args: &RollArgs,
    shared: &Recipe,
    roll_warnings: &mut Vec<String>,
    log: &Log,
) -> Result<Vec<PlannedFrame>> {
    let out_dir = args.out_dir.as_path();
    let mut planned = Vec::new();
    match &args.frames {
        Some(manifest_path) => {
            let manifest = load_manifest(manifest_path)?;
            if manifest.frames.is_empty() {
                return Err(NcError::Usage(format!(
                    "--frames manifest {} lists no frames",
                    manifest_path.display()
                )));
            }
            // The shared recipe as JSON, so a per-frame partial override can be
            // deep-merged onto it and deserialized back with `deny_unknown_fields`.
            // Serialized once, cloned per frame.
            let shared_value = serde_json::to_value(shared)
                .map_err(|e| NcError::Other(format!("serializing shared recipe: {e}")))?;
            for mf in manifest.frames {
                let (recipe, overrides) = match mf.params {
                    Some(ov) => {
                        // A per-frame override carrying a removed key gets the same
                        // migration guidance as the shared recipe, not an opaque
                        // `deny_unknown_fields` serde error.
                        let context =
                            format!("frame {}: per-frame `params` override", mf.input.display());
                        recipe::check_body(&ov, false, &context)?;
                        let mut v = shared_value.clone();
                        merge_json(&mut v, &ov);
                        let r: Recipe = serde_json::from_value(v).map_err(|e| {
                            NcError::Usage(format!(
                                "frame {}: invalid params override: {e}",
                                mf.input.display()
                            ))
                        })?;
                        validate_roll_recipe(&r, &context)?;
                        for msg in roll_wide_breaks(&mf.input, &r, shared)? {
                            log.warn(&msg);
                            roll_warnings.push(msg);
                        }
                        (r, Some(ov))
                    }
                    None => (shared.clone(), None),
                };
                let output = resolve_frame_output(
                    mf.output.as_deref(),
                    &mf.input,
                    out_dir,
                    OutputTarget::resolve(&recipe, KnobNames::KeyOnly, false)?,
                )?;
                planned.push(PlannedFrame {
                    input: mf.input,
                    output,
                    recipe,
                    overrides,
                });
            }
        }
        None => {
            let mut inputs = Vec::new();
            for p in &args.inputs {
                expand_input(p, &mut inputs)?;
            }
            inputs.sort();
            inputs.dedup();
            if inputs.is_empty() {
                return Err(NcError::Usage(
                    "no input frames to convert (the inputs matched no files)".into(),
                ));
            }
            let target = OutputTarget::resolve(shared, KnobNames::KeyOnly, false)?;
            for input in inputs {
                let output = default_output_name(&input, out_dir, target);
                planned.push(PlannedFrame {
                    input,
                    output,
                    recipe: shared.clone(),
                    overrides: None,
                });
            }
        }
    }
    Ok(planned)
}

/// Guard every roll write target (per-frame outputs, `--report-file`)
/// against every input scan and against one another — so a same-stem collision or
/// a target aimed at an input fails loudly up front rather than clobbering a scan
/// or a just-written sibling. The roll-input analogue of
/// [`ensure_write_targets_distinct`] (multiple inputs, case-insensitivity-aware).
fn ensure_roll_targets_distinct(inputs: &[&Path], targets: &[(String, PathBuf)]) -> Result<()> {
    let input_keys: Vec<PathBuf> = inputs.iter().map(|p| collision_key(p)).collect();
    let mut seen: Vec<(&str, PathBuf)> = Vec::with_capacity(targets.len());
    for (label, path) in targets {
        let key = collision_key(path);
        if input_keys.iter().any(|ik| keys_collide(ik, &key)) {
            return Err(NcError::Usage(format!(
                "{label} ({}) would overwrite an input scan",
                path.display()
            )));
        }
        if let Some((other, _)) = seen.iter().find(|(_, k)| keys_collide(k, &key)) {
            return Err(NcError::Usage(format!(
                "{label} ({}) collides with {other}",
                path.display()
            )));
        }
        seen.push((label.as_str(), key));
    }
    Ok(())
}

/// Map a successfully-converted frame's [`Report`] to its [`FrameReport`] entry.
fn frame_report_ok(pf: &PlannedFrame, report: Report) -> FrameReport {
    FrameReport {
        input: pf.input.clone(),
        output: Some(pf.output.clone()),
        status: FrameStatus::Ok {
            film_base: report.film_base,
            effective_area: report.effective_area.map(Box::new),
            input_color: report.input_color.map(Box::new),
            loss: report.loss,
            output_stats: report.output_stats,
            film_type: report.film_type,
            identity: report.identity.map(Box::new),
            chain: report.chain.map(Box::new),
            avif: report.avif.map(Box::new),
            hdr_linear_tiff: report.hdr_linear_tiff.map(Box::new),
            hdr_coded_tiff: report.hdr_coded_tiff.map(Box::new),
        },
        memory: report.memory,
        warnings: report.warnings,
        overrides: pf.overrides.clone(),
    }
}

/// A failed frame's [`FrameReport`] entry — the error message plus any warnings
/// accumulated before the failure point (decode/IR/film-base notices), so a frame
/// that warns and then fails still reports them (and they aren't lost to `--quiet`).
/// `memory` is whatever the preflight decided before the failure (it runs first, so
/// only a frame rejected *by* the gate has none).
fn frame_report_err(
    pf: &PlannedFrame,
    err: &NcError,
    memory: Option<MemoryReport>,
    warnings: Vec<String>,
) -> FrameReport {
    FrameReport {
        input: pf.input.clone(),
        output: Some(pf.output.clone()),
        status: FrameStatus::Failed {
            error: err.to_string(),
        },
        memory,
        warnings,
        overrides: pf.overrides.clone(),
    }
}

/// `hanten roll` — convert a batch of frames from one shared, frozen recipe (the
/// batch-apply scaffold, design-spec §8/§12 item 6). Resolves the plan (frames +
/// per-frame configs), guards write targets, then converts each frame through the
/// same [`convert_frame`] core `convert` uses — so per-frame output is
/// byte-identical to a single `convert` with the same effective recipe. A frame's
/// failure is recorded and the roll continues; the loud non-zero exit + per-frame
/// `error` in the roll report are the signal.
fn run_roll(args: RollArgs) -> Result<()> {
    let started = Instant::now();
    let log = Log::new(&args.report);
    reject_new_flow(args.new_flow)?;

    // Shared frozen recipe — validated once up front so a broken recipe fails
    // loudly before any frame is touched. Roll-specific rejections run first and the
    // missing-base rule last (`validate_roll_recipe`): a recipe that is both baseless
    // and roll-invalid should surface the roll problem first, or the user adds a base
    // only to meet a second error.
    let LoadedRecipe {
        recipe: shared,
        meta_pipeline_version,
    } = load_recipe(args.recipe_in.as_deref())?;
    validate_roll_recipe(&shared, "shared recipe")?;

    // A roll's headline guarantee is one frozen, roll-fixed film base shared by
    // every frame. Only an *explicit* base delivers that: `auto`/`region`
    // re-estimate `Dmin` from each frame's own pixels, so the roll is neither
    // frozen nor color-consistent even though the report still prints "one shared
    // recipe". Warn loudly (report + stderr, `--strict`-promotable) rather than
    // hard-failing, so a best-effort batch stays usable.
    let mut roll_warnings: Vec<String> = Vec::new();
    // A frozen recipe replayed under a different behavioral `pipeline_version` than
    // it was captured under is a roll-level fact (one shared recipe, N frames), so
    // it rides `roll_warnings` rather than any single frame's list.
    if let Some(msg) = pipeline_version_warning(meta_pipeline_version) {
        log.warn(&msg);
        roll_warnings.push(msg);
    }
    // The shared recipe's fallbacks and possible leftovers, once for the roll. A
    // per-frame override is that frame's explicit choice, so it does not warn.
    for msg in shared.recipe_warnings(recipe::TypedStyle::default()) {
        log.warn(&msg);
        roll_warnings.push(msg);
    }
    if !matches!(
        shared.calibration.film_base,
        Some(FilmBaseSource::Explicit(_))
    ) {
        // `validate_shared` above already rejected `None`, so only the two estimating
        // sources reach here.
        let kind = match shared.calibration.film_base {
            Some(FilmBaseSource::Auto) => "auto",
            Some(FilmBaseSource::Region(_)) => "region",
            Some(FilmBaseSource::Explicit(_)) | None => unreachable!("validate rejects both"),
        };
        let msg = format!(
            "roll film base is NOT frozen: calibration.film_base is `{kind}`, so every frame \
             estimates its own Dmin — the roll is not color-consistent and the shared \
             recipe is not truly shared. Calibrate the base once (e.g. `hanten estimate \
             --base-region X,Y,W,H <reference-scan>`), then set the reported explicit base \
             as `calibration.film_base` in the shared recipe."
        );
        log.warn(&msg);
        roll_warnings.push(msg);
    }

    // Resolve the plan. A per-frame override that changes a roll-wide value appends
    // its own roll-level warning here (warn-and-continue, like the not-frozen warning
    // above), so `roll_warnings` is passed in to collect it.
    let planned = resolve_frames(&args, &shared, &mut roll_warnings, &log)?;

    // Guard every write target (per-frame outputs, and the report file) against every
    // input and against one another before writing anything. The `--frames` manifest
    // is a read input too — a write target aimed at it (e.g. `--report-file` equal to
    // the manifest path) must be rejected, not silently clobbered — so include it in
    // the protected read set.
    let mut inputs: Vec<&Path> = planned.iter().map(|p| p.input.as_path()).collect();
    if let Some(frames) = args.frames.as_deref() {
        inputs.push(frames);
    }
    let mut targets: Vec<(String, PathBuf)> = Vec::new();
    for pf in &planned {
        targets.push((
            format!("output for {}", pf.input.display()),
            pf.output.clone(),
        ));
    }
    if let Some(rf) = args.report.report_file.as_deref() {
        targets.push(("--report-file".to_string(), rf.to_path_buf()));
    }
    ensure_roll_targets_distinct(&inputs, &targets)?;

    // Create the output directory now that the plan is known-good. A manifest may
    // name a per-frame output in a subdirectory (`sub/x.tiff`), so create each
    // frame's output parent too — otherwise the encode fails on a missing dir.
    std::fs::create_dir_all(&args.out_dir).map_err(|e| {
        NcError::Write(format!(
            "cannot create --out-dir {}: {e}",
            args.out_dir.display()
        ))
    })?;
    for pf in &planned {
        if let Some(parent) = pf.output.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| {
                NcError::Write(format!(
                    "cannot create output directory {}: {e}",
                    parent.display()
                ))
            })?;
        }
    }

    let mut frames = Vec::with_capacity(planned.len());
    let (mut succeeded, mut failed) = (0usize, 0usize);
    for pf in &planned {
        log.info(format_args!("converting {}", pf.input.display()));
        // Per-frame warnings and the preflight decision accumulate here so a frame
        // that warns / gets sized and *then* fails still hands them back (the report
        // only rides out on success).
        let mut warnings = Vec::new();
        let mut facts = FrameFacts::default();
        match convert_frame(
            "roll",
            &pf.input,
            &pf.output,
            &pf.recipe,
            InputFromCli::none(),
            &args
                .recipe_in
                .iter()
                .chain(args.frames.iter())
                .map(PathBuf::as_path)
                .collect::<Vec<_>>(),
            args.memory.budget(),
            &mut facts,
            &log,
            &mut warnings,
        ) {
            Ok(frame) => {
                succeeded += 1;
                frames.push(frame_report_ok(pf, frame.report));
            }
            Err(e) => {
                failed += 1;
                // Batch resilience: one frame's failure is recorded and the roll
                // continues (the loud non-zero exit + per-frame `error` are the
                // signal). Echo to stderr too; stdout stays the JSON report.
                log.warn(&format!("frame {} failed: {e}", pf.input.display()));
                frames.push(frame_report_err(pf, &e, facts.memory, warnings));
            }
        }
    }

    // `--strict` promotes any warning to a failing exit (convert's gate,
    // aggregated across the roll): both the roll-level warnings (e.g. the base is
    // not frozen) and any per-frame warning. Decided before the report is emitted.
    let strict_failure =
        args.strict && (!roll_warnings.is_empty() || frames.iter().any(|f| !f.warnings.is_empty()));

    let total = frames.len();
    let roll = RollReport {
        command: "roll",
        identity: Identity::new(),
        warnings: roll_warnings,
        frames,
        summary: RollSummary {
            total,
            succeeded,
            failed,
        },
        elapsed_ms: Some(elapsed_ms(started)),
    };
    // Emit the report before the failure gates so the machine-readable per-frame
    // record still lands even when the roll then exits non-zero (convert/estimate
    // contract).
    emit_json(
        &roll,
        args.report.report,
        args.report.report_file.as_deref(),
        &log,
    )?;

    if failed > 0 {
        return Err(NcError::Other(format!(
            "roll: {failed} of {total} frame(s) failed to convert (see report)"
        )));
    }
    if strict_failure {
        return Err(NcError::Other(
            "--strict: the roll produced warnings (see report)".into(),
        ));
    }
    Ok(())
}

/// `hanten inspect` — decode a scan and report what was found (format, dimensions,
/// channels, bit depth, IR presence, scanner metadata) plus a best-effort
/// suggested `Dmin`. No output image is written.
fn run_inspect(args: IoArgs) -> Result<()> {
    let started = Instant::now();
    let log = Log::new(&args.report);

    // A bad `--measure-inset` is a *usage* error (exit 2), not a diagnostic that
    // degrades to a warning: these commands resolve no recipe, so `validate` never
    // sees the flag and the best-effort `effective_area` call below would swallow
    // it at exit 0. Checked before the decode, so a 160 MB read is not wasted on a
    // typo.
    if let Some(f) = args.measure.measure_inset {
        check_measure_inset(f)?;
    }

    if let Some(rf) = args.report.report_file.as_deref() {
        ensure_write_targets_distinct(&args.input, &[("--report-file", rf)])?;
    }
    let mut report = Report {
        command: Some("inspect"),
        // `identity` is on EVERY report (design-spec §9), not just conversions —
        // an `inspect` result is an artifact someone files, and "which build read
        // this scan" is exactly as attributable a question. `Identity::new` is the
        // no-recipe constructor: `inspect` resolves no recipe, so `params_hash` is
        // genuinely absent rather than a construction artifact.
        identity: Some(Identity::new()),
        input: Some(args.input.clone()),
        ..Report::default()
    };

    // Memory preflight before decode, on the decode-only profile: `inspect` never
    // renders or encodes, so gating it on the full-pipeline peak would reject
    // scans it can diagnose comfortably. It always runs the auto detector below
    // (the suggested `Dmin`), so the film-base phase counts its interior sample.
    let budget = args.memory.budget();
    report.memory = Some(preflight_memory(
        &args.input,
        RunProfile::DecodeOnly,
        SamplePlan::auto(),
        budget,
        memory::detect_total_ram(),
        &log,
        &mut report.warnings,
    )?);

    let (image, info) = decode_within(&args.input, budget.bytes())?;
    log.info(format_args!(
        "decoded {:?} {}x{} (ir={})",
        info.format, info.width, info.height, info.ir_present
    ));

    for w in &info.warnings {
        push_warning(&mut report, &log, w.clone());
    }

    // Resolve the input color semantics with no user assertions (auto/auto) so the
    // report shows the file's *intrinsic* evidence — transfer + measurement meaning
    // with per-axis evidence and a safe ICC summary. `inspect` is diagnostic: it
    // reports even ambiguous/unsupported inputs (it never gates like `convert`),
    // so `resolve` cannot error here (auto assertions never contradict structure).
    let input_meta =
        input_semantics::resolve(&container_color_facts(&info), &InputAssertions::auto())
            .expect("auto/auto resolution never fails");
    let input_report = InputColorReport::from_metadata(&input_meta);
    if input_report.icc_unparsable() {
        push_warning(
            &mut report,
            &log,
            "embedded ICC profile present but could not be parsed for a summary".into(),
        );
    }
    report.input_color = Some(input_report);

    // IR film-holder mask (on a scan carrying a *marker-verified* IR plane that
    // measures usable). Diagnostic on `inspect`: it shows which along-edge segments
    // the opaque holder occludes, and drives the film-segment restriction the
    // candidate search below uses. The verdict is measured
    // (`ir-usability-detection`) — `--film-type` takes no part in it.
    let ir_separability = film_base::ir_separability(&image);
    let ir_usable = ir_separability.is_some_and(|s| s.usable);
    report.ir_separability = ir_separability;
    report.film_type = args.film_type.filter(|&t| t != FilmType::Unknown);
    // Build the mask *before* the notes, so each one describes what actually
    // happened rather than predicting it: a usable plane can still produce no mask
    // (a too-small image errors on `scan_depth`; an all-holder mask falls back).
    // Best-effort, like the candidate search below — `inspect` is informational and
    // must not abort over a diagnostic.
    let mut mask_error = false;
    match film_base::ir_holder_mask(&image) {
        Ok(mask) => report.holder_mask = mask,
        Err(e) => {
            mask_error = true;
            push_warning(
                &mut report,
                &log,
                format!("holder-mask detection skipped — {e}"),
            );
        }
    }
    // The effective measurement area: the holder depth march plus the static
    // inset. Independent of the along-edge mask above — it deliberately does not
    // inherit `ir_holder_mask`'s all-holder decline, so a frame whose holder wraps
    // the whole border is measured here even when the mask above is `None`.
    // Best-effort like every other `inspect` diagnostic.
    match film_base::effective_area(
        &image,
        args.measure.measure_inset.unwrap_or(DEFAULT_MEASURE_INSET),
    ) {
        Ok(area) => {
            report.effective_area = Some(area);
            for w in film_base::effective_area_warnings(&area) {
                push_warning(&mut report, &log, w);
            }
        }
        Err(e) => push_warning(
            &mut report,
            &log,
            // `message()`, not `{e}`: `Display` prefixes the kind, so this
            // rendered as "skipped — usage: …" — a warning announcing an error
            // inside itself.
            format!("effective-area resolution skipped — {}", e.message()),
        ),
    }
    // Both IR consumers, each reporting what it did rather than what the inputs
    // predict. The holder march is the second one: on 22 of 25 real chromogenic
    // frames the mask above declines while the march measures a holder and moves
    // the reported rectangle, and keying the note on the mask alone made one report
    // carry both a measured `effective_area.holder` and "preserved but not used".
    let ir_consumed = report.holder_mask.is_some()
        || report
            .effective_area
            .is_some_and(|a: film_base::EffectiveArea| a.holder_applied);

    if info.ir_present && !image.ir_verified {
        // Shape-only IR plane: carried/exportable but not trusted for detection.
        push_warning(
            &mut report,
            &log,
            "an IR plane is present, but it is identified by shape alone (no \
             NewSubfileType=4 marker) and not trusted for holder detection; \
             film-holder detection is RGB-only"
                .into(),
        );
    } else if info.ir_present && !ir_usable {
        // Trusted, but this frame's own film is too opaque to tell from the holder.
        push_warning(
            &mut report,
            &log,
            format!(
                "the IR plane cannot separate the film holder on this frame (interior \
                 IR transmission {:.4}); film-holder detection is RGB-only",
                ir_separability.map_or(0.0, |s| s.interior_median)
            ),
        );
    } else if info.ir_present && !ir_consumed && !mask_error {
        // Marker-verified and measured usable, yet **neither** reader consumed it:
        // "both declined for a reason neither note above named". This is no longer
        // the all-holder fallback — that frame marches to a ring, so
        // `holder_applied` is true and `ir_consumed` short-circuits here. With a
        // usable plane the only combination left is an errored `effective_area`
        // beside an all-holder mask, and the "effective-area resolution skipped"
        // warning above already describes that event.
        //
        // Kept as a safety net all the same, and deliberately *not* gated on
        // `report.effective_area.is_some()`: if a third IR reader ever lands, this
        // is the branch that keeps saying so rather than going quiet.
        // (`mask_error` already said its piece.)
        push_warning(
            &mut report,
            &log,
            "input carries an IR plane; preserved but not used in Step 1 \
             (use `convert --export-ir` to write it out)"
                .into(),
        );
    }

    // Candidate rebate bands + suggested Dmin via the inward-scan detector. For
    // inspect this is informational — a refusal is a note, not fatal — and the
    // candidates are reported even when selection refuses, so the user can
    // confirm a rectangle for `--base-region` instead of measuring one.
    match film_base::rebate_candidates(&image, report.holder_mask.as_deref()) {
        Ok(candidates) => {
            match film_base::select_auto_base(&image, &candidates) {
                Ok(est) => {
                    report.film_base = Some(est.base);
                    report.film_base_source = Some(FilmBaseSource::Auto);
                    for w in est.warnings {
                        push_warning(&mut report, &log, w);
                    }
                }
                // The selection error already carries actionable advice (pass
                // --base-region/--film-base, or --base-content per the
                // film-base/content-fallback task); short lead-in only.
                Err(e) => push_warning(
                    &mut report,
                    &log,
                    format!("suggested Dmin unavailable — {e}"),
                ),
            }
            if !candidates.is_empty() {
                report.base_candidates = Some(candidates);
            }
        }
        Err(e) => push_warning(
            &mut report,
            &log,
            format!("film-base detection skipped — {e}"),
        ),
    }

    report.decode = Some(info);
    report.elapsed_ms = Some(elapsed_ms(started));
    emit_report(
        &report,
        args.report.report,
        args.report.report_file.as_deref(),
        &log,
    )
}

/// Reuse-ready forms of a measured base: a paste-ready `--film-base R,G,B`
/// flag string and the matching `calibration.film_base` value — or `None` when
/// the measurement fails the explicit-base validation `convert` applies (each
/// channel in `(0, 1]`), so a degenerate base is never advertised as reusable.
/// `f32`'s `Display` prints the shortest round-tripping decimal, so both forms
/// reproduce the exact measured value when fed back to `convert`.
fn reuse_ready(rgb: [f32; 3]) -> Option<(String, FilmBaseSource)> {
    validate_explicit_film_base(&rgb).ok()?;
    Some((
        format!("--film-base {},{},{}", rgb[0], rgb[1], rgb[2]),
        FilmBaseSource::Explicit(rgb),
    ))
}

/// `hanten estimate` — run only film-base / `Dmin` estimation from the selected
/// source (default `auto`, or `--base-region`/`--film-base`; `--grid` samples
/// a 5-cell grid for unexposed-frame calibration) and emit the resolved
/// [`FilmBase`] as JSON — together with reuse-ready forms of it (a
/// `--film-base` flag string and a `film_base` recipe fragment) when the
/// measurement is usable as an explicit base (each channel in `(0, 1]`;
/// otherwise a warning explains why not) — so the measured value drops
/// straight into a `convert` call or a roll recipe (design-spec §8). Auto
/// detection may fail loudly on real scans; that propagates as an error (the
/// user asked for an estimate we can't give). `--strict` promotes warnings
/// (e.g. grid disagreement) to a failing exit after the report is emitted.
///
/// **The `unwrap_or(FilmBaseSource::Auto)` below is the only surviving default
/// film-base choice in the crate, and no fingerprint watches it.** Since
/// `calibration.film_base` lost its default, `version::PIPELINE_FINGERPRINTS` no
/// longer covers this decision from either side: `base` pins the detector by
/// naming `Auto` explicitly, and `recipe` sees the resolved config's `null`.
/// Changing what an unstated `estimate` resolves to would therefore move every
/// `hanten estimate` result with the whole drift gate green — verify such a change by
/// hand, and do not assume the gate is watching.
fn run_estimate(args: EstimateArgs) -> Result<()> {
    let started = Instant::now();
    let log = Log::new(&args.report);

    if args.d_max_region.is_some() {
        return Err(NcError::Usage(removed_dmax_message(
            "--d-max-region",
            "measured the roll reference density off a light-struck leader",
        )));
    }

    // A bad `--measure-inset` is a *usage* error (exit 2), not a diagnostic that
    // degrades to a warning: these commands resolve no recipe, so `validate` never
    // sees the flag and the best-effort `effective_area` call below would swallow
    // it at exit 0. Checked before the decode, so a 160 MB read is not wasted on a
    // typo.
    if let Some(f) = args.measure.measure_inset {
        check_measure_inset(f)?;
    }

    if let Some(rf) = args.report.report_file.as_deref() {
        ensure_write_targets_distinct(&args.input, &[("--report-file", rf)])?;
    }
    // `estimate` exists to *produce* a base, so requiring one first would be
    // circular: here — and only here — an unstated source still means `auto`.
    let source = film_base_source_override(&args.film_base).unwrap_or(FilmBaseSource::Auto);
    // Guard an explicit base with the same check `convert` applies (a recipe
    // never reaches estimate, but a bad `--film-base` must fail loudly rather
    // than be echoed back). Region bounds are checked by `film_base::estimate`.
    if let FilmBaseSource::Explicit(b) = &source {
        validate_explicit_film_base(b)?;
    }

    let mut report = Report {
        command: Some("estimate"),
        // Same contract as `inspect`: build identity on every report, no
        // `params_hash` because no recipe was resolved. An estimated `Dmin` is
        // routinely frozen into a roll recipe, so which build measured it matters.
        identity: Some(Identity::new()),
        input: Some(args.input.clone()),
        ..Report::default()
    };

    // Memory preflight before decode (decode-only profile — `estimate` samples the
    // decoded image and stops). Its film-base phase is the largest rectangle this
    // invocation will gather: the base source's own sample (`--grid` samples cells
    // of `--base-region`, or of the whole frame when it is absent — counted
    // conservatively as the whole rectangle).
    let budget = args.memory.budget();
    let sampling = if args.grid {
        match args.film_base.base_region {
            // `--grid` samples five cells of the rectangle, one at a time, so the
            // phase peaks at one cell — not at the whole rectangle.
            Some([_, _, w, h]) => SamplePlan::rect(film_base::grid_cell_pixels(w, h)),
            None => SamplePlan::none().with_whole_frame_grid(),
        }
    } else {
        sample_plan(&source)
    };
    report.memory = Some(preflight_memory(
        &args.input,
        RunProfile::DecodeOnly,
        sampling,
        budget,
        memory::detect_total_ram(),
        &log,
        &mut report.warnings,
    )?);

    let (image, info) = decode_within(&args.input, budget.bytes())?;
    log.info(format_args!(
        "decoded {:?} {}x{} (ir={})",
        info.format, info.width, info.height, info.ir_present
    ));

    for w in &info.warnings {
        push_warning(&mut report, &log, w.clone());
    }

    // Mirror `convert`'s notes: only the `auto` single-measurement path consults the
    // IR holder mask, so it degrades to RGB-only when the IR plane is shape-only
    // (unverified provenance) or measures unable to separate holder from film. The
    // `--grid` and explicit/region paths never touch IR, so they need no note.
    report.film_type = args.film_type.filter(|&t| t != FilmType::Unknown);
    // The calibration command reports the measurement itself, not just a warning
    // about it: `estimate` is where a user decides how to acquire a base, so the
    // number that drove the decision belongs in its artifact too.
    report.ir_separability = film_base::ir_separability(&image);
    // Same rationale for the effective area: `estimate` is where a user decides how
    // to acquire a base, so the region a measurement would be read over belongs in
    // its artifact. It does not (yet) drive this command's estimate — that is
    // `film-base/holder-masked-measurement`'s change.
    match film_base::effective_area(
        &image,
        args.measure.measure_inset.unwrap_or(DEFAULT_MEASURE_INSET),
    ) {
        Ok(area) => {
            report.effective_area = Some(area);
            for w in film_base::effective_area_warnings(&area) {
                push_warning(&mut report, &log, w);
            }
        }
        Err(e) => push_warning(
            &mut report,
            &log,
            // `message()`, not `{e}`: `Display` prefixes the kind, so this
            // rendered as "skipped — usage: …" — a warning announcing an error
            // inside itself.
            format!("effective-area resolution skipped — {}", e.message()),
        ),
    }
    let mut ir_note_pending = !args.grid && matches!(source, FilmBaseSource::Auto);
    if ir_note_pending && info.ir_present {
        let sep = report.ir_separability;
        if !image.ir_verified {
            ir_note_pending = false;
            push_warning(
                &mut report,
                &log,
                "an IR plane is present, but it is identified by shape alone (no \
                 NewSubfileType=4 marker) and not trusted for holder detection; \
                 using RGB-only film-holder detection for the film base"
                    .into(),
            );
        } else if !sep.is_some_and(|s| s.usable) {
            ir_note_pending = false;
            push_warning(
                &mut report,
                &log,
                format!(
                    "the IR plane cannot separate the film holder on this frame \
                     (interior IR transmission {:.4}); using RGB-only film-holder \
                     detection for the film base",
                    sep.map_or(0.0, |s| s.interior_median)
                ),
            );
        }
    }

    let base = if args.grid {
        // Grid calibration: clap rejects `--grid` with `--film-base` /
        // `--auto-base`, so the rectangle is `--base-region` or the full frame.
        let rect = args
            .film_base
            .base_region
            .unwrap_or([0, 0, image.width, image.height]);
        let grid = film_base::estimate_grid(&image, rect)?;
        if !grid.agreement {
            // The 1.0 spread sentinel also fires when a channel's cells all
            // measure ~0 (a degenerate sample, not a light leak); diagnose by
            // the combined base so the warning names the actual problem.
            let msg = if <[f32; 3]>::from(grid.base).iter().any(|v| *v <= 0.0) {
                format!(
                    "grid measured non-positive transmission (combined base \
                     [{}, {}, {}]) — degenerate sample, not film base; was the \
                     sampled area unexposed film? See the report's grid.cells",
                    grid.base.r, grid.base.g, grid.base.b
                )
            } else {
                format!(
                    "grid cells disagree: per-channel relative spread \
                     [{:.4}, {:.4}, {:.4}] exceeds tolerance {} — possible light \
                     leak, scanner illumination falloff, or dust; see the \
                     report's grid.cells for the per-region values",
                    grid.spread[0], grid.spread[1], grid.spread[2], grid.tolerance
                )
            };
            push_warning(&mut report, &log, msg);
        }
        // The source records the overall rectangle the grid sampled; the
        // `grid` report field documents the per-cell method.
        report.film_base_source = Some(FilmBaseSource::Region(rect));
        let base = grid.base;
        report.grid = Some(grid);
        base
    } else {
        // Single-measurement path: `film_base::estimate` guards the base
        // finite-and-positive at birth (auto-base-redesign) and may attach
        // quality warnings (non-uniform region, cross-edge disagreement). The
        // `auto` source uses the IR holder mask when the scan carries an IR plane
        // that measures able to separate holder from film.
        let est = film_base::estimate(&image, &source)?;
        report.film_base_source = Some(source);
        for w in est.warnings {
            push_warning(&mut report, &log, w);
        }
        // A plane that survived both notes above and still went unused **for the
        // film base**. Read off what stage 2 did, never predicted.
        //
        // The all-holder fallback is no longer the only route here: since
        // `holder-depth-mask`, `estimate` also resolves the effective area, whose
        // march can measure a holder on the same frame. This note deliberately does
        // **not** gain that disjunct — the scoping to "for the film base" is what
        // makes it honest, and it is load-bearing rather than incidental. The film
        // base really did not use the plane, which is the verdict a calibration
        // command owes; `inspect` (which reports on the whole Step-1 read) counts
        // both readers, and `convert` counts the region only when it reaches a
        // pixel. Three questions, three rules, each individually honest — adding the
        // march here would make *this* message wrong.
        if info.ir_present && image.ir_verified && !est.ir_mask_applied && ir_note_pending {
            push_warning(
                &mut report,
                &log,
                "input carries an IR plane; preserved but not used for the film base".into(),
            );
        }
        est.base
    };
    report.film_base = Some(base);

    // Reuse-ready forms — attached only when the measurement passes the
    // explicit-base validation `convert` applies: a base outside `(0, 1]` on any
    // channel is still reported as the measurement, but never as "reuse-ready".
    // The single-measurement path already errors on a degenerate base via
    // `estimate`'s guard; the grid path's degenerate (`<= 0` / non-finite)
    // combined base is hard-errored below, *after* the report is emitted — so
    // this suppression keeps that emitted report from advertising the degenerate
    // value as reusable, and still stands alone for a non-degenerate but
    // out-of-range base (a channel `> 1`).
    //
    // Deliberately independent of grid *agreement*: a `--grid` run whose cells
    // disagree (light leak / falloff / dust) still emits reuse-ready output when
    // the combined median base is in range — the median resists a single bad
    // cell, and the disagreement already rides `warnings`. A consumer treating
    // the base as authoritative must check `warnings` (or run `--strict`, which
    // promotes the disagreement to a hard failure); only a *degenerate* base
    // withholds the reuse forms. (Design-spec §8.)
    match reuse_ready(<[f32; 3]>::from(base)) {
        Some((flag, source)) => {
            report.reuse = Some(ReuseReady { flag, source });
        }
        None => push_warning(
            &mut report,
            &log,
            format!(
                "measured base {:?} is not usable as an explicit --film-base \
                 (channels must be in (0, 1]) — was the sampled area unexposed \
                 film base? No reuse-ready output emitted",
                <[f32; 3]>::from(base)
            ),
        ),
    }

    // The recipe-shaped handoff, assembled from the reuse-ready pairs rather than
    // from the measurements directly: that is what keeps each key present exactly
    // when its flag is, and withholds a value the reuse gates declined to advertise.
    // Built here, after every gate above has decided.
    report.calibration = calibration_fragment(&report);

    report.elapsed_ms = Some(elapsed_ms(started));
    // Emit the report before the `--strict` gate so the machine-readable record
    // (the measured base) lands even when a warning then fails the run (same
    // contract as `convert`).
    emit_report(
        &report,
        args.report.report,
        args.report.report_file.as_deref(),
        &log,
    )?;
    // A degenerate grid combined base (non-finite or <= 0 on any channel — e.g.
    // `--grid --base-region` on the dark holder) cannot anchor the density
    // divide, so hard-error **regardless of `--strict`**, mirroring the
    // single-measurement path where `film_base::estimate`'s finite-and-positive
    // guard rejects the same condition at birth. Same `NcError::Other` (exit 1)
    // as that guard, so both estimate paths map a degenerate base to one exit
    // code. The diagnostic report (with `grid.cells` and the per-cell warning) is
    // emitted above first, so the evidence lands before this gate.
    if args.grid
        && <[f32; 3]>::from(base)
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0)
    {
        return Err(NcError::Other(format!(
            "grid combined film base {:?} is not finite and positive on every \
             channel; it cannot anchor the density divide — was the sampled area \
             unexposed film base? See the report's grid.cells",
            <[f32; 3]>::from(base)
        )));
    }
    if args.strict && !report.warnings.is_empty() {
        return Err(NcError::Other(format!(
            "--strict: {} warning(s) present (see report)",
            report.warnings.len()
        )));
    }
    Ok(())
}

/// The report's `calibration` object, derived from the reuse-ready pairs.
///
/// **Derived, never assembled separately.** Each key is present exactly when its
/// own reuse-ready pair is, so the pairing invariant is enforced by construction
/// rather than by two call sites agreeing — and a measurement the reuse gates
/// declined to advertise (a base outside `(0, 1]`)
/// cannot leak into a fragment a user would pipe straight into `--params`.
///
/// `None` when nothing was measured: an empty `{}` would pipe into `--params` as a
/// no-op the user could mistake for a calibration.
fn calibration_fragment(report: &Report) -> Option<CalibrationFragment> {
    let fragment = CalibrationFragment {
        film_base: report.reuse.as_ref().map(|r| r.source.clone()),
    };
    (!fragment.is_empty()).then_some(fragment)
}

// ---------------------------------------------------------------------------
// measure-roll — the roll white balance (`nf-scene-correction/roll-white-balance`)
// ---------------------------------------------------------------------------

/// The `measure-roll` JSON report.
#[derive(Debug, Serialize)]
struct MeasureRollReport {
    command: &'static str,
    /// Which build measured the gains — they are frozen into a recipe and outlive it.
    identity: Identity,
    /// The film base every input was decoded with.
    film_base: FilmBase,
    /// The decode the gains belong to: they are measured at its output.
    decode: fixed::DecodeReport,
    /// The leader guard, when `--leader` was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    leader: Option<MeasuredLeader>,
    frames: Vec<MeasuredFrame>,
    white_balance: RollWhiteBalance,
    /// The roll's white and the look contrast that places it (`nf-calibration/roll-white-rule`).
    white: MeasuredRollWhite,
    /// The gains and the white in the forms a user freezes them in.
    reuse: RollReuse,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
    elapsed_ms: f64,
}

#[derive(Debug, Serialize)]
struct MeasuredLeader {
    input: PathBuf,
    #[serde(flatten)]
    guard: roll_white::LeaderGuard,
    /// The median of the leader's brightest channel in film RGB, before the working-space
    /// 3×3 — the measure a frame's white takes, and what its saturation distance is
    /// taken against.
    film_peak: f32,
    memory: MemoryReport,
}

#[derive(Debug, Serialize)]
struct MeasuredFrame {
    input: PathBuf,
    /// The effective area the frame was sampled over.
    region: [u32; 4],
    /// Whether a measured holder moved `region` (`convert`'s `effective_area.holder_applied`);
    /// otherwise the inset alone cut it.
    holder_applied: bool,
    #[serde(flatten)]
    counts: roll_white::FrameCounts,
    /// The frame's white — the percentile of its pixels' brightest channel, before the
    /// leader guard — in scene stops above mid-grey; absent when no pixel was usable.
    #[serde(skip_serializing_if = "Option::is_none")]
    white_stops: Option<f32>,
    /// How far the white sits under the leader, in scene stops; only with `--leader`.
    #[serde(skip_serializing_if = "Option::is_none")]
    leader_distance_stops: Option<f32>,
    /// The frame's part in the roll's white.
    white_role: roll_white::FrameRole,
    memory: MemoryReport,
}

#[derive(Debug, Serialize)]
struct RollWhiteBalance {
    /// Green-anchored gains for `roll.white_balance`.
    gains: [f32; 3],
    /// The per-channel percentile of the pooled pixels they equalize.
    percentile: f32,
    /// Pixels pooled over the whole roll.
    pooled: usize,
}

/// The roll's white, placed by the rule `roll_white` documents.
#[derive(Debug, Serialize)]
struct MeasuredRollWhite {
    /// In scene stops above mid-grey.
    stops: f32,
    /// Which limit set it: `none` (a frame's own white), `floor` or `cap`.
    bound: roll_white::WhiteBound,
    /// The frame it was taken from, when no limit bound.
    #[serde(skip_serializing_if = "Option::is_none")]
    from: Option<PathBuf>,
    /// The look contrast `roll.white_stops` renders at: the white at diffuse white,
    /// mid-grey pinned — at `scene_correction.exposure` 0. The look expands an exposure too, so with one set
    /// the white lands `exposure · contrast` stops off diffuse white.
    contrast: f32,
    /// `contrast` times the decode's linearization — the whole slope, for comparison
    /// only; the recipe stores `stops` (`roll.white_stops`).
    whole_contrast: f32,
    /// Frames above the cap, rendered at the cap's contrast rather than the roll's.
    /// Disclosed, not warned about: an ordinary bright scene lands here too.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    clamped: Vec<ClampedFrame>,
    rule: WhiteRule,
}

#[derive(Debug, Serialize)]
struct ClampedFrame {
    input: PathBuf,
    white_stops: f32,
    /// Its look contrast, the cap's — against the roll's `contrast`.
    contrast: f32,
    /// This frame's own `convert` flags: `reuse.flag` carries the roll's white, which
    /// would undo the clamp on this frame.
    flag: String,
}

/// The `convert` flags that freeze `gains` and the white `white_stops` — `reuse.flag`,
/// and a clamped frame's own.
fn reuse_flag(gains: [f32; 3], white_stops: f32) -> String {
    format!(
        "--roll-white-balance {},{},{} --roll-white {white_stops}",
        gains[0], gains[1], gains[2]
    )
}

/// The rule's values, provisional (`nf-calibration/roll-white-rule`).
#[derive(Debug, Serialize)]
struct WhiteRule {
    /// What each pixel contributes: `max`, its brightest channel.
    channel: &'static str,
    percentile: f32,
    cap_stops: f32,
    floor_stops: f32,
    /// Absent without `--leader`: there is then no saturation check.
    #[serde(skip_serializing_if = "Option::is_none")]
    saturation_margin_stops: Option<f32>,
}

#[derive(Debug, Serialize)]
struct RollReuse {
    /// For `convert`, on every frame but a clamped one, which takes its own
    /// `white.clamped[].flag`.
    flag: String,
    /// A partial recipe, to merge into the roll's.
    recipe: RollFragment,
    /// A `roll --frames` manifest giving each clamped frame its own white (the cap); only
    /// when a frame renders at a contrast other than the roll's. Input paths are as given
    /// here.
    #[serde(skip_serializing_if = "Option::is_none")]
    frames: Option<ClampManifest>,
}

/// `{"roll": {"white_balance": [r, g, b], "white_stops": w}}` — the recipe's `roll`
/// section (`nf-calibration/roll-section`), typed rather than built as a
/// `serde_json::Value` so the values print in their `f32` form — a `Value` widens them to
/// `f64` digits the flag form does not show.
#[derive(Debug, Serialize)]
struct RollFragment {
    roll: recipe::RollSection,
}

#[derive(Debug, Serialize)]
struct ClampManifest {
    frames: Vec<ClampManifestFrame>,
}

#[derive(Debug, Serialize)]
struct ClampManifestFrame {
    input: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<ClampParams>,
}

/// A clamped frame's own white. Only `white_stops`, so merging it over the roll's
/// recipe keeps the roll's gains.
#[derive(Debug, Serialize)]
struct ClampParams {
    roll: ClampedWhite,
}

#[derive(Debug, Serialize)]
struct ClampedWhite {
    white_stops: f32,
}

/// One input decoded into linear ACEScg at the recipe's decode, plus its effective
/// area: the front half of `convert_frame`, gated the same way
/// (memory preflight, input semantics, positive-mode refusal) and stopping before
/// scene correction — the point the roll's gains will be applied at.
///
/// `on_film` reads the decode's film RGB just before the working-space map, for what is
/// measured there (the roll's white); it runs on the frame's effective area as found.
///
/// **A copy, and it must stay in step.** `convert_frame`'s front half is tangled with
/// its report and IR notes, so this repeats its gates rather than sharing them: a
/// refusal or measurement-region warning added there belongs here too, or gains get
/// frozen from frames `convert` would refuse.
fn decode_for_roll_white<T>(
    input: &Path,
    recipe: &Recipe,
    base: &FilmBase,
    budget: memory::Budget,
    log: &Log,
    warnings: &mut Vec<String>,
    on_film: impl FnOnce(&FilmRgbImage, &Result<film_base::EffectiveArea>) -> Result<T>,
) -> Result<(
    AcesCgImage,
    Result<film_base::EffectiveArea>,
    T,
    fixed::DecodeReport,
    MemoryReport,
)> {
    let memory = preflight_memory(
        input,
        RunProfile::MeasureRoll,
        SamplePlan::none(),
        budget,
        memory::detect_total_ram(),
        log,
        warnings,
    )?;
    let (image, info) = decode_within(input, budget.bytes())?;
    log.info(format_args!(
        "decoded {} {}x{}",
        input.display(),
        info.width,
        info.height
    ));
    for w in &info.warnings {
        push_warning_buf(warnings, log, format!("{}: {w}", input.display()));
    }
    let input_meta = input_semantics::resolve(
        &container_color_facts(&info),
        &input_assertions(&recipe.input, InputFromCli::none()),
    )?;
    if InputColorReport::from_metadata(&input_meta).icc_unparsable() {
        push_warning_buf(
            warnings,
            log,
            format!(
                "{}: embedded ICC profile present but could not be parsed for a summary",
                input.display()
            ),
        );
    }
    input_semantics::require_convertible(&input_meta)?;
    reject_positive_mode(&info)?;
    let area = film_base::effective_area(&image, recipe.measure.inset);
    let (film, decoded) = fixed::decode(&image, base, &recipe.reconstruction)?;
    drop(image);
    let measured = on_film(&film, &area)?;
    Ok((
        working_space::map_nc_film_rgb_v1(film),
        area,
        measured,
        decoded,
        memory,
    ))
}

/// Place the roll's white from the measured frames, recording each frame's role.
fn measured_roll_white(
    frames: &mut [MeasuredFrame],
    guarded: bool,
    linearization: f32,
    gains: [f32; 3],
) -> Result<MeasuredRollWhite> {
    let stops: Vec<Option<f32>> = frames.iter().map(|f| f.white_stops).collect();
    let placed = roll_white::place_roll_white(&stops)?;
    for (f, role) in frames.iter_mut().zip(&placed.roles) {
        f.white_role = *role;
    }
    let contrast = roll_white::contrast_for(placed.stops);
    let cap_contrast = roll_white::contrast_for(roll_white::WHITE_CAP_STOPS);
    Ok(MeasuredRollWhite {
        stops: placed.stops,
        bound: placed.bound,
        from: frames
            .iter()
            .find(|f| f.white_role == roll_white::FrameRole::SetsRoll)
            .map(|f| f.input.clone()),
        contrast,
        whole_contrast: contrast * linearization,
        clamped: frames
            .iter()
            .filter(|f| f.white_role == roll_white::FrameRole::Clamped)
            .map(|f| ClampedFrame {
                input: f.input.clone(),
                white_stops: f.white_stops.expect("a clamped frame has a white"),
                contrast: cap_contrast,
                flag: reuse_flag(gains, roll_white::WHITE_CAP_STOPS),
            })
            .collect(),
        rule: WhiteRule {
            channel: "max",
            percentile: roll_white::WHITE_PERCENTILE,
            cap_stops: roll_white::WHITE_CAP_STOPS,
            floor_stops: roll_white::WHITE_FLOOR_STOPS,
            saturation_margin_stops: guarded.then_some(roll_white::SATURATION_MARGIN_STOPS),
        },
    })
}

/// The `roll --frames` manifest that renders each clamped frame at its own white, the
/// cap — `None` when every frame renders at the roll's (none clamped, or the roll's white
/// is the cap itself, which a clamped frame already has).
fn clamp_manifest(frames: &[MeasuredFrame], white: &MeasuredRollWhite) -> Option<ClampManifest> {
    let own = |f: &MeasuredFrame| {
        (f.white_role == roll_white::FrameRole::Clamped
            && white.bound != roll_white::WhiteBound::Cap)
            .then_some(roll_white::WHITE_CAP_STOPS)
    };
    frames
        .iter()
        .any(|f| own(f).is_some())
        .then(|| ClampManifest {
            frames: frames
                .iter()
                .map(|f| ClampManifestFrame {
                    input: f.input.clone(),
                    params: own(f).map(|white_stops| ClampParams {
                        roll: ClampedWhite { white_stops },
                    }),
                })
                .collect(),
        })
}

/// `hanten measure-roll` — measure a roll's white balance and white once, over its
/// picture frames, and report the gains and the contrast to freeze into its recipe.
fn run_measure_roll(args: MeasureRollArgs) -> Result<()> {
    let started = Instant::now();
    let log = Log::new(&args.report);

    let mut recipe = load_recipe(args.recipe_in.as_deref())?.recipe;
    if let Some(b) = args.film_base {
        recipe.calibration.film_base = Some(FilmBaseSource::Explicit(b));
    }
    if let Some(f) = args.measure.measure_inset {
        recipe.measure.inset = f;
    }
    // Before any decode: a bad inset would otherwise surface from the first frame's
    // effective area, blamed on that file.
    check_measure_inset(recipe.measure.inset)?;
    // What this command measures — the roll section, and the style knobs a roll recipe
    // may carry beside it — is never read, and none of it may refuse the run. Nor may
    // the rendering, since this command renders nothing: a roll recipe stating `direct`
    // beside a film master would otherwise be refused as that pair. Keys only: this
    // command takes none of the conversion flags.
    recipe.roll = recipe::RollSection::default();
    recipe.rendering = crate::rendering::Rendering::default();
    recipe.scene_correction = scene_correction::SceneCorrectionParams::default();
    recipe.look = recipe::LookKeys::default();
    recipe::validate(&recipe, KnobNames::KeyOnly)?;
    if args.strict && args.leader.is_none() {
        return Err(NcError::Usage(
            "--strict refuses an unguarded measurement: without --leader a fully exposed \
             frame among the inputs would set the roll's white balance, and no frame is \
             checked for film saturation. Pass the roll's leader scan"
                .into(),
        ));
    }
    // A frame named twice would weigh twice in the pool — silently, since every
    // frame contributes the same sample count. The leader named as a frame too (the
    // natural glob when it sits beside them) would pool its unguarded edges.
    let mut seen: Vec<(PathBuf, &Path)> = Vec::new();
    if let Some(leader) = &args.leader {
        let key = collision_key(leader);
        if let Some(input) = args
            .inputs
            .iter()
            .find(|i| keys_collide(&collision_key(i), &key))
        {
            return Err(NcError::Usage(format!(
                "{} is both the --leader and an input frame; every input is pooled as \
                 picture, so leave the leader out of the frames",
                input.display()
            )));
        }
    }
    for input in &args.inputs {
        let key = collision_key(input);
        if let Some((_, first)) = seen.iter().find(|(k, _)| keys_collide(k, &key)) {
            let spelled = if *first == input.as_path() {
                String::new()
            } else {
                format!(" (also as {})", first.display())
            };
            return Err(NcError::Usage(format!(
                "{} is named twice{spelled}; each frame is pooled once, so a repeat would \
                 double its weight in the roll's white",
                input.display()
            )));
        }
        seen.push((key, input));
    }
    let base = match recipe.calibration.film_base {
        Some(FilmBaseSource::Explicit(b)) => {
            validate_explicit_film_base(&b)?;
            FilmBase::from(b)
        }
        _ => {
            return Err(NcError::Usage(
                "measure-roll needs the roll's film base stated explicitly — `--film-base \
                 R,G,B` or `calibration.film_base` as `{\"explicit\": [r, g, b]}` in the \
                 recipe: a base estimated per frame would measure each frame under a \
                 different decode. Measure it once with `hanten estimate --grid \
                 <base.tif>`"
                    .into(),
            ));
        }
    };
    if let Some(rf) = args.report.report_file.as_deref() {
        let inputs: Vec<&Path> = args
            .inputs
            .iter()
            .map(PathBuf::as_path)
            .chain(args.leader.as_deref())
            .collect();
        for input in inputs {
            ensure_write_targets_distinct(input, &[("--report-file", rf)])?;
        }
    }

    let budget = args.memory.budget();
    let mut warnings = Vec::new();
    let mut decode = None;

    let leader = match &args.leader {
        Some(path) => {
            // Each error keeps its kind, and so its exit code (a memory refusal stays 6).
            let (aces, _, film_peak, _, memory) = decode_for_roll_white(
                path,
                &recipe,
                &base,
                budget,
                &log,
                &mut warnings,
                |film, _| {
                    roll_white::leader_peak(film.rgb(), film.width(), film.height())
                        .map_err(|e| e.prefixed(path.display()))
                },
            )?;
            // The guard first: it names the channel a bad leader lacks.
            let guard = roll_white::leader_guard(
                aces.rgb(),
                aces.width(),
                aces.height(),
                recipe.reconstruction.linearization,
            )
            .map_err(|e| e.prefixed(path.display()))?;
            let film_peak = film_peak.ok_or_else(|| {
                NcError::Other(format!(
                    "{}: leader: no usable pixel — is the file a fully exposed leader, decoded \
                     with the roll's film base?",
                    path.display()
                ))
            })?;
            Some(MeasuredLeader {
                input: path.clone(),
                guard,
                film_peak,
                memory,
            })
        }
        None => {
            push_warning_buf(
                &mut warnings,
                &log,
                "no --leader: the measurement is unguarded, so a fully exposed frame among \
                 the inputs would set the roll's white balance (measured: it moves the gains \
                 0.4–1.3 stops), and no frame is checked for film saturation. Pass the \
                 roll's leader scan"
                    .into(),
            );
            None
        }
    };

    let mut pool = Vec::new();
    let mut frames = Vec::with_capacity(args.inputs.len());
    for input in &args.inputs {
        let (aces, area, white, decoded, memory) = decode_for_roll_white(
            input,
            &recipe,
            &base,
            budget,
            &log,
            &mut warnings,
            |film, area| match area {
                Ok(a) => roll_white::frame_white(film.rgb(), film.width(), a.region)
                    .map_err(|e| e.prefixed(input.display())),
                // Refused just below, with the input named.
                Err(_) => Ok(None),
            },
        )?;
        let area = area.map_err(|e| {
            NcError::Usage(format!(
                "{}: {} (every frame is measured over its effective area)",
                input.display(),
                e.message()
            ))
        })?;
        // A capped or unsettled holder march leaves holder in the region, and the pool
        // would take it as picture — the same warning `convert` gives, so `--strict`
        // sees it.
        for w in film_base::effective_area_warnings(&area) {
            push_warning_buf(&mut warnings, &log, format!("{}: {w}", input.display()));
        }
        let counts = roll_white::pool_frame(
            aces.rgb(),
            aces.width(),
            area.region,
            leader.as_ref().map(|l| &l.guard),
            &mut pool,
        )?;
        let leader_distance_stops = leader
            .as_ref()
            .zip(white)
            .map(|(l, w)| roll_white::leader_distance_stops(w, l.film_peak));
        if let Some(d) = leader_distance_stops.filter(|d| roll_white::near_saturation(*d)) {
            push_warning_buf(
                &mut warnings,
                &log,
                format!(
                    "{}: near film saturation — its white sits {d:.2} stop under the \
                     leader (margin {} stop); the film compresses highlights there, which \
                     the decode renders flat",
                    input.display(),
                    roll_white::SATURATION_MARGIN_STOPS
                ),
            );
        }
        if counts.kept == 0 {
            push_warning_buf(
                &mut warnings,
                &log,
                format!(
                    "{}: contributed no pixel ({} guarded, {} unusable) — is it a picture \
                     frame of this roll?",
                    input.display(),
                    counts.guarded,
                    counts.unusable
                ),
            );
        }
        frames.push(MeasuredFrame {
            input: input.clone(),
            region: area.region,
            holder_applied: area.holder_applied,
            counts,
            white_stops: white.map(roll_white::scene_stops),
            leader_distance_stops,
            // Placed once every frame is measured, below.
            white_role: roll_white::FrameRole::Unmeasured,
            memory,
        });
        decode.get_or_insert(decoded);
    }
    let gains = roll_white::roll_gains(&pool)?;
    log.info(format_args!("roll white balance {gains:?}"));
    let white = measured_roll_white(
        &mut frames,
        args.leader.is_some(),
        recipe.reconstruction.linearization,
        gains,
    )?;
    log.info(format_args!(
        "roll white {:+.2} stops ({:?}), contrast {}",
        white.stops, white.bound, white.contrast
    ));
    let clamps = clamp_manifest(&frames, &white);

    let report = MeasureRollReport {
        command: "measure-roll",
        identity: Identity::new(),
        film_base: base,
        decode: decode.expect("clap requires at least one input"),
        leader,
        frames,
        white_balance: RollWhiteBalance {
            gains,
            percentile: roll_white::PERCENTILE,
            pooled: pool.len() / 3,
        },
        reuse: RollReuse {
            flag: reuse_flag(gains, white.stops),
            recipe: RollFragment {
                roll: recipe::RollSection {
                    white_balance: Some(gains),
                    white_stops: Some(white.stops),
                },
            },
            frames: clamps,
        },
        white,
        warnings,
        elapsed_ms: elapsed_ms(started),
    };
    emit_json(
        &report,
        args.report.report,
        args.report.report_file.as_deref(),
        &log,
    )?;
    if args.strict && !report.warnings.is_empty() {
        return Err(NcError::Other(format!(
            "--strict: {} warning(s) present (see report)",
            report.warnings.len()
        )));
    }
    Ok(())
}

/// Whether this run should collect telemetry — opt-in via either flag.
fn telemetry_requested(args: &ConvertArgs) -> bool {
    args.telemetry || args.telemetry_file.is_some()
}

/// The `--telemetry-file` value as a filesystem write target, or `None` when it's
/// absent or `-` (stdout, which is not a file and needs no collision check).
fn telemetry_file_target(args: &ConvertArgs) -> Option<&Path> {
    match args.telemetry_file.as_deref() {
        Some(p) if p != "-" => Some(Path::new(p)),
        _ => None,
    }
}

/// Build this run's telemetry event — a success, or a failure from what `attempt`
/// had learned — and write it to the requested sink(s): the persistent JSONL log
/// (`--telemetry`) and/or a one-off file or stdout (`--telemetry-file`).
/// `telemetry_log` is the log path resolved once, so the guarded and the written
/// path are the same. Best-effort — every failure is warned on stderr and
/// swallowed, and nothing here enters `report.warnings`, so neither `--strict` nor
/// a telemetry fault can change the run's exit code. This is the one documented
/// deviation from the house fail-loudly rule (telemetry is non-critical
/// observability). `error` is never read for its text: only its kind and exit code
/// reach the event.
fn emit_telemetry(
    args: &ConvertArgs,
    log: &Log,
    started: Instant,
    attempt: &ConvertAttempt,
    error: Option<&NcError>,
    telemetry_log: Option<&Path>,
) {
    // A telemetry write failure warns but never fails the run. Unlike ordinary
    // warnings, these are deliberately kept out of `report.warnings` (so
    // `--strict` can't promote them), which means the report can't carry them
    // either — so they must show even under `--quiet` (the `non_finite` precedent):
    // an opted-in feature failing silently would defeat the opt-in. The
    // successful-write notices stay `log.info` (visible only under `-v`).
    let warn = |msg: String| log.warn_always(&msg);

    // A run that failed before the write-target guard has not proven its sinks safe:
    // write only if none lands on a file the run read or might have written. The
    // output is the resolved path once known, else `-o` as typed or completed with
    // any suffix; `--export-ir` the recipe's once merged, else the flag's.
    if !attempt.guarded {
        let output = attempt.output.as_deref().unwrap_or(&args.output);
        let export_ir = attempt.export_ir.as_deref().or(args
            .input_opts
            .export_ir
            .as_deref()
            .map(Path::new));
        let collision =
            telemetry_sink_collision(args, output, export_ir, telemetry_log).or_else(|| {
                let completed = [telemetry_file_target(args), telemetry_log];
                (attempt.output.is_none()
                    && completed
                        .into_iter()
                        .flatten()
                        .any(|sink| completes(sink, &args.output)))
                .then(|| format!("it may be --output ({}) completed", args.output.display()))
            });
        if let Some(msg) = collision {
            warn(format!("telemetry: no event written: {msg}"));
            return;
        }
    }
    let Some(event_id) = telemetry::EventId::random() else {
        warn("telemetry: no event written: the system random source failed".into());
        return;
    };

    let outcome = match error {
        None => telemetry::Outcome::Success,
        Some(e) => telemetry::Outcome::Failure {
            stage: attempt.failed_stage(),
            kind: if attempt.strict {
                telemetry::ErrorKind::Strict
            } else {
                telemetry::ErrorKind::of(e)
            },
            exit_code: e.exit_code() as u8,
        },
    };
    // The primary exists to be measured only once the frame committed it; before
    // that a file at the path is an earlier run's.
    let finished = attempt.finished();
    let output_bytes = finished.and(attempt.output.as_deref()).and_then(file_len);
    let event = telemetry::build_event(telemetry::EventInputs {
        outcome,
        event_id,
        // The ambient reads live here in the orchestrator; `build_event` stays a
        // pure function of its inputs (mirrors `default_log_path`/`resolve_log_path`).
        timestamp_ms: telemetry::now_unix_millis(),
        cpu_count: telemetry::cpu_count(),
        timings: telemetry::TimingInfo {
            total: finished.map_or_else(|| elapsed_ms(started), |f| f.total_ms),
            ..attempt.frame.clock.timings
        },
        image: attempt
            .frame
            .info
            .as_ref()
            .map(|info| telemetry::ImageFacts {
                info,
                input_bytes: file_len(&args.input),
                output_bytes,
            }),
        conversion: attempt.conversion.clone(),
        loss: finished.map(|f| f.loss),
        warnings: finished.map_or(attempt.warnings.len(), |f| f.warnings),
    });

    // One compact JSON object (one line for the JSONL log).
    let line = match serde_json::to_string(&event) {
        Ok(line) => line,
        Err(e) => {
            warn(format!("telemetry: could not serialize event: {e}"));
            return;
        }
    };

    if args.telemetry {
        match telemetry_log {
            Some(path) => {
                if let Err(e) = telemetry::append_jsonl(path, &line) {
                    warn(format!(
                        "telemetry: could not append to {}: {e}",
                        path.display()
                    ));
                } else {
                    log.info(format_args!("telemetry: appended to {}", path.display()));
                }
            }
            None => warn(
                "telemetry: could not locate a data dir for the log \
                 (set NC_TELEMETRY_LOG)"
                    .into(),
            ),
        }
    }

    if let Some(target) = args.telemetry_file.as_deref() {
        if target == "-" {
            // `-` = stdout. Written fail-soft with `writeln!` (not `println!`,
            // which panics on a broken pipe) so a closed stdout reader can't turn
            // the run into a panic. Note: if the JSON report is also on stdout (the
            // default), stdout then carries the report plus this one line — pair
            // `--telemetry-file -` with `--report none`/`--report-file` when a
            // parser consumes stdout.
            if let Err(e) = writeln!(std::io::stdout(), "{line}") {
                warn(format!("telemetry: could not write to stdout: {e}"));
            }
        } else if let Err(e) = telemetry::write_oneoff(Path::new(target), &line) {
            warn(format!("telemetry: could not write {target}: {e}"));
        } else {
            log.info(format_args!("telemetry: wrote {target}"));
        }
    }
}

/// Where a telemetry sink would land on a file it must not overwrite — the input,
/// the `--params` recipe the run reads, an output, or the other sink — as a message
/// naming both; `None` when every sink is clear. `--params` is guarded here rather
/// than in [`write_targets`], which would refuse `--dump-params X --params X`.
fn telemetry_sink_collision(
    args: &ConvertArgs,
    output: &Path,
    export_ir: Option<&Path>,
    telemetry_log: Option<&Path>,
) -> Option<String> {
    let is_sink = |label: &str| matches!(label, "--telemetry-file" | "the telemetry log");
    let targets = write_targets(args, output, export_ir, telemetry_log);
    let (sinks, mut others): (Vec<_>, Vec<_>) =
        targets.into_iter().partition(|(label, _)| is_sink(label));
    others.push(("the input scan", &args.input));
    if let Some(p) = &args.recipe_in {
        others.push(("--params", p));
    }
    sinks.iter().enumerate().find_map(|(i, (label, sink))| {
        let key = collision_key(sink);
        others
            .iter()
            .chain(&sinks[i + 1..])
            .find(|(_, other)| keys_collide(&key, &collision_key(other)))
            .map(|(other, _)| format!("{label} ({}) would overwrite {other}", sink.display()))
    })
}

/// Whether `path` is `stem` with a suffix appended (`out` → `out.tiff`), the way
/// [`resolve_output_path`] completes `-o`; compared as [`keys_collide`] does.
fn completes(path: &Path, stem: &Path) -> bool {
    let (path, stem) = (collision_key(path), collision_key(stem));
    let (path, stem) = (path.to_string_lossy(), stem.to_string_lossy());
    path.len() > stem.len() + 1
        && path.as_bytes()[stem.len()] == b'.'
        && path.is_char_boundary(stem.len())
        && path[..stem.len()].eq_ignore_ascii_case(&stem)
}

/// The event's conversion summary, known once the destination resolved.
fn conversion_info(recipe: &Recipe, destination: recipe::Destination) -> telemetry::ConversionInfo {
    telemetry::ConversionInfo {
        destination: match destination {
            recipe::Destination::FilmMaster => OutputSection::FilmMaster,
            recipe::Destination::Display(d) => OutputSection::Display(d.axes()),
        },
        params_hash: recipe.params_hash(),
        // `validate_convert` refuses a recipe with no film base before this is
        // built, so the fallback is unreachable; telemetry degrades rather than
        // panics.
        film_base_source: recipe
            .calibration
            .film_base
            .clone()
            .unwrap_or(FilmBaseSource::Auto),
        output_depth: primary_depth(destination),
    }
}

/// The primary image's sample depth as the destination's encoder writes it, for the
/// telemetry record. Exhaustive, so a new encoding states its own.
fn primary_depth(destination: recipe::Destination) -> &'static str {
    match destination {
        recipe::Destination::FilmMaster => "f32",
        recipe::Destination::Display(d) => match d.encoding {
            Encoding::SdrTiff | Encoding::HdrCodedTiff(_) => "u16",
            Encoding::HdrLinearTiff => "f32",
            Encoding::HdrAvif(_) => "u10",
            Encoding::GainMapJpeg => "u8",
        },
    }
}

/// Best-effort file size in bytes for the telemetry record; `None` if the file
/// can't be stat'd (never fails the run).
fn file_len(path: &Path) -> Option<u64> {
    std::fs::metadata(path).map(|m| m.len()).ok()
}

/// Milliseconds elapsed since `started`, as an `f64` for the report.
fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A recipe whose film base is **stated**.
    ///
    /// `calibration.film_base` has no default, so `validate_shared` rejects an
    /// unstated one. Tests that are not *about* the film base use this, otherwise every
    /// unrelated assertion would trip that rule instead of the one under test.
    fn base_recipe() -> Recipe {
        let mut r = Recipe::default();
        r.calibration.film_base = Some(FilmBaseSource::Auto);
        r
    }

    /// Parse a `convert` invocation (with the required input/output already set)
    /// and return its args, so merge can be tested against the real parser.
    fn parse_convert(extra: &[&str]) -> ConvertArgs {
        let mut argv = vec!["hanten", "convert", "in.tiff", "-o", "out.tiff"];
        argv.extend_from_slice(extra);
        match Cli::try_parse_from(argv).unwrap().command {
            Command::Convert(a) => a,
            _ => unreachable!("expected convert"),
        }
    }

    #[test]
    fn cli_parser_is_valid() {
        // Catches clap derive mistakes (duplicate flags, bad value parsers).
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn parse_rgb_and_region() {
        assert_eq!(parse_rgb("0.9, 0.5,0.4").unwrap(), [0.9, 0.5, 0.4]);
        assert!(parse_rgb("0.9,0.5").is_err()); // too few
        assert!(parse_rgb("a,b,c").is_err()); // not numbers
        assert_eq!(parse_region("0,1,2,3").unwrap(), [0, 1, 2, 3]);
        assert!(parse_region("0,1,2").is_err()); // too few
        assert!(parse_region("0,1,2,-3").is_err()); // negative
    }

    #[test]
    fn validate_bounds_the_measure_inset_from_either_provenance() {
        // A *value* rule, so it is in `validate` rather than `validate_convert` —
        // `roll` and every per-frame override reach only the former. Both
        // provenances hit the same check, which is why the bound has one home.
        for bad in [-0.1, 0.5, 1.0, f32::NAN, f32::INFINITY] {
            let mut cfg = base_recipe();
            cfg.measure.inset = bad;
            let err = validate_shared(&cfg, FilmBaseRemedy::Flags).unwrap_err();
            assert!(
                format!("{err}").contains("measure-inset"),
                "inset {bad} must be refused by name, got: {err}"
            );
            // Every remedy it names must be one that exists. `effective_area` never
            // sees a user-stated region — `--base-region` sets the film-base
            // source — so "state a region explicitly" was advice nothing could
            // follow. Asserting its *absence*, because both wordings name the knob.
            let err = format!("{err}");
            assert!(
                !err.contains("state a region"),
                "inset {bad}: there is no explicit measurement region to state: {err}"
            );
        }
        // Over the maximum, the remedies are lowering the fraction or not measuring
        // over the area at all.
        let mut over = base_recipe();
        over.measure.inset = 0.5;
        let err = format!(
            "{}",
            validate_shared(&over, FilmBaseRemedy::Flags).unwrap_err()
        );
        assert!(
            err.contains("lower the fraction") && !err.contains("--auto-d-max"),
            "{err}"
        );
        let mut ok = base_recipe();
        ok.measure.inset = 0.0;
        assert!(
            validate_shared(&ok, FilmBaseRemedy::Flags).is_ok(),
            "zero is legal — the stage floors it at one probe step on a measured \
             frame, which is not a usage question"
        );
        ok.measure.inset = crate::types::MAX_MEASURE_INSET;
        assert!(
            validate_shared(&ok, FilmBaseRemedy::Flags).is_ok(),
            "the bound itself is inclusive"
        );
    }

    #[test]
    fn removed_algorithm_and_simple_flags_are_migration_errors() {
        // `--algorithm` is rejected with guidance naming the replacement.
        let err = reject_removed_flags(&parse_convert(&["--algorithm", "sigmoid"])).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("recipe `reconstruction`"), "{err}");
        // The remedy is to drop the flag, not to reach for the removed curve selector.
        assert!(err.to_string().contains("Drop the flag"), "{err}");
        assert!(!err.to_string().contains("--density-curve"), "{err}");

        // `--reconstruction`, whatever its value — density is the only one left.
        for value in ["simple", "density"] {
            let err =
                reject_removed_flags(&parse_convert(&["--reconstruction", value])).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{value}");
            assert!(
                err.to_string().contains(REMOVED_SIMPLE_RECONSTRUCTION),
                "{value}: {err}"
            );
        }

        // The removed simple controls are rejected, naming what replaced each.
        for flags in [
            ["--invert-white-balance", "1.1,1.0,0.9"].as_slice(),
            ["--clip-low", "0.1"].as_slice(),
            ["--clip-high", "0.9"].as_slice(),
        ] {
            let err = reject_removed_flags(&parse_convert(flags)).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{flags:?}");
            assert!(err.to_string().contains("was removed"), "{flags:?}: {err}");
        }

        // A clean invocation passes.
        assert!(reject_removed_flags(&parse_convert(&[])).is_ok());
    }

    /// Whether `convert` has a visible flag spelled `flag` — a remedy a message names
    /// must be one the user can type.
    fn is_convert_flag(flag: &str) -> bool {
        use clap::CommandFactory;
        Cli::command()
            .find_subcommand("convert")
            .unwrap()
            .get_arguments()
            .any(|a| !a.is_hide_set() && a.get_long() == Some(flag.trim_start_matches("--")))
    }

    /// Every flag the sigmoid owned is a migration error naming a remedy that exists.
    #[test]
    fn the_sigmoid_flags_are_migration_errors() {
        for (flags, remedy) in [
            (["--sigmoid-contrast", "2"].as_slice(), "--contrast"),
            (
                ["--sigmoid-toe", "0.2"].as_slice(),
                "--display-tone-headroom",
            ),
            (
                ["--sigmoid-shoulder", "0"].as_slice(),
                "--display-tone-headroom",
            ),
            (
                ["--sigmoid-mid-fraction", "0.5"].as_slice(),
                "--anchor-mid-offset",
            ),
            (
                ["--sigmoid-white-at-d-max"].as_slice(),
                "--anchor-mid-offset",
            ),
        ] {
            let err = reject_removed_flags(&parse_convert(flags)).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{flags:?}");
            let msg = err.to_string();
            assert!(msg.contains("removed with the sigmoid"), "{flags:?}: {msg}");
            assert!(msg.contains(remedy), "{flags:?}: {msg}");
            assert!(is_convert_flag(remedy), "{remedy} is not a flag");
        }
    }

    /// Every flag retired with the `characteristic` curve is a migration error at every
    /// value, before anything coarser can refuse it — and no remedy names a flag that no
    /// longer exists.
    #[test]
    fn the_characteristic_flags_are_migration_errors() {
        let cases: &[(&[&str], &str)] = &[
            (
                &["--density-curve", "characteristic"],
                "--density-curve was removed",
            ),
            (
                &["--density-curve", "exponential"],
                "--density-curve was removed",
            ),
            (
                &["--density-curve", "sigmoid"],
                "--density-curve was removed",
            ),
            (&["--density-curve"], "--density-curve was removed"),
            (&["--film-stock", "portra-400"], "--film-stock was removed"),
            (&["--film-stock"], "--film-stock was removed"),
            (
                &["--preset", "characteristic-generic"],
                "--preset was removed",
            ),
            (&["--preset", "sigmoid-knees"], "--preset was removed"),
            (&["--preset"], "--preset was removed"),
        ];
        for (flags, want) in cases {
            let err = reject_removed_flags(&parse_convert(flags))
                .unwrap_err()
                .to_string();
            assert!(err.contains(want), "{flags:?}: {err}");
            assert!(err.contains("rop the flag"), "{flags:?}: {err}");
            for gone in ["--density-curve exponential", "Pass --", "--film-stock <"] {
                assert!(!err.contains(gone), "{flags:?} advises `{gone}`: {err}");
            }
        }
        // `characteristic` and `sigmoid` get their own history; the plain identity does not.
        let why = |v: &str| {
            reject_removed_flags(&parse_convert(&["--density-curve", v]))
                .unwrap_err()
                .to_string()
        };
        assert!(why("characteristic").contains("reference-snapshot"));
        assert!(why("sigmoid").contains("--display-tone-headroom"));
        // Every flag the `--preset` remedy names parses.
        for named in [
            "--exposure",
            "--contrast",
            "--channel-grade",
            "--highlight-desaturation",
            "--params",
        ] {
            assert!(is_convert_flag(named), "{named} is not a flag");
        }
    }

    #[test]
    fn the_reference_density_and_retired_anchor_flags_are_migration_errors() {
        // Every one is refused with a remedy naming the one placement left — and the
        // named remedy must itself be accepted, or the message sends the user in a circle.
        for flags in [
            vec!["--d-max", "1.5"],
            vec!["--fixed-d-max"],
            vec!["--auto-d-max"],
            vec!["--no-d-max"],
            vec!["--anchor-white-at-reference"],
            vec!["--anchor-mid-fraction", "0.5"],
            vec!["--anchor-black-floor", "0.005"],
        ] {
            let err = reject_removed_flags(&parse_convert(&flags)).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{flags:?}");
            let msg = err.to_string();
            assert!(
                msg.contains(flags[0]) && msg.contains("was removed"),
                "{msg}"
            );
            assert!(msg.contains("--anchor-mid-offset"), "{msg}");
            assert!(msg.contains("Drop the flag"), "{msg}");
            assert!(!msg.contains("characteristic"), "{msg}");
        }
        // Every old spelling reaches the migration message, not clap's generic error:
        // no value, and a negative value that would otherwise read as a flag.
        for argv in [
            vec!["--d-max"],
            vec!["--d-max", "-1.5"],
            vec!["--anchor-mid-fraction", "-0.5"],
            vec!["--anchor-black-floor"],
        ] {
            let err = reject_removed_flags(&parse_convert(&argv)).unwrap_err();
            assert!(err.to_string().contains("was removed"), "{argv:?}: {err}");
        }
        let remedy = parse_convert(&["--anchor-mid-offset", "0.5"]);
        reject_removed_flags(&remedy).unwrap();
        let r = recipe::merge(base_recipe(), &remedy);
        assert_eq!(
            r.reconstruction.anchor,
            crate::algo::fixed::AnchorRule::MidAboveBase(0.5)
        );
    }

    /// The print controls and output selectors retired with the chain that read them
    /// (`nf-core/default-flip`): each spelling names what replaced it — a flag the user
    /// can type — and the retirement, never parsing as an unknown argument.
    #[test]
    fn the_removed_chains_print_and_output_flags_are_migration_errors() {
        for (argv, names, replacement) in [
            (
                vec!["--print-exposure", "1"],
                "--print-exposure was removed",
                Some("--exposure"),
            ),
            (
                vec!["--print-exposure", "-1"],
                "--print-exposure was removed",
                Some("--exposure"),
            ),
            (
                vec!["--black-point", "0.01"],
                "--black-point was removed",
                Some("--display-black"),
            ),
            (
                vec!["--auto-wb", "percentile"],
                "--auto-wb was removed",
                Some("--roll-white-balance"),
            ),
            (
                vec!["--auto-wb"],
                "--auto-wb was removed",
                Some("--roll-white-balance"),
            ),
            (
                vec!["--linear-range", "0,1"],
                "--linear-range was removed",
                None,
            ),
            (
                vec!["--output-preset", "display-p3"],
                "--output-preset was removed",
                None,
            ),
            (
                vec!["--output-preset", "hdr-pq"],
                "--output-preset was removed",
                Some("--container"),
            ),
            (
                vec!["--output-preset", "gain-map-hdr"],
                "--output-preset was removed",
                Some("--range"),
            ),
            (
                vec!["--output-preset", "film-master"],
                "--output-preset was removed",
                Some("--film-master"),
            ),
            (
                vec!["--output-preset", "compatibility"],
                "--output-preset was removed",
                Some("--gamut"),
            ),
            // Retired before the chain was, so no counterpart: the default is named.
            (
                vec!["--output-preset", "legacy"],
                "--output-preset was removed",
                Some("--film-master"),
            ),
            (
                vec!["--out-depth", "f32"],
                "--out-depth was removed",
                Some("--transfer"),
            ),
            (
                vec!["--output-hdr"],
                "--output-hdr was removed",
                Some("--film-master"),
            ),
            (
                vec!["--output-profile", "prophoto"],
                "--output-profile was removed",
                Some("--gamut"),
            ),
            (vec!["--bigtiff", "on"], "--bigtiff was removed", None),
            (vec!["--clip-low", "0.1"], "--clip-low was removed", None),
            (
                vec!["--invert-white-balance", "1,1,1"],
                "--invert-white-balance was removed",
                Some("--white-balance"),
            ),
        ] {
            let err = reject_removed_flags(&parse_convert(&argv)).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{argv:?}");
            let msg = err.to_string();
            assert!(msg.contains(names), "{argv:?}: {msg}");
            if let Some(flag) = replacement {
                assert!(msg.contains(flag), "{argv:?}: {msg}");
                assert!(is_convert_flag(flag), "{flag} is not a flag");
            }
            // The removed chain's vocabulary is not advice any more.
            assert!(!msg.contains("--new-flow"), "{argv:?}: {msg}");
            assert!(
                !msg.contains("  "),
                "{argv:?}: a sentence is missing: {msg}"
            );
        }
        let msg = reject_removed_flags(&parse_convert(&["--output-preset", "legacy"]))
            .unwrap_err()
            .to_string();
        assert!(msg.contains("Drop the flag, or state the axes"), "{msg}");
        // `display-p3` names its gamut rather than "the default": under `--rendering
        // direct` the default is the HDR float TIFF.
        let msg = reject_removed_flags(&parse_convert(&["--output-preset", "display-p3"]))
            .unwrap_err()
            .to_string();
        assert!(msg.contains("pass --gamut display-p3"), "{msg}");
        assert!(reject_removed_flags(&parse_convert(&[])).is_ok());
    }

    #[test]
    fn the_preset_counterparts_resolve_the_same_under_every_rendering() {
        // The refusal runs before the recipe says which rendering applies, so each named
        // set must write one destination under either — and be written today.
        use crate::destination::{Defaults, resolve};
        for &(name, flags) in PRESET_COUNTERPARTS {
            let argv: Vec<&str> = flags.split_whitespace().collect();
            let r = crate::recipe::merge(Recipe::default(), &parse_convert(&argv));
            let OutputSection::Display(axes) = r.output else {
                assert_eq!(flags, "--film-master", "{name}");
                continue;
            };
            let rows: Vec<_> = [Defaults::STANDARD, crate::rendering::DIRECT.axes]
                .iter()
                .map(|d| resolve(&axes, d).unwrap_or_else(|f| panic!("{name}: {f:?}")))
                .collect();
            assert_eq!(rows[0], rows[1], "{name}: `{flags}` differs by rendering");
        }
    }

    #[test]
    fn the_film_master_counterpart_names_the_way_out_of_direct() {
        let text = |extra: &[&str]| {
            reject_removed_flags(&parse_convert(
                &[&["--output-preset", "film-master"][..], extra].concat(),
            ))
            .unwrap_err()
            .to_string()
        };
        assert!(
            text(&[]).contains("For `film-master`, pass --film-master."),
            "{}",
            text(&[])
        );
        let direct = text(&["--rendering", "direct"]);
        assert!(
            direct.contains("--rendering default in place of --rendering direct"),
            "{direct}"
        );
        let recipe = text(&["--params", "r.json"]);
        assert!(
            recipe.contains("if the recipe states `rendering`: \"direct\""),
            "{recipe}"
        );
    }

    /// Every hidden `convert` flag is a removed one, and each reaches a migration error
    /// — a hidden flag that parsed and did nothing would be the accepted-and-ignored
    /// defect, invisible in `--help`. Driven off clap, so a new hidden flag without a
    /// refusal reds.
    #[test]
    fn every_hidden_convert_flag_is_refused() {
        use clap::CommandFactory;
        let cli = Cli::command();
        let convert = cli.find_subcommand("convert").unwrap();
        let mut hidden = 0;
        for arg in convert.get_arguments().filter(|a| a.is_hide_set()) {
            let Some(long) = arg.get_long() else { continue };
            let flag = format!("--{long}");
            // A value that every hidden flag's parser accepts — they all take a string,
            // or none.
            let takes_value = arg.get_action().takes_values();
            let argv: Vec<&str> = if takes_value {
                vec![flag.as_str(), "1"]
            } else {
                vec![flag.as_str()]
            };
            // The two gates `run_convert` opens with, in its order.
            let args = parse_convert(&argv);
            let err = reject_deprecated_input_flags(&args.input_opts)
                .and_then(|()| reject_removed_flags(&args))
                .err()
                .unwrap_or_else(|| panic!("{flag} parsed and was not refused"));
            assert_eq!(err.exit_code(), 2, "{flag}");
            hidden += 1;
        }
        assert!(
            hidden > 30,
            "only {hidden} hidden flags found — the walk is broken"
        );
    }

    #[test]
    fn new_flow_is_a_removed_flag_on_every_command_that_took_it() {
        for argv in [
            vec!["hanten", "convert", "in.tif", "-o", "out", "--new-flow"],
            vec!["hanten", "roll", "in.tif", "-o", "dir", "--new-flow"],
            vec!["hanten", "params", "--new-flow"],
        ] {
            let new_flow = match Cli::try_parse_from(&argv).unwrap().command {
                Command::Convert(a) => a.new_flow,
                Command::Roll(a) => a.new_flow,
                Command::Params(a) => a.new_flow,
                _ => unreachable!(),
            };
            let err = reject_new_flow(new_flow).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{argv:?}");
            let msg = err.to_string();
            assert!(msg.contains("--new-flow was removed"), "{msg}");
            assert!(msg.contains("Drop the flag"), "{msg}");
        }
        reject_new_flow(false).unwrap();
        // `convert` refuses it before anything else, so a removed flag beside it is not
        // diagnosed first.
        let err = reject_removed_flags(&parse_convert(&["--new-flow", "--print-exposure", "1"]))
            .unwrap_err();
        assert!(err.to_string().contains("--new-flow was removed"), "{err}");
    }

    #[test]
    fn the_retired_display_tone_flags_are_refused_with_a_migration_error() {
        // Every remedy names only the headroom flag, which exists.
        for (value, remedy) in [
            ("shoulder", "has no replacement"),
            ("none", "--display-tone-headroom 0"),
            ("reinhard", "always"),
            ("sigmoid", "was never a tone"),
        ] {
            let msg = reject_removed_flags(&parse_convert(&["--display-tone", value]))
                .unwrap_err()
                .to_string();
            assert!(msg.contains("--display-tone was removed"), "{value}: {msg}");
            assert!(msg.contains(remedy), "{value}: {msg}");
            assert!(msg.contains("fit_range.headroom_stops"), "{value}: {msg}");
        }
        let msg = reject_removed_flags(&parse_convert(&["--highlight-compress", "0"]))
            .unwrap_err()
            .to_string();
        assert!(msg.contains("--highlight-compress was removed"), "{msg}");
        assert!(msg.contains("--display-tone-headroom"), "{msg}");
        // Falsifiable: the headroom flag itself is not a removed flag.
        reject_removed_flags(&parse_convert(&["--display-tone-headroom", "4"])).unwrap();
    }

    /// The output target a `convert` command line's destination flags resolve.
    fn target_of(extra: &[&str]) -> OutputTarget {
        let args = parse_convert(extra);
        let r = recipe::merge(base_recipe(), &args);
        OutputTarget::resolve(&r, KnobNames::FlagAndKey, args.destination.film_master).unwrap()
    }

    /// The gain-map JPEG destination (`--range hdr`).
    fn jpeg() -> OutputTarget {
        target_of(&["--range", "hdr"])
    }

    #[test]
    fn a_missing_extension_is_completed_from_the_resolved_container() {
        // An extensionless path is completed from whatever container the destination
        // resolved — the default's TIFF, the gain map's JPEG, the AVIF's own.
        for (extra, want) in [
            (&[][..], "positive.tiff"),
            (&["--film-master"][..], "positive.tiff"),
            (&["--range", "hdr"][..], "positive.jpg"),
            (
                &["--transfer", "pq", "--container", "avif"][..],
                "positive.avif",
            ),
        ] {
            let mut args = parse_convert(extra);
            args.output = PathBuf::from("positive");
            validate_convert(&recipe::merge(base_recipe(), &args), &args).unwrap();
            assert_eq!(
                resolve_output_path(&args.output, target_of(extra), SuffixContext::Convert)
                    .unwrap(),
                PathBuf::from(want),
                "{extra:?}"
            );
        }
    }

    #[test]
    fn a_mismatched_suffix_names_the_destination_and_the_way_out() {
        // A suffix nc's containers know but this destination does not write is a usage
        // error — completion never rescues a *stated* one. The message names the
        // destination, the suffixes it writes, that dropping the suffix works, and a
        // destination that writes the suffix given.
        let mut args = parse_convert(&[]);
        args.output = PathBuf::from("positive.jpg");
        let err = validate_convert(&recipe::merge(base_recipe(), &args), &args)
            .unwrap_err()
            .to_string();
        assert!(err.contains(".tif or .tiff"), "{err}");
        assert!(err.contains("drop .jpg"), "{err}");
        assert!(err.contains("--range hdr"), "{err}");
        // A removed chain's vocabulary is not advice.
        assert!(!err.contains("--output-preset"), "{err}");
        // The converse: the gain map refuses a TIFF path and names itself.
        let mut hdr = parse_convert(&["--range", "hdr"]);
        hdr.output = PathBuf::from("positive.tiff");
        let err = validate_convert(&recipe::merge(base_recipe(), &hdr), &hdr)
            .unwrap_err()
            .to_string();
        assert!(err.contains(".jpg or .jpeg"), "{err}");
        // Control: the matching suffix is accepted.
        hdr.output = PathBuf::from("positive.jpg");
        validate_convert(&recipe::merge(base_recipe(), &hdr), &hdr).unwrap();
    }

    #[test]
    fn a_dotted_stem_is_completed_and_a_known_spelling_is_judged() {
        // The rule that tells a suffix from a stem: a dot-segment is a container
        // request only when *some* container accepts that spelling. `-o out.v2` and
        // `-o roll-1.2` are stems and keep their dot; `.tif` is a spelling nc knows,
        // so under a JPEG destination it is the mismatch error rather than a stem.
        let jpeg = jpeg();
        for (given, want) in [
            ("out.v2", "out.v2.jpg"),
            ("roll-1.2", "roll-1.2.jpg"),
            ("scan.2026-09-22", "scan.2026-09-22.jpg"),
            // Degenerate but consistent: an empty dot-segment is not a spelling nc
            // knows, and nothing the user typed is ever dropped.
            ("out.", "out..jpg"),
            // A leading dot with nothing after it is a stem, not an extension.
            (".hidden", ".hidden.jpg"),
            ("dir/out", "dir/out.jpg"),
        ] {
            assert_eq!(
                resolve_output_path(Path::new(given), jpeg, SuffixContext::Convert).unwrap(),
                PathBuf::from(want),
                "{given}"
            );
        }
        // The falsifiable half: a *known* spelling is judged, never appended to.
        let err = resolve_output_path(Path::new("out.tif"), jpeg, SuffixContext::Convert)
            .unwrap_err()
            .to_string();
        assert!(err.contains(".jpg"), "{err}");
        // …and one this container accepts survives byte for byte, spelling and case
        // included.
        for keep in ["out.jpeg", "out.JPG", "out.v2.jpeg"] {
            assert_eq!(
                resolve_output_path(Path::new(keep), jpeg, SuffixContext::Convert).unwrap(),
                PathBuf::from(keep),
                "{keep}"
            );
        }
        // A path whose last meaningful component is a *directory*. `file_name()`
        // normalises the trailing separator — and an interior `.` — away, so
        // appending would quietly write a sibling (`dir/` and `dir/.` both →
        // `dir.jpg`) instead of the file inside it the user meant. Refused, not
        // completed.
        for bad in ["dir/", "dir//", "dir/./", "dir/.", "a/b/.", "./out/"] {
            let err = resolve_output_path(Path::new(bad), jpeg, SuffixContext::Convert)
                .unwrap_err()
                .to_string();
            assert!(err.contains("names a directory"), "{bad}: {err}");
            assert!(err.contains(bad), "{bad} must be named back: {err}");
        }
        // The two shapes stay distinct: a path `file_name()` itself declines keeps
        // the "names no file" wording, including `dir/..`, which the directory arm's
        // string test would never see.
        for bad in [".", "..", "/", "dir/.."] {
            let err = resolve_output_path(Path::new(bad), jpeg, SuffixContext::Convert)
                .unwrap_err()
                .to_string();
            assert!(err.contains("names no file"), "{bad}: {err}");
            assert!(!err.contains("names a directory"), "{bad}: {err}");
        }
        // The falsifiable controls. Same paths *without* the trailing directory
        // component still complete — including a **leading** `./`, which is not a
        // trailing one — and `out.` is the documented degenerate stem, which pins
        // that the rule is "separator then `.`" and not merely `ends_with('.')`.
        for (stem, want) in [
            ("dir", "dir.jpg"),
            ("a/b", "a/b.jpg"),
            ("./out", "./out.jpg"),
            ("out.", "out..jpg"),
            (".hidden", ".hidden.jpg"),
        ] {
            assert_eq!(
                resolve_output_path(Path::new(stem), jpeg, SuffixContext::Convert).unwrap(),
                PathBuf::from(want),
                "{stem}"
            );
        }
    }

    #[test]
    fn a_roll_frames_unappendable_output_is_attributed_and_gets_a_remedy_it_can_reach() {
        // A remedy must be one the *reader's* command line can reach. A roll user
        // meeting this is running `hanten roll --out-dir` at that moment, so telling
        // them to use it is advice they are already taking — and with 40 manifest
        // entries, a message that does not say which frame is unactionable.
        let jpeg = jpeg();
        let frame = Path::new("/scans/f12.tif");
        for bad in ["/out/.", "/out/", "/out/sub/"] {
            let err = resolve_output_path(Path::new(bad), jpeg, SuffixContext::RollFrame(frame))
                .unwrap_err()
                .to_string();
            assert!(err.contains("frame /scans/f12.tif"), "{bad}: {err}");
            assert!(err.contains("`output`"), "{bad}: {err}");
            // The losing wording must be *absent*, not merely out-ranked — asserting
            // only that the right words appear cannot tell the two arms apart.
            assert!(!err.contains("hanten roll --out-dir"), "{bad}: {err}");
        }
        // …and `convert` keeps the wording that fits *its* reader, which is what
        // makes the assertion above falsifiable rather than vacuous.
        let err = resolve_output_path(Path::new("/out/."), jpeg, SuffixContext::Convert)
            .unwrap_err()
            .to_string();
        assert!(err.contains("hanten roll --out-dir"), "{err}");
        assert!(!err.contains("frame "), "{err}");
    }

    #[test]
    fn every_container_spelling_is_a_known_suffix_and_the_derived_one_is_accepted() {
        // `given_container` is what separates a mismatch from a stem.
        for &container in Container::ALL {
            for spelling in container.accepted() {
                let path = format!("out.{spelling}");
                assert_eq!(given_container(Path::new(&path)), Some(container), "{path}");
                // Case-insensitively too, which is how the accept check reads it.
                let upper = format!("out.{}", spelling.to_uppercase());
                assert_eq!(given_container(Path::new(&upper)), Some(container));
            }
            // Every container's *supplied* spelling is one it accepts — the invariant
            // that lets a completed or derived path skip re-checking.
            assert!(
                accepts(container, OsStr::new(container.canonical())),
                "{container:?}"
            );
        }
        // Falsifiable: a format nc does not write is a stem, not a suffix.
        for foreign in ["png", "webp", "exr", "v2"] {
            assert_eq!(given_container(Path::new(&format!("out.{foreign}"))), None);
        }
        // The spellings themselves, pinned so they cannot drift silently: `tiff` over
        // `tif`, `jpg` over `jpeg` — the names roll has always derived.
        assert_eq!(Container::Tiff.canonical(), "tiff");
        assert_eq!(Container::Jpeg.canonical(), "jpg");
        assert_eq!(Container::Avif.canonical(), "avif");
    }

    #[test]
    fn help_names_no_removed_flag() {
        use clap::CommandFactory;
        let mut cmd = Cli::command();
        let help = cmd
            .find_subcommand_mut("convert")
            .expect("convert subcommand")
            .render_long_help()
            .to_string();
        assert!(help.contains("--film-master"), "{help}");
        assert!(help.contains("--range"), "{help}");
        for gone in [
            "--output-preset",
            "--new-flow",
            "--print-exposure",
            "--linear-range",
            "--auto-wb",
            "scene-master",
        ] {
            assert!(!help.contains(gone), "`{gone}` is in --help:\n{help}");
        }
    }

    #[test]
    fn params_default_is_parseable_json_but_no_longer_runnable() {
        // The subject is the exact document `hanten params` prints — `run_params`
        // serializes `Recipe::default()` — so this must stay on the real default, not on
        // a film-base-stated stand-in.
        let json = serde_json::to_string_pretty(&Recipe::default()).unwrap();
        let back: Recipe = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Recipe::default());

        // ...and the scaffold is deliberately NOT runnable as printed: it states no
        // film base, so `validate_shared` rejects it — `hanten params` emits a template to
        // edit, not a recipe to run.
        let msg = match validate_shared(&back, FilmBaseRemedy::Flags) {
            Err(NcError::Usage(m)) => m,
            other => panic!(
                "the printed default scaffold must be rejected until a film base is \
                 stated, got {other:?}"
            ),
        };
        assert!(msg.contains("calibration.film_base"), "{msg}");

        // Falsifiable control: the same document with a base stated does validate,
        // so the rejection above is about the film base and nothing else.
        let mut runnable = back.clone();
        runnable.calibration.film_base = Some(FilmBaseSource::Auto);
        validate_shared(&runnable, FilmBaseRemedy::Flags).unwrap();
    }

    #[test]
    fn validate_requires_a_stated_film_base() {
        // `calibration.film_base` has no default: `Dmin` is the divisor of the density
        // conversion, so falling into auto-detection by omission decided the most
        // consequential parameter for the user. All three stated forms are fine —
        // including `auto`, which is what this used to default to. The rule is
        // that the choice is *made*, not that it is explicit.
        let unstated = Recipe::default();
        assert_eq!(
            unstated.calibration.film_base, None,
            "there must be no default"
        );
        let msg = match validate_shared(&unstated, FilmBaseRemedy::Flags) {
            Err(NcError::Usage(m)) => m,
            other => panic!("an unstated film base must be a usage error, got {other:?}"),
        };
        // The message has to be actionable: name every way out, since a user who
        // hit this has no idea which of the three they wanted.
        for expected in [
            "--film-base",
            "--base-region",
            "--auto-base",
            "calibration.film_base",
        ] {
            assert!(msg.contains(expected), "{expected} missing from: {msg}");
        }

        for stated in [
            FilmBaseSource::Auto,
            FilmBaseSource::Region([0, 0, 100, 40]),
            FilmBaseSource::Explicit([0.9, 0.55, 0.42]),
        ] {
            let mut cfg = Recipe::default();
            cfg.calibration.film_base = Some(stated.clone());
            validate_shared(&cfg, FilmBaseRemedy::Flags)
                .unwrap_or_else(|e| panic!("a stated {stated:?} base must be accepted: {e}"));
        }
    }

    #[test]
    fn roll_is_told_about_the_shared_recipe_rather_than_flags_it_rejects() {
        // Same rule, different remedy: `RollArgs` flattens only `MemoryArgs` /
        // `ReportArgs`, so every film-base flag exits 2 on `roll`. Naming them
        // would be advice the user cannot follow.
        let unstated = Recipe::default();
        let msg = match validate_shared(&unstated, FilmBaseRemedy::SharedRecipe) {
            Err(NcError::Usage(m)) => m,
            other => panic!("an unstated film base must be a usage error, got {other:?}"),
        };
        assert!(msg.contains("--params"), "{msg}");
        assert!(msg.contains("calibration.film_base"), "{msg}");
        for absent in ["--auto-base", "--film-base"] {
            assert!(!msg.contains(absent), "{absent} must not be offered: {msg}");
        }
        // `--base-region` *does* appear — but only inside the `hanten estimate`
        // invocation the message recommends, which is a different command and does
        // accept it. What must never appear is a `roll` flag.
        assert!(
            msg.matches("--base-region")
                .count()
                .eq(&msg.matches("hanten estimate --base-region").count()),
            "--base-region may only appear as an argument of `hanten estimate`: {msg}"
        );
        // The *requirement* is remedy-independent — only the wording moves.
        let mut stated = Recipe::default();
        stated.calibration.film_base = Some(FilmBaseSource::Auto);
        validate_shared(&stated, FilmBaseRemedy::SharedRecipe).unwrap();
        // And `convert_frame`'s totality guard shares the same two spellings, so
        // the unreachable restatement cannot drift from the gate's.
        assert_eq!(
            missing_film_base_message(FilmBaseRemedy::for_command("roll")),
            missing_film_base_message(FilmBaseRemedy::SharedRecipe)
        );
        assert_eq!(
            missing_film_base_message(FilmBaseRemedy::for_command("convert")),
            missing_film_base_message(FilmBaseRemedy::Flags)
        );
    }

    #[test]
    fn the_missing_base_rule_never_pre_empts_a_more_specific_one() {
        // `validate_shared`'s documented principle is value rules first: a recipe that
        // both carries a bad value and states no base must be told about the value,
        // which names the thing the user actually typed.
        // Placing the `None` arm first (its original position) reversed that for
        // every rule in the function.
        let mut contradictory = Recipe::default();
        contradictory.measure.inset = 0.9;
        assert_eq!(contradictory.calibration.film_base, None);
        let msg = validate_shared(&contradictory, FilmBaseRemedy::Flags)
            .unwrap_err()
            .to_string();
        assert!(
            msg.contains("measure-inset"),
            "the bad value must be diagnosed ahead of the missing base: {msg}"
        );

        // Falsifiable control: with the bad value removed, the same recipe does report
        // the missing base — so the assertion above is about ordering, not about the
        // missing-base rule having stopped working.
        let mut only_unstated = Recipe::default();
        assert!(
            validate_shared(&only_unstated, FilmBaseRemedy::Flags)
                .unwrap_err()
                .to_string()
                .contains("no film base selected")
        );
        only_unstated.calibration.film_base = Some(FilmBaseSource::Auto);
        validate_shared(&only_unstated, FilmBaseRemedy::Flags).unwrap();
    }

    #[test]
    fn auto_base_flag_states_the_source_rather_than_relying_on_a_default() {
        // The flag is the migration path for anyone who *wanted* detection: it
        // resolves to exactly the source that used to be implicit.
        let cfg = recipe::merge(Recipe::default(), &parse_convert(&["--auto-base"]));
        assert_eq!(cfg.calibration.film_base, Some(FilmBaseSource::Auto));
        validate_shared(&cfg, FilmBaseRemedy::Flags).unwrap();
    }

    #[test]
    fn validate_rejects_recipe_smuggled_bad_values() {
        // A recipe can carry values the CLI value-parsers would have rejected,
        // so validate is the only guard for these once they're in the config.
        let mut cfg = base_recipe();
        cfg.calibration.film_base = Some(FilmBaseSource::Explicit([0.9, 0.0, 0.4])); // zero transmission
        assert!(matches!(
            validate_shared(&cfg, FilmBaseRemedy::Flags),
            Err(NcError::Usage(_))
        ));

        let mut cfg = base_recipe();
        cfg.calibration.film_base = Some(FilmBaseSource::Explicit([0.9, 90.0, 0.4])); // "90" typo for "0.90"
        assert!(matches!(
            validate_shared(&cfg, FilmBaseRemedy::Flags),
            Err(NcError::Usage(_))
        ));
        let mut cfg = base_recipe();
        cfg.calibration.film_base = Some(FilmBaseSource::Explicit([1.0, 1.0, 1.0])); // 1.0 exactly is valid
        validate_shared(&cfg, FilmBaseRemedy::Flags).unwrap();

        let mut cfg = base_recipe();
        cfg.calibration.film_base = Some(FilmBaseSource::Region([0, 0, 0, 0])); // zero-area region
        assert!(matches!(
            validate_shared(&cfg, FilmBaseRemedy::Flags),
            Err(NcError::Usage(_))
        ));
    }

    #[test]
    fn export_ir_and_seed_parse_into_the_right_homes() {
        // `--export-ir` is an input/decode key (design-spec §9), not output.
        let cfg = recipe::merge(base_recipe(), &parse_convert(&["--export-ir", "ir.tiff"]));
        assert_eq!(cfg.input.export_ir.as_deref(), Some("ir.tiff"));

        // The reserved `--seed` flag parses rather than being rejected by clap.
        let args = parse_convert(&["--seed", "42"]);
        assert_eq!(args.seed, Some(42));
    }

    #[test]
    fn merge_keeps_recipe_source_until_a_flag_replaces_it() {
        // No flag → the recipe's mutually-exclusive choice survives.
        let mut recipe = base_recipe();
        recipe.calibration.film_base = Some(FilmBaseSource::Explicit([0.9, 0.5, 0.4]));
        let cfg = recipe::merge(recipe.clone(), &parse_convert(&[]));
        assert_eq!(
            cfg.calibration.film_base,
            Some(FilmBaseSource::Explicit([0.9, 0.5, 0.4]))
        );

        // A flag replaces the whole source — no field is left behind to win on
        // precedence (the #5/#6 fix). `--base-region` beats a recipe explicit base.
        let cfg = recipe::merge(recipe, &parse_convert(&["--base-region", "0,0,100,40"]));
        assert_eq!(
            cfg.calibration.film_base,
            Some(FilmBaseSource::Region([0, 0, 100, 40]))
        );
    }

    #[test]
    fn input_axes_merge_independently_and_flags_win() {
        // transfer and meaning are independent axes: a flag on one axis replaces
        // that axis and leaves the other at the recipe value (flags win per axis).
        let mut recipe = base_recipe();
        recipe.input.transfer = TransferAssertion::Auto;
        recipe.input.meaning = MeaningAssertion::ScannerDevice;

        // No flags → both recipe values survive.
        let cfg = recipe::merge(recipe.clone(), &parse_convert(&[]));
        assert_eq!(cfg.input.transfer, TransferAssertion::Auto);
        assert_eq!(cfg.input.meaning, MeaningAssertion::ScannerDevice);

        // `--input-transfer` replaces only the transfer axis.
        let cfg = recipe::merge(
            recipe.clone(),
            &parse_convert(&["--input-transfer", "linear"]),
        );
        assert_eq!(cfg.input.transfer, TransferAssertion::Linear);
        assert_eq!(cfg.input.meaning, MeaningAssertion::ScannerDevice);

        // `--input-meaning` replaces only the meaning axis (over a recipe value).
        let cfg = recipe::merge(recipe, &parse_convert(&["--input-meaning", "colorimetric"]));
        assert_eq!(cfg.input.transfer, TransferAssertion::Auto);
        assert_eq!(cfg.input.meaning, MeaningAssertion::Colorimetric);
    }

    #[test]
    fn merge_film_type_flag_overrides_recipe_else_keeps_recipe() {
        // `--film-type` maps to `input.film_type`; the flag replaces a recipe
        // value, and its absence never clobbers one (a forgotten merge arm would
        // silently make the flag a no-op — the four-spot knob rule).
        let mut recipe = base_recipe();
        recipe.input.film_type = FilmType::Silver;

        // No flag → the recipe value survives.
        let cfg = recipe::merge(recipe.clone(), &parse_convert(&[]));
        assert_eq!(cfg.input.film_type, FilmType::Silver);

        // The flag wins over the recipe.
        let cfg = recipe::merge(recipe, &parse_convert(&["--film-type", "chromogenic"]));
        assert_eq!(cfg.input.film_type, FilmType::Chromogenic);

        // Over the default recipe, the flag sets the declared type.
        let cfg = recipe::merge(
            base_recipe(),
            &parse_convert(&["--film-type", "chromogenic"]),
        );
        assert_eq!(cfg.input.film_type, FilmType::Chromogenic);
        // ...and the untouched default is `unknown` (the safe off state).
        let cfg = recipe::merge(base_recipe(), &parse_convert(&[]));
        assert_eq!(cfg.input.film_type, FilmType::Unknown);
    }

    #[test]
    fn deprecated_assume_linear_is_a_migration_error() {
        // The old combined assertion must never silently assert both axes — it is a
        // loud usage error (exit 2) pointing at the two independent flags.
        let args = parse_convert(&["--assume-linear"]);
        let err = reject_deprecated_input_flags(&args.input_opts).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("--input-transfer"));
    }

    #[test]
    fn input_profile_stays_rejected_for_convert() {
        // `--input-profile` is reserved (deferred experiment) — rejected loudly
        // (exit 4) rather than silently ignored.
        let args = parse_convert(&["--input-profile", "scanner.icc"]);
        let err = reject_deprecated_input_flags(&args.input_opts).unwrap_err();
        assert_eq!(err.exit_code(), 4);
    }

    #[test]
    fn mutually_exclusive_source_flags_are_rejected() {
        // clap must reject conflicting source flags rather than silently picking one.
        assert!(
            Cli::try_parse_from([
                "hanten",
                "convert",
                "i",
                "-o",
                "o",
                "--auto-base",
                "--film-base",
                "0.9,0.5,0.4"
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "hanten",
                "convert",
                "i",
                "-o",
                "o",
                "--base-region",
                "0,0,1,1",
                "--film-base",
                "0.9,0.5,0.4"
            ])
            .is_err()
        );
    }

    #[test]
    fn estimate_grid_conflicts_with_explicit_and_auto_base() {
        // Grid replaces sampling/detection, so an explicit base or auto-base
        // alongside it is contradictory — clap must reject, not silently pick.
        for bad in [
            ["--grid", "--film-base", "0.9,0.5,0.4"].as_slice(),
            ["--grid", "--auto-base"].as_slice(),
        ] {
            let mut argv = vec!["hanten", "estimate", "in.tiff"];
            argv.extend_from_slice(bad);
            assert!(
                Cli::try_parse_from(argv).is_err(),
                "{bad:?} should conflict"
            );
        }
        // `--grid` with `--base-region` is the documented sub-rectangle mode.
        let cli = Cli::try_parse_from([
            "hanten",
            "estimate",
            "in.tiff",
            "--grid",
            "--base-region",
            "0,0,9,9",
        ])
        .unwrap();
        match cli.command {
            Command::Estimate(a) => {
                assert!(a.grid);
                assert_eq!(a.film_base.base_region, Some([0, 0, 9, 9]));
            }
            _ => unreachable!("expected estimate"),
        }
    }

    #[test]
    fn the_calibration_fragment_round_trips_as_a_recipe() {
        // The report's `calibration` object must drop into a recipe with no hand
        // editing of its own — that is the workflow it exists for
        // (`hanten estimate … | jq '{recipe_version: 2, calibration}' > roll-cal.json`).
        let fragment = CalibrationFragment {
            film_base: Some(FilmBaseSource::Explicit([0.553, 0.271, 0.159])),
        };
        let json = serde_json::to_string(&fragment).unwrap();
        assert_eq!(json, r#"{"film_base":{"explicit":[0.553,0.271,0.159]}}"#);
        let recipe: Recipe =
            serde_json::from_str(&format!(r#"{{"recipe_version":2,"calibration":{json}}}"#))
                .unwrap();
        assert_eq!(
            recipe.calibration.film_base,
            Some(FilmBaseSource::Explicit([0.553, 0.271, 0.159]))
        );
        validate_shared(&recipe, FilmBaseRemedy::Flags).unwrap();
    }

    /// **Each member is emitted only when it was measured, and never defaulted**, so a
    /// fragment piped into `--params` pins nothing a later layer states.
    #[test]
    fn the_calibration_fragment_omits_what_was_not_measured() {
        let base_only = CalibrationFragment {
            film_base: Some(FilmBaseSource::Auto),
        };
        assert_eq!(
            serde_json::to_string(&base_only).unwrap(),
            r#"{"film_base":"auto"}"#
        );
        // Nothing measured is `None`, not `{}`: an empty object would pipe into
        // `--params` as a no-op a user could mistake for a calibration.
        assert!(CalibrationFragment::default().is_empty());
        assert!(calibration_fragment(&Report::default()).is_none());
    }

    #[test]
    fn film_base_flag_string_round_trips_exact_f32s() {
        // `Display` for f32 prints the shortest decimal that parses back to the
        // same bits, so the emitted `--film-base` string reproduces the exact
        // measured base — including awkward values with no short decimal form.
        let rgb = [0.553_712_3_f32, 1.0 / 3.0, f32::MIN_POSITIVE];
        let (flag, source) = reuse_ready(rgb).expect("a valid base is reuse-ready");
        let value = flag.strip_prefix("--film-base ").unwrap();
        assert_eq!(parse_rgb(value).unwrap(), rgb);
        // The two forms carry the same value — never allowed to drift.
        assert_eq!(source, FilmBaseSource::Explicit(rgb));
    }

    #[test]
    fn report_reuse_flattens_to_flat_keys_or_nothing() {
        // The wire contract: the flag half serializes as the flat top-level key
        // `film_base_flag`, the recipe half rides inside `calibration`, and the
        // `ReuseReady` wrapper / `reuse` field name never leaks. `None` emits
        // neither. Locks the `#[serde(flatten)]` + rename shape so a refactor
        // can't silently change the agent-facing JSON.
        // Values exactly representable in f32 (halves/quarters/eighths) so the
        // JSON literals match without precision noise — the shape is the point.
        let reuse = ReuseReady {
            flag: "--film-base 0.5,0.25,0.125".to_string(),
            source: FilmBaseSource::Explicit([0.5, 0.25, 0.125]),
        };
        let with = Report {
            calibration: calibration_fragment(&Report {
                reuse: Some(reuse.clone()),
                ..Report::default()
            }),
            reuse: Some(reuse),
            ..Report::default()
        };
        let v = serde_json::to_value(&with).unwrap();
        assert_eq!(v["film_base_flag"], "--film-base 0.5,0.25,0.125");
        assert_eq!(
            v["calibration"],
            serde_json::json!({ "film_base": { "explicit": [0.5, 0.25, 0.125] } })
        );
        // The old key is gone, not renamed in place: a consumer still reading it must
        // fail loudly rather than silently see nothing.
        assert!(v.get("film_base_recipe").is_none());
        assert!(v.get("reuse").is_none(), "the wrapper name must not leak");

        let without = Report::default();
        let v = serde_json::to_value(&without).unwrap();
        assert!(v.get("film_base_flag").is_none());
        assert!(v.get("calibration").is_none());
        assert!(v.get("reuse").is_none());
    }

    #[test]
    fn reuse_ready_suppresses_degenerate_bases() {
        // The safety contract of the reuse output: a measurement `convert`
        // would reject (dark-holder zero, non-finite, >1 typo-scale) must never
        // be advertised as a paste-ready --film-base.
        assert!(reuse_ready([0.0, 0.5, 0.5]).is_none()); // dark holder channel
        assert!(reuse_ready([f32::NAN, 0.5, 0.5]).is_none()); // numerical fault
        assert!(reuse_ready([0.9, 90.0, 0.4]).is_none()); // "90" typo for "0.90"
        assert!(reuse_ready([-0.1, 0.5, 0.5]).is_none()); // negative
        // A valid base produces the exact flag string and matching fragment.
        let (flag, source) = reuse_ready([0.553, 0.271, 0.159]).unwrap();
        assert_eq!(flag, "--film-base 0.553,0.271,0.159");
        assert_eq!(source, FilmBaseSource::Explicit([0.553, 0.271, 0.159]));
    }

    #[test]
    fn load_recipe_maps_failures_to_usage() {
        // No path → defaults, infallibly.
        let loaded = load_recipe(None).unwrap();
        assert_eq!(loaded.recipe, Recipe::default());

        // Missing file → Usage (exit 2), not Other.
        let missing = std::env::temp_dir().join("nc-no-such-recipe-xyz.json");
        assert!(matches!(
            load_recipe(Some(&missing)),
            Err(NcError::Usage(_))
        ));

        // Malformed JSON and unknown keys both map to Usage.
        for (tag, body) in [
            ("malformed", "{ not json"),
            (
                "unknown-key",
                r#"{"recipe_version":2,"reconstruction":{"scal":[1,1,1]}}"#,
            ),
        ] {
            let got = load_recipe_body(tag, body);
            assert!(
                matches!(got, Err(NcError::Usage(_))),
                "{tag} should be Usage"
            );
        }

        // A valid partial recipe loads and fills defaults.
        let got = load_recipe_body("ok", r#"{"recipe_version":2,"look":{"contrast":1.3}}"#)
            .unwrap()
            .recipe;
        assert_eq!(got.look.contrast, Some(1.3));
        assert_eq!(got.reconstruction, Recipe::default().reconstruction);
    }

    /// Write `body` to a temp recipe, load it, clean up, return the result.
    fn load_recipe_body(tag: &str, body: &str) -> Result<LoadedRecipe> {
        let p = std::env::temp_dir().join(format!("nc-env-{tag}-{}.json", std::process::id()));
        std::fs::write(&p, body).unwrap();
        let got = load_recipe(Some(&p));
        std::fs::remove_file(&p).ok();
        got
    }

    #[test]
    fn load_recipe_accepts_the_envelope_and_the_bare_shape() {
        // Identity lives in a `meta` envelope beside the recipe, so both an enveloped
        // and a bare document load — and to the *same* recipe.
        let bare = r#"{"recipe_version":2,"look":{"contrast":1.3}}"#;
        let enveloped = format!(
            r#"{{"meta":{{"nc_version":"0.1.0","pipeline_version":7,
                          "params_hash":"0123456789abcdef","git_commit":"abc"}},
                "params":{bare}}}"#
        );
        let flat = load_recipe_body("bare", bare).unwrap();
        let wrapped = load_recipe_body("env", &enveloped).unwrap();
        assert_eq!(
            flat.recipe, wrapped.recipe,
            "both shapes resolve to one recipe"
        );
        assert_eq!(wrapped.recipe.look.contrast, Some(1.3));
        // A bare recipe records no provenance; the envelope's is read but never
        // applied — only compared (see `pipeline_version_warning`).
        assert_eq!(flat.meta_pipeline_version, None);
        assert_eq!(wrapped.meta_pipeline_version, Some(7));
    }

    #[test]
    fn a_recipe_or_sidecar_the_removed_chain_wrote_is_refused_with_the_way_forward() {
        // Every sidecar and `--dump-params` file written before `pipeline_version` 8 is
        // unversioned — bare, or enveloped. Both are refused by name, never parsed as an
        // unknown field, and the message says what to do instead.
        let old = r#"{"reconstruction":{"schema_version":1},"print":{"print_exposure":0.0},
                      "output":{"preset":"gain-map-hdr"}}"#;
        for body in [
            old.to_string(),
            format!(r#"{{"meta":{{"pipeline_version":7}},"params":{old}}}"#),
        ] {
            let err = load_recipe_body("removed", &body).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{body}");
            let msg = err.to_string();
            assert!(msg.contains("recipe_version"), "{msg}");
            assert!(msg.contains("`hanten params`"), "{msg}");
            assert!(!msg.contains("--new-flow"), "{msg}");
        }
    }

    #[test]
    fn envelope_errors_are_loud_and_specific() {
        // `meta` with no `params` is a half-written envelope, not a bare recipe:
        // it must say so rather than emit serde's opaque `unknown field 'meta'`.
        let err = load_recipe_body("half", r#"{"meta":{"pipeline_version":1}}"#).unwrap_err();
        assert!(
            matches!(&err, NcError::Usage(m) if m.contains("`meta` block but no `params`")),
            "got {err}"
        );
        // A third sibling key alongside meta/params is rejected, not ignored — the
        // envelope is `deny_unknown_fields` too.
        assert!(matches!(
            load_recipe_body("extra", r#"{"meta":{},"params":{},"surprise":1}"#),
            Err(NcError::Usage(_))
        ));
        // Legacy-key migration errors still fire on an enveloped body.
        assert!(matches!(
            load_recipe_body("legacy", r#"{"meta":{},"params":{"algorithm":"density"}}"#),
            Err(NcError::Usage(_))
        ));
    }

    #[test]
    fn pipeline_version_warning_fires_only_on_a_real_mismatch() {
        // No recorded version (a bare/legacy recipe) ⇒ nothing to compare, no noise.
        assert_eq!(pipeline_version_warning(None), None);
        // The current version ⇒ no warning.
        assert_eq!(
            pipeline_version_warning(Some(version::PIPELINE_VERSION)),
            None
        );
        // Any other version ⇒ a warning naming both numbers, so the operator can
        // see which direction the skew runs.
        let other = version::PIPELINE_VERSION.wrapping_add(1);
        let msg = pipeline_version_warning(Some(other)).expect("mismatch must warn");
        assert!(msg.contains(&format!("pipeline_version {other}")), "{msg}");
        assert!(
            msg.contains(&format!("pipeline_version {}", version::PIPELINE_VERSION)),
            "{msg}"
        );
    }

    #[test]
    fn params_and_meta_are_not_recipe_keys() {
        // `params` is the reserved discriminator that tells an envelope from a bare
        // recipe, and `meta` its sibling. If a future stage section ever claimed
        // either name, every recipe carrying it would be silently reinterpreted as
        // an envelope (or rejected), so pin that the resolved recipe's own top level
        // never uses them.
        let value = serde_json::to_value(base_recipe()).unwrap();
        let keys: Vec<&String> = value.as_object().unwrap().keys().collect();
        for reserved in ["params", "meta"] {
            assert!(
                !keys.iter().any(|k| k.as_str() == reserved),
                "`{reserved}` is reserved for the sidecar envelope but is now a recipe key: \
                 {keys:?}"
            );
        }
    }

    #[test]
    fn a_malformed_meta_container_is_as_loud_as_a_malformed_field() {
        // The guard was on the *field* but not its *container*: `Value::get` on a
        // non-object returns `None`, which this path read as "records no
        // pipeline_version" — indistinguishable from a bare recipe. So a
        // sidecar whose whole `meta` block was corrupt replayed with NO skew check,
        // while a corrupt field inside a well-formed `meta` was a loud exit 2.
        for body in [
            r#"{"meta":null,"params":{}}"#,
            r#"{"meta":"x","params":{}}"#,
            r#"{"meta":[],"params":{}}"#,
            r#"{"meta":123,"params":{}}"#,
            r#"{"meta":true,"params":{}}"#,
        ] {
            let err = load_recipe_body("bad-meta", body).unwrap_err();
            assert!(
                matches!(&err, NcError::Usage(m) if m.contains("`meta` must be an object")),
                "{body}: got {err}"
            );
        }
        // An OMITTED `meta` stays legal — a hand-wrapped `--dump-params` recipe has no
        // provenance to record, and that is not a malformed envelope.
        assert_eq!(
            load_recipe_body("no-meta", r#"{"params":{"recipe_version":2}}"#)
                .unwrap()
                .meta_pipeline_version,
            None
        );
        // An empty `meta` object is legal too, and records nothing.
        assert_eq!(
            load_recipe_body("empty-meta", r#"{"meta":{},"params":{"recipe_version":2}}"#)
                .unwrap()
                .meta_pipeline_version,
            None
        );
        // Unknown fields inside a well-formed `meta` stay lenient — that leniency is
        // the forward-compatibility contract, not an oversight.
        assert_eq!(
            load_recipe_body(
                "future-meta",
                r#"{"meta":{"invented":[1],"pipeline_version":7},"params":{"recipe_version":2}}"#
            )
            .unwrap()
            .meta_pipeline_version,
            Some(7)
        );
    }

    #[test]
    fn meta_pipeline_version_rejects_values_it_cannot_read() {
        // Present-but-unreadable must be LOUD. Mapped to `None` it would be
        // indistinguishable from "this file records no version" and would silently
        // disable the skew warning; truncated with `as u32` it can even land on this
        // build's version and pretend to agree.
        let ok = serde_json::json!({"pipeline_version": 7});
        assert_eq!(meta_pipeline_version(Some(&ok), "ctx").unwrap(), Some(7));
        // Absent (whole meta, or just the key) ⇒ genuinely nothing recorded.
        assert_eq!(meta_pipeline_version(None, "ctx").unwrap(), None);
        let empty = serde_json::json!({});
        assert_eq!(meta_pipeline_version(Some(&empty), "ctx").unwrap(), None);

        for bad in [
            serde_json::json!({"pipeline_version": 1.0}),
            serde_json::json!({"pipeline_version": "1"}),
            serde_json::json!({"pipeline_version": -1}),
            serde_json::json!({"pipeline_version": null}),
            // u32::MAX + 2 — `as u32` would truncate this to 1, matching a build at
            // pipeline_version 1 and suppressing the warning entirely.
            serde_json::json!({"pipeline_version": 4294967297u64}),
        ] {
            assert!(
                matches!(
                    meta_pipeline_version(Some(&bad), "ctx"),
                    Err(NcError::Usage(_))
                ),
                "{bad} must be refused"
            );
        }
    }

    #[test]
    fn a_non_object_recipe_body_is_refused_instead_of_silently_defaulting() {
        // serde accepts a sequence for a struct, so these would otherwise reach the
        // typed parse with a message about a sequence rather than about the document —
        // a truncated or mis-generated file should be named as one.
        for (tag, body) in [
            ("arr-envelope", r#"{"params": []}"#),
            ("arr-bare", "[]"),
            ("num-envelope", r#"{"params": 3}"#),
            ("str-bare", r#""nope""#),
        ] {
            let err = load_recipe_body(tag, body).unwrap_err();
            assert!(
                matches!(&err, NcError::Usage(m) if m.contains("must be a")),
                "{tag}: got {err}"
            );
        }
        // A document stating only its version is valid on both levels — a recipe that
        // legitimately means "all defaults". Note this compares against the *bare*
        // default: a recipe that says nothing leaves `calibration.film_base` unset
        // (`None`), which `validate_shared` later rejects for `convert`. "All defaults"
        // is not the same as "ready to run".
        for (tag, body) in [
            ("obj-bare", r#"{"recipe_version": 2}"#),
            ("obj-envelope", r#"{"params": {"recipe_version": 2}}"#),
        ] {
            assert_eq!(
                load_recipe_body(tag, body).unwrap().recipe,
                Recipe::default()
            );
        }
        // An empty object says nothing about which document it is, so it is refused
        // for its missing version rather than read as defaults.
        let err = load_recipe_body("obj-empty", "{}").unwrap_err();
        assert!(err.to_string().contains("recipe_version"), "{err}");
    }

    #[test]
    fn keys_collide_is_case_insensitivity_aware() {
        assert!(keys_collide(
            Path::new("/d/out.tiff"),
            Path::new("/d/out.tiff")
        ));
        // Case-only difference must collide (conservative over-reject).
        assert!(keys_collide(
            Path::new("/d/out.tiff"),
            Path::new("/d/OUT.TIFF")
        ));
        // Genuinely different names must not.
        assert!(!keys_collide(
            Path::new("/d/out.tiff"),
            Path::new("/d/other.tiff")
        ));
    }

    #[test]
    fn write_targets_reject_case_only_collision_before_creation() {
        // `-o out.tiff --telemetry-file OUT.TIFF` on a case-insensitive FS is the
        // same file; with neither pre-existing, `collision_key` can't canonicalize
        // to a shared casing, so the guard must catch it via the case-insensitive
        // comparison. Use a real (existing) parent dir with non-existent children.
        let dir = std::env::temp_dir().join(format!("nc-case-collide-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("out.tiff");
        let tel = dir.join("OUT.TIFF");
        let input = dir.join("in.tiff");
        let got = ensure_write_targets_distinct(
            &input,
            &[("--output", &out), ("--telemetry-file", &tel)],
        );
        std::fs::remove_dir_all(&dir).ok();
        assert!(
            matches!(got, Err(NcError::Usage(_))),
            "a case-only telemetry-file/output collision must be a usage error: {got:?}"
        );
    }

    // --- roll (batch) --------------------------------------------------------

    #[test]
    fn roll_requires_input_or_frames_and_they_conflict() {
        // Neither positional inputs nor --frames → usage error.
        assert!(Cli::try_parse_from(["hanten", "roll", "-o", "out"]).is_err());
        // Both → mutually exclusive.
        assert!(
            Cli::try_parse_from(["hanten", "roll", "a.tif", "--frames", "m.json", "-o", "out"])
                .is_err()
        );
        // Either alone (with --out-dir) is fine.
        assert!(Cli::try_parse_from(["hanten", "roll", "a.tif", "b.tif", "-o", "out"]).is_ok());
        assert!(Cli::try_parse_from(["hanten", "roll", "--frames", "m.json", "-o", "out"]).is_ok());
        // --out-dir is required.
        assert!(Cli::try_parse_from(["hanten", "roll", "a.tif"]).is_err());
    }

    /// A bare tag over a newtype variant **replaces** it, so serde rejects the incomplete
    /// override. Every externally-tagged recipe variant is a newtype carrying a positional
    /// payload, where a bare tag states nothing *and there is nothing it could state*. A
    /// guard that once kept the base on a matching tag (for the since-retired
    /// `print.display_tone` struct variant) silently turned a malformed per-frame
    /// `{"film_base": {"source": "explicit"}}` into an inherit at exit 0.
    #[test]
    fn a_bare_tag_overlay_replaces_a_newtype_variant() {
        for base in [
            serde_json::json!({"explicit": [0.9, 0.55, 0.42]}), // FilmBaseSource / WbSource
            serde_json::json!({"region": [1, 2, 3, 4]}),        // FilmBaseSource::Region
        ] {
            let tag = base.as_object().unwrap().keys().next().unwrap().clone();
            let mut merged = base.clone();
            merge_json(&mut merged, &serde_json::json!(tag.clone()));
            assert_eq!(
                merged,
                serde_json::json!(tag),
                "a bare tag over the newtype variant {base} must replace it, so serde still \
                 rejects the incomplete override rather than silently inheriting"
            );
        }
    }

    #[test]
    fn merge_json_deep_merges_objects_and_replaces_other_values() {
        // Objects merge key-by-key (recursively); scalars/arrays replace wholesale.
        let mut base = serde_json::json!({"a": {"x": 1, "y": 2}, "b": 3});
        let overlay = serde_json::json!({"a": {"y": 20, "z": 30}, "b": [1, 2]});
        merge_json(&mut base, &overlay);
        assert_eq!(
            base,
            serde_json::json!({"a": {"x": 1, "y": 20, "z": 30}, "b": [1, 2]})
        );
    }

    #[test]
    fn merge_json_merges_destination_axes_but_switches_the_output_variant() {
        // One stated axis each, different keys: the shape of a variant switch, but a
        // struct of optional fields — the frame's container joins the roll's transfer.
        let mut base = serde_json::json!({"output": {"display": {"transfer": "pq"}}});
        let overlay = serde_json::json!({"output": {"display": {"container": "avif"}}});
        merge_json(&mut base, &overlay);
        assert_eq!(
            base,
            serde_json::json!({"output": {"display": {"transfer": "pq", "container": "avif"}}})
        );
        // The genuine enum level still switches: the film master replaces the display
        // arm, and a display arm replaces the film master.
        let mut base = serde_json::json!({"output": {"display": {"transfer": "pq"}}});
        merge_json(&mut base, &serde_json::json!({"output": "film-master"}));
        assert_eq!(base, serde_json::json!({"output": "film-master"}));
        let overlay = serde_json::json!({"output": {"display": {"gamut": "adobe-rgb"}}});
        merge_json(&mut base, &overlay);
        assert_eq!(base, overlay);
        // An externally tagged enum elsewhere is unaffected.
        let mut base = serde_json::json!({"film_base": {"region": [1, 2, 3, 4]}});
        merge_json(
            &mut base,
            &serde_json::json!({"film_base": {"explicit": [0.9, 0.5, 0.4]}}),
        );
        assert_eq!(
            base,
            serde_json::json!({"film_base": {"explicit": [0.9, 0.5, 0.4]}})
        );
    }

    #[test]
    fn a_frames_own_roll_white_keeps_the_rolls_gains() {
        // `roll --frames` merges over the *serialized* shared recipe, where an unset roll
        // value is a `null` key, so a one-key overlay is never read as an enum switch.
        let mut shared = crate::recipe::Recipe::default();
        shared.roll.white_balance = Some([0.8, 1.0, 1.25]);
        let mut v = serde_json::to_value(&shared).unwrap();
        merge_json(&mut v, &serde_json::json!({"roll": {"white_stops": 2.0}}));
        let frame: crate::recipe::Recipe = serde_json::from_value(v).unwrap();
        assert_eq!(frame.roll.white_balance, Some([0.8, 1.0, 1.25]));
        assert_eq!(frame.roll.white_stops, Some(2.0));
    }

    #[test]
    fn merge_json_replaces_enum_variant_switch_but_deep_merges_same_tag() {
        // An externally-tagged enum variant switch (`region` → `explicit`) must
        // REPLACE the one-key map, not union the tags — a `{"region":…,
        // "explicit":…}` object deserializes as no enum variant. Regression guard
        // for the per-frame `calibration.film_base` override path.
        let mut base = serde_json::json!({"film_base": {"source": {"region": [1, 2, 3, 4]}}});
        let overlay = serde_json::json!({"film_base": {"source": {"explicit": [0.9, 0.5, 0.4]}}});
        merge_json(&mut base, &overlay);
        assert_eq!(
            base,
            serde_json::json!({"film_base": {"source": {"explicit": [0.9, 0.5, 0.4]}}})
        );
        // The SAME tag on both sides is not a variant switch: recurse into it so a
        // partial override of one sub-field keeps its siblings.
        let mut base = serde_json::json!({"curve": {"dmax": {"auto": {"p": 0.5, "q": 1}}}});
        let overlay = serde_json::json!({"curve": {"dmax": {"auto": {"p": 0.9}}}});
        merge_json(&mut base, &overlay);
        assert_eq!(
            base,
            serde_json::json!({"curve": {"dmax": {"auto": {"p": 0.9, "q": 1}}}})
        );
    }

    /// `roll` over a `--frames` manifest at `manifest`, writing into `dir`.
    fn roll_args(manifest: &Path, dir: &Path) -> RollArgs {
        RollArgs {
            inputs: vec![],
            frames: Some(manifest.to_path_buf()),
            out_dir: dir.to_path_buf(),
            recipe_in: None,
            strict: false,
            new_flow: false,
            memory: MemoryArgs::default(),
            report: ReportArgs::default(),
        }
    }

    #[test]
    fn per_frame_override_refuses_a_removed_key_by_name() {
        // Through `resolve_frames`: a per-frame override naming the removed chain's keys
        // gets the same migration guidance as a whole recipe, not an opaque
        // deny-unknown-fields error from the merged deserialize.
        let dir = std::env::temp_dir().join(format!("nc-roll-removed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = dir.join("frames.json");
        let args = roll_args(&manifest, &dir);
        let log = Log::new(&args.report);
        let plan = |frames: &str| {
            std::fs::write(&manifest, frames).unwrap();
            resolve_frames(&args, &base_recipe(), &mut Vec::new(), &log)
        };
        for (params, names) in [
            (r#"{"print":{"print_exposure":0.2}}"#, "`print`"),
            (r#"{"density":{"scale":[1,1,1]}}"#, "`density`"),
            (
                r#"{"reconstruction":{"curve":{"type":"exponential"}}}"#,
                "reconstruction.curve",
            ),
            (r#"{"output":{"preset":"display-p3"}}"#, "output.preset"),
        ] {
            let err = plan(&format!(
                r#"{{"frames":[{{"input":"a.tif","params":{params}}}]}}"#
            ))
            .expect_err("a removed key must be refused");
            assert_eq!(err.exit_code(), 2, "{params}");
            assert!(err.to_string().contains(names), "{params}: {err}");
            assert!(err.to_string().contains("frame a.tif"), "{params}: {err}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn per_frame_override_can_switch_film_base_variant_and_still_warns() {
        // A per-frame `params` override that flips the roll-fixed `calibration.film_base`
        // from `region` to `explicit` must APPLY (the merged JSON deserializes) and
        // still raise the roll-level "base overridden" warning. Before the
        // variant-switch fix the merge unioned the tags and `from_value` rejected
        // it, turning a valid override into a confusing error.
        let dir = std::env::temp_dir().join(format!("nc-roll-varswitch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = dir.join("frames.json");
        std::fs::write(
            &manifest,
            r#"{"frames":[{"input":"a.tif",
                          "params":{"calibration":{"film_base":{"explicit":[0.9,0.55,0.42]}}}}]}"#,
        )
        .unwrap();
        let args = roll_args(&manifest, &dir);
        let mut shared = base_recipe();
        shared.calibration.film_base = Some(FilmBaseSource::Region([10, 10, 20, 20]));
        let mut warnings = Vec::new();
        let log = Log::new(&args.report);
        let planned = resolve_frames(&args, &shared, &mut warnings, &log);
        std::fs::remove_dir_all(&dir).ok();
        let planned = planned.expect("region→explicit override should apply, not error");
        assert_eq!(planned.len(), 1);
        assert_eq!(
            planned[0].recipe.calibration.film_base,
            Some(FilmBaseSource::Explicit([0.9, 0.55, 0.42]))
        );
        assert!(
            !warnings.is_empty(),
            "overriding the roll-fixed film base must still warn"
        );
    }

    /// Every recipe key a roll frame may set with no warning: the complement of
    /// [`ROLL_WIDE`].
    const FRAME_LOCAL: &[&str] = &[
        "recipe_version",
        "input",
        "roll.white_stops",
        "measure",
        "scene_correction",
        "look",
        "fit_range",
        "fit_gamut",
    ];

    #[test]
    fn roll_classifies_every_recipe_key() {
        let doc = serde_json::to_value(Recipe::default()).unwrap();
        let wide: Vec<&str> = ROLL_WIDE.iter().map(|w| w.key).collect();
        let classified = |k: &str| FRAME_LOCAL.contains(&k) || wide.contains(&k);
        let mut keys = Vec::new();
        for (section, v) in doc.as_object().unwrap() {
            if classified(section) {
                keys.push(section.clone());
                continue;
            }
            let fields = v
                .as_object()
                .unwrap_or_else(|| panic!("`{section}` is neither roll-wide nor frame-local"));
            for field in fields.keys() {
                let key = format!("{section}.{field}");
                assert!(
                    classified(&key),
                    "`{key}` is neither roll-wide nor frame-local"
                );
                keys.push(key);
            }
        }
        for k in wide.iter().chain(FRAME_LOCAL) {
            assert!(keys.iter().any(|x| x == k), "`{k}` names no recipe key");
        }
    }

    #[test]
    fn every_roll_wide_value_warns_on_its_own_key() {
        let mut shared = base_recipe();
        shared.calibration.film_base = Some(FilmBaseSource::Explicit([0.9, 0.55, 0.42]));
        shared.roll.white_balance = Some([1.05, 1.0, 0.95]);
        type Change = (&'static str, fn(&mut Recipe));
        let changes: [Change; 8] = [
            ("calibration.film_base", |r| {
                r.calibration.film_base = Some(FilmBaseSource::Explicit([0.8, 0.5, 0.4]))
            }),
            ("roll.white_balance", |r| {
                r.roll.white_balance = Some([1.1, 1.0, 0.9])
            }),
            ("reconstruction.scale", |r| {
                r.reconstruction.scale = [1.0, 0.9, 0.8]
            }),
            ("reconstruction.offset", |r| {
                r.reconstruction.offset = [0.1, 0.0, 0.0]
            }),
            ("reconstruction.linearization", |r| {
                r.reconstruction.linearization = 1.7
            }),
            ("reconstruction.anchor", |r| {
                r.reconstruction.anchor = fixed::AnchorRule::MidAboveBase(0.5)
            }),
            ("rendering", |r| {
                r.rendering = crate::rendering::Rendering::Direct
            }),
            ("output", |r| r.output = OutputSection::FilmMaster),
        ];
        assert_eq!(changes.len(), ROLL_WIDE.len(), "a row with no case");
        for (key, change) in changes {
            let mut frame = shared.clone();
            change(&mut frame);
            let w = roll_wide_breaks(Path::new("a.tif"), &frame, &shared).unwrap();
            assert_eq!(w.len(), 1, "{key}: {w:?}");
            assert!(
                w[0].contains(&format!("resolves `{key}` to ")),
                "{key}: {}",
                w[0]
            );
        }
    }

    #[test]
    fn a_roll_wide_value_warns_only_when_it_changes_and_names_both() {
        let mut shared = base_recipe();
        shared.calibration.film_base = Some(FilmBaseSource::Explicit([0.9, 0.55, 0.42]));
        let frame = Path::new("a.tif");
        // A restatement (even as -0.0), and a frame-local change, are silent.
        let mut same = shared.clone();
        same.roll.white_stops = Some(1.7);
        same.scene_correction.exposure = 0.3;
        same.reconstruction.offset = [-0.0, 0.0, 0.0];
        assert!(roll_wide_breaks(frame, &same, &shared).unwrap().is_empty());

        // Gains that never reach the frame are not a break: `direct` leaves the roll
        // out, and the film master runs no scene correction.
        for leaves_out_the_roll in [
            |r: &mut Recipe| r.rendering = crate::rendering::Rendering::Direct,
            |r: &mut Recipe| r.output = OutputSection::FilmMaster,
        ] {
            let mut roll = shared.clone();
            leaves_out_the_roll(&mut roll);
            let mut gains = roll.clone();
            gains.roll.white_balance = Some([1.1, 1.0, 0.9]);
            assert!(roll_wide_breaks(frame, &gains, &roll).unwrap().is_empty());
        }

        let mut moved = shared.clone();
        moved.reconstruction.linearization = 0.5;
        moved.roll.white_balance = Some([1.1, 1.0, 0.9]);
        let w = roll_wide_breaks(frame, &moved, &shared).unwrap();
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(
            w[0].contains("`roll.white_balance` to [1.1,1.0,0.9]"),
            "{}",
            w[0]
        );
        assert!(w[0].contains("the roll's is unset"), "{}", w[0]);
        assert!(
            w[1].contains("`reconstruction.linearization` to 0.5"),
            "{}",
            w[1]
        );
        assert!(w.iter().all(|m| m.starts_with("frame a.tif:")), "{w:?}");

        // A `rendering` change that moves the derived destination is one warning, naming
        // both destinations; `output` warns only when the frame states another.
        let mut direct = shared.clone();
        direct.rendering = crate::rendering::Rendering::Direct;
        let w = roll_wide_breaks(frame, &direct, &shared).unwrap();
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(
            w[0].contains("`rendering` to \"direct\" (destination {"),
            "{}",
            w[0]
        );
        let mut master = shared.clone();
        master.output = OutputSection::FilmMaster;
        let w = roll_wide_breaks(frame, &master, &shared).unwrap();
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("`output` to \"film-master\""), "{}", w[0]);
    }

    #[test]
    fn per_frame_override_keeps_shared_roll_fixed_params() {
        // A partial override changes only its own knob and keeps the shared roll-fixed
        // params (the film base) — the "frame-local override applies to just that
        // frame" guarantee, through `resolve_frames` itself.
        let dir = std::env::temp_dir().join(format!("nc-roll-partial-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = dir.join("frames.json");
        std::fs::write(
            &manifest,
            r#"{"frames":[{"input":"a.tif","params":{"scene_correction":{"exposure":0.15}}},
                          {"input":"b.tif"}]}"#,
        )
        .unwrap();
        let args = roll_args(&manifest, &dir);
        let mut shared = base_recipe();
        shared.calibration.film_base = Some(FilmBaseSource::Explicit([0.9, 0.55, 0.42]));
        shared.look.contrast = Some(1.3);
        let log = Log::new(&args.report);
        let planned = resolve_frames(&args, &shared, &mut Vec::new(), &log);
        std::fs::remove_dir_all(&dir).ok();
        let planned = planned.unwrap();
        let a = &planned[0].recipe;
        assert_eq!(a.scene_correction.exposure, 0.15);
        assert_eq!(a.calibration.film_base, shared.calibration.film_base);
        assert_eq!(a.look, shared.look);
        // The frame without an override runs the shared recipe unchanged.
        assert_eq!(planned[1].recipe, shared);
        assert_eq!(planned[1].overrides, None);
    }

    #[test]
    fn manifest_rejects_unknown_keys_and_parses_overrides() {
        // `deny_unknown_fields` at both levels catches a typo'd manifest.
        assert!(serde_json::from_str::<RollManifest>(r#"{"framez":[]}"#).is_err());
        assert!(
            serde_json::from_str::<RollManifest>(r#"{"frames":[{"input":"a.tif","bogus":1}]}"#)
                .is_err()
        );
        // A well-formed manifest with a per-frame override + output parses.
        let m: RollManifest = serde_json::from_str(
            r#"{"frames":[{"input":"a.tif","output":"a_out.tiff",
                           "params":{"scene_correction":{"exposure":0.2}}}]}"#,
        )
        .unwrap();
        assert_eq!(m.frames.len(), 1);
        assert_eq!(m.frames[0].input, PathBuf::from("a.tif"));
        assert_eq!(m.frames[0].output, Some(PathBuf::from("a_out.tiff")));
        assert!(m.frames[0].params.is_some());
    }

    #[test]
    fn tiff_ext_and_output_naming() {
        assert!(has_tiff_ext(Path::new("a.tif")));
        assert!(has_tiff_ext(Path::new("a.TIFF")));
        assert!(!has_tiff_ext(Path::new("a.png")));
        assert!(!has_tiff_ext(Path::new("a")));
        let tiff = target_of(&[]);
        assert_eq!(
            default_output_name(Path::new("/scans/frame01.tif"), Path::new("/out"), tiff),
            PathBuf::from("/out/frame01_positive.tiff")
        );
        // A manifest output: relative joins the out-dir, absolute is used verbatim,
        // and `None` falls back to the derived name.
        for (given, want) in [
            (Some("custom.tiff"), "/out/custom.tiff"),
            (Some("/abs/c.tiff"), "/abs/c.tiff"),
            (None, "/out/f_positive.tiff"),
        ] {
            assert_eq!(
                resolve_frame_output(
                    given.map(Path::new),
                    Path::new("/s/f.tif"),
                    Path::new("/out"),
                    tiff,
                )
                .unwrap(),
                PathBuf::from(want),
                "{given:?}"
            );
        }
    }

    #[test]
    fn roll_derives_every_container_suffix_and_checks_only_explicit_paths() {
        // The derived name follows the destination's container, and passing it back
        // through the resolver returns it **unchanged** rather than completing it a
        // second time — the invariant that lets roll skip re-checking a derived name.
        for (extra, want) in [
            (&["--film-master"][..], "f_positive.tiff"),
            (&["--range", "hdr"][..], "f_positive.jpg"),
            (
                &["--transfer", "pq", "--container", "avif"][..],
                "f_positive.avif",
            ),
            (&[][..], "f_positive.tiff"),
        ] {
            let target = target_of(extra);
            let derived = default_output_name(Path::new("/s/f.tif"), Path::new("/out"), target);
            assert_eq!(derived, PathBuf::from(format!("/out/{want}")), "{extra:?}");
            assert_eq!(
                resolve_output_path(
                    &derived,
                    target,
                    SuffixContext::RollFrame(Path::new("/s/f.tif"))
                )
                .unwrap_or_else(|e| panic!("{extra:?} derives a name it rejects: {e}")),
                derived,
                "{extra:?} derives {} and the resolver then changed it",
                derived.display()
            );
        }
        // An **explicit** manifest path is checked, and the diagnosis names the frame
        // and the way out rather than just the rule — by recipe key, since `roll`
        // takes no conversion flags.
        let err = resolve_frame_output(
            Some(Path::new("frame.tiff")),
            Path::new("/s/f.tif"),
            Path::new("/out"),
            OutputTarget::resolve(
                &recipe::merge(base_recipe(), &parse_convert(&["--range", "hdr"])),
                KnobNames::KeyOnly,
                false,
            )
            .unwrap(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("frame /s/f.tif"), "{err}");
        assert!(err.contains(".jpg"), "{err}");
        assert!(
            !err.contains("--range"),
            "a roll frame names keys, not flags: {err}"
        );
        // An explicit manifest path shares the whole rule, so one stating no suffix
        // is *completed* like a `convert` path rather than refused — and the
        // relative-to-out-dir join still happens first.
        for (extra, want) in [
            (&["--range", "hdr"][..], "/out/chosen.jpg"),
            (&[][..], "/out/chosen.tiff"),
        ] {
            assert_eq!(
                resolve_frame_output(
                    Some(Path::new("chosen")),
                    Path::new("/s/f.tif"),
                    Path::new("/out"),
                    target_of(extra),
                )
                .unwrap(),
                PathBuf::from(want),
                "{extra:?}"
            );
        }
    }

    #[test]
    fn expand_input_lists_sorted_tiffs_and_skips_others() {
        // Directory expansion after the fail-loud rewrite: `.tif`/`.tiff` files
        // (case-insensitive) in sorted order, non-TIFF and extension-less entries
        // skipped. (A per-entry `read_dir` error is not portably reproducible in a
        // test, so only the happy path is exercised here.)
        let dir = std::env::temp_dir().join(format!("nc-expand-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["b.tif", "a.TIFF", "c.png", "d"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        let mut out = Vec::new();
        let got = expand_input(&dir, &mut out);
        std::fs::remove_dir_all(&dir).ok();
        got.expect("expanding a readable directory should succeed");
        assert_eq!(out, vec![dir.join("a.TIFF"), dir.join("b.tif")]);
    }

    #[test]
    fn reject_roll_unsupported_rejects_export_ir() {
        let mut cfg = base_recipe();
        assert!(reject_roll_unsupported(&cfg).is_ok());
        cfg.input.export_ir = Some("ir.tiff".into());
        assert!(matches!(
            reject_roll_unsupported(&cfg),
            Err(NcError::Usage(_))
        ));
    }

    #[test]
    fn ensure_roll_targets_distinct_catches_input_and_sibling_collisions() {
        // A target aimed at an input scan, and two frames colliding on one output
        // (e.g. same stem from different dirs), both fail loudly.
        let inputs = [Path::new("/scans/a.tif"), Path::new("/scans/b.tif")];
        let clobber_input = vec![("output for a".to_string(), PathBuf::from("/scans/a.tif"))];
        assert!(matches!(
            ensure_roll_targets_distinct(&inputs, &clobber_input),
            Err(NcError::Usage(_))
        ));
        let sibling_collision = vec![
            (
                "output for a".to_string(),
                PathBuf::from("/out/img_positive.tiff"),
            ),
            (
                "output for b".to_string(),
                PathBuf::from("/out/img_positive.tiff"),
            ),
        ];
        assert!(matches!(
            ensure_roll_targets_distinct(&inputs, &sibling_collision),
            Err(NcError::Usage(_))
        ));
        // Distinct outputs not touching any input are fine.
        let ok = vec![
            (
                "output for a".to_string(),
                PathBuf::from("/out/a_positive.tiff"),
            ),
            (
                "output for b".to_string(),
                PathBuf::from("/out/b_positive.tiff"),
            ),
        ];
        assert!(ensure_roll_targets_distinct(&inputs, &ok).is_ok());
    }

    #[test]
    fn ensure_roll_targets_distinct_protects_the_frames_manifest() {
        // `run_roll` adds the `--frames` manifest to the protected read set, so a
        // write target aimed at it (e.g. `--report-file` equal to the manifest
        // path) is rejected up front rather than clobbering the manifest.
        let manifest = Path::new("/rolls/frames.json");
        let inputs = [Path::new("/scans/a.tif"), manifest];
        let clobber_manifest = vec![(
            "--report-file".to_string(),
            PathBuf::from("/rolls/frames.json"),
        )];
        assert!(matches!(
            ensure_roll_targets_distinct(&inputs, &clobber_manifest),
            Err(NcError::Usage(_))
        ));
    }

    #[test]
    fn roll_report_carries_each_frames_resolved_values() {
        // Each frame echoes the *resolved* base it used (a redundant echo when the
        // recipe pins an explicit base, meaningful under `auto`/`region`). The per-frame
        // entry is the data-carrying `FrameStatus` — an "ok" frame serializes the flat
        // `"status":"ok"` with its payload as sibling keys. The shared recipe is not
        // echoed: each frame's identity hashes the recipe that frame ran.
        let roll = RollReport {
            command: "roll",
            identity: Identity::new(),
            warnings: vec![],
            frames: vec![FrameReport {
                input: PathBuf::from("f1.tif"),
                output: Some(PathBuf::from("out/f1_positive.tiff")),
                status: FrameStatus::Ok {
                    film_base: Some(FilmBase::from([0.9, 0.55, 0.42])),
                    effective_area: None,
                    input_color: None,
                    loss: None,
                    output_stats: Some(OutputStats {
                        mean: [0.25, 0.5, 0.75],
                    }),
                    film_type: Some(FilmType::Silver),
                    identity: Some(Box::new(Identity::new())),
                    chain: None,
                    avif: None,
                    hdr_linear_tiff: None,
                    hdr_coded_tiff: None,
                },
                memory: None,
                warnings: vec![],
                overrides: None,
            }],
            summary: RollSummary {
                total: 1,
                succeeded: 1,
                failed: 0,
            },
            elapsed_ms: Some(1.0),
        };
        let v = serde_json::to_value(&roll).unwrap();
        assert_eq!(v["command"], "roll");
        assert!(v.get("recipe").is_none(), "{v}");
        assert_eq!(v["summary"]["succeeded"], 1);
        // The flattened `FrameStatus::Ok` still serializes the flat `status`
        // discriminator and its payload as sibling keys of the frame entry.
        assert_eq!(v["frames"][0]["status"], "ok");
        assert_eq!(v["frames"][0]["input"], "f1.tif");
        let ffb: Vec<f64> = v["frames"][0]["film_base"]
            .as_object()
            .expect("per-frame resolved film base is a sibling key of status")
            .values()
            .map(|x| x.as_f64().unwrap())
            .collect();
        assert_eq!(ffb.len(), 3);
        assert_eq!(v["frames"][0]["film_type"], "silver");
    }

    #[test]
    fn failed_frame_report_keeps_accumulated_warnings_and_the_preflight_decision() {
        // A frame that warned and got sized before failing still carries both in its
        // report entry (neither is reset on the failure path). The memory block lives
        // on `FrameReport`, not on the `Ok` payload, precisely so a frame that passed
        // the gate and then failed doesn't throw the estimate away.
        let pf = PlannedFrame {
            input: PathBuf::from("bad.tif"),
            output: PathBuf::from("out/bad_positive.tiff"),
            recipe: base_recipe(),
            overrides: None,
        };
        let warnings = vec!["a warning raised before the failure".to_string()];
        let mem = memory::preflight(
            &crate::io::decode::ImageShape::new(1000, 1000, 3, 16, true).unwrap(),
            RunProfile::DecodeOnly,
            SamplePlan::auto(),
            memory::Budget::resolve(None),
            None,
        )
        .unwrap();
        let fr = frame_report_err(&pf, &NcError::Decode("boom".into()), Some(mem), warnings);
        let v = serde_json::to_value(&fr).unwrap();
        assert_eq!(v["status"], "failed");
        assert_eq!(v["error"], "decode: boom");
        assert_eq!(
            v["warnings"][0], "a warning raised before the failure",
            "a failed frame must keep the warnings accumulated before it failed: {v}"
        );
        assert_eq!(
            v["memory"]["estimated_peak_bytes"], mem.estimate.estimated_peak_bytes,
            "a failed frame must keep the preflight decision: {v}"
        );
    }

    #[test]
    fn ram_pressure_warning_reaches_the_report_and_strict() {
        // The warn tier is the one environment-dependent piece of the gate — and,
        // via `--strict`, the one way "same input + params ⇒ same exit" can break.
        // Drive it through the real wiring by injecting the RAM figure (there is
        // deliberately no env override): a machine small enough that the fixture's
        // estimate exceeds 70% of RAM must produce a report warning, which is what
        // `--strict` promotes to a failing exit.
        let log = Log::new(&ReportArgs::default());
        let input = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hdri-64bit.tif");
        let budget = memory::Budget::resolve(None);

        // Enough RAM: no warning.
        let mut warnings = Vec::new();
        let quiet = preflight_memory(
            &input,
            RunProfile::DecodeOnly,
            SamplePlan::auto(),
            budget,
            Some(64 * 1024 * 1024 * 1024),
            &log,
            &mut warnings,
        )
        .unwrap();
        assert_eq!(quiet.decision, memory::Verdict::Ok);
        assert!(warnings.is_empty(), "{warnings:?}");

        // A machine whose 70% line sits below the estimate: warn, but proceed.
        let tiny_ram = quiet.estimate.estimated_peak_bytes; // 70% of it is below the estimate
        let mut warnings = Vec::new();
        let warned = preflight_memory(
            &input,
            RunProfile::DecodeOnly,
            SamplePlan::auto(),
            budget,
            Some(tiny_ram),
            &log,
            &mut warnings,
        )
        .unwrap();
        assert_eq!(warned.decision, memory::Verdict::Warn);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("70%"), "{warnings:?}");
        assert!(warnings[0].contains("may swap"), "{warnings:?}");
        // …and that is a report warning, so `--strict`'s gate (`args.strict &&
        // !report.warnings.is_empty()`) fails the run on it.
        let report = Report {
            warnings,
            ..Report::default()
        };
        assert!(
            !report.warnings.is_empty(),
            "the warning must be `--strict`-promotable"
        );
    }
}

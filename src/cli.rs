//! CLI orchestration — the agent-facing command surface.
//!
//! This is the scriptable contract an agent drives: clap argument parsing for
//! every subcommand and flag (design-spec §8–9), layered JSON recipes (flags
//! override every `--params` layer; `recipe::compose`), `--dump-params` / `params` for discovery, a JSON
//! report, and stable exit codes via [`NcError`]. The conversion runs here:
//! `convert` drives the full read → film-base → fixed decode → scene correction →
//! look → fit range → fit gamut → encode chain (delegating the pure stages to
//! `pipeline`/`algo`/`io`); `inspect` and `measure-base` decode and report without
//! writing an image.
//!
//! Determinism rule: stdout carries *only* the JSON report / params; all logs and
//! warnings go to stderr, so an agent can pipe stdout straight into a parser.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fmt::Display;
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
use crate::io::encode::{PreEncodePage, PreEncodeSamples};
use crate::io::{encode, iso_gain_map, staged};
use crate::pipeline::chain;
use crate::pipeline::fit_gamut::DestinationGamut;
use crate::pipeline::fit_range;
use crate::pipeline::input_semantics::{
    self, ContainerColorFacts, InputAssertions, InputColorReport, RawMode,
};
use crate::pipeline::memory::{self, MemoryReport, RunProfile, SamplePlan};
use crate::pipeline::midtone_neutral::MidtoneLine;
use crate::pipeline::working_space::AcesCgImage;
use crate::pipeline::{
    color, correction_confidence, film_base, gain_encode, gain_ratio, hdr, look, midtone_neutral,
    roll_white, scene_correction, working_space,
};
use crate::recipe::{self, KnobNames, Recipe};
use crate::stage::{StageClock, StageKind};
use crate::stdio::{self, Delivery};
use crate::telemetry;
use crate::types::{
    DEFAULT_MEASURE_INSET, EncodeOutcome, EncodeReport, FilmBase, FilmBaseProvenance,
    FilmBaseSource, FilmType, InputParams, LinearImage, MeaningAssertion, MeasureParams, NcError,
    OutputStats, REMOVED_SIMPLE_RECONSTRUCTION, Result, TransferAssertion, check_measure_inset,
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
    /// Measure the film base (Dmin) alone, from one frame or a region; emit JSON, and
    /// write it as a recipe with --out.
    MeasureBase(MeasureBaseArgs),
    /// Removed: renamed `measure-base`. Hidden, and kept only to emit a migration error.
    #[command(hide = true, disable_help_flag = true)]
    Estimate(RemovedCommandArgs),
    /// Measure what a roll shares — its white balance, midtone line, white and exposure,
    /// and with --unexposed its film base — once; emit JSON, and write it as a recipe with
    /// --out.
    MeasureRoll(MeasureRollArgs),
    /// Print the full default parameter set as JSON (recipe scaffolding).
    Params(ParamsArgs),
    /// Manage opt-in upload of anonymous `convert` telemetry: enable, disable, status,
    /// preview, flush, purge.
    Telemetry(TelemetryArgs),
}

/// `hanten telemetry` options.
#[derive(Args, Debug)]
pub struct TelemetryArgs {
    #[command(subcommand)]
    pub command: TelemetryCommand,
}

#[derive(Subcommand, Debug)]
pub enum TelemetryCommand {
    /// Show what is uploaded and, once confirmed, upload every `convert`'s event from
    /// now on, plus the current-schema events already in the selected queue.
    Enable {
        /// The queue (a JSONL log) to collect into and upload from [default:
        /// NC_TELEMETRY_LOG, else the platform data dir's nc/telemetry.jsonl].
        #[arg(long, value_name = "PATH")]
        queue: Option<PathBuf>,
        /// Confirm without a prompt (required when stdin is not a terminal).
        #[arg(long)]
        yes: bool,
    },
    /// Stop collecting and uploading. Waits for an upload in flight; keeps the queue.
    Disable,
    /// Print consent, queue size and the last upload outcome as JSON.
    Status,
    /// Print the request bodies that would be uploaded now, one per line, unsent.
    Preview,
    /// Upload the queue now, in the foreground, and print what happened as JSON.
    Flush,
    /// Delete every queued record of the selected queue. Only while disabled.
    Purge {
        /// Confirm without a prompt (required when stdin is not a terminal).
        #[arg(long)]
        yes: bool,
    },
    /// The detached upload helper.
    #[command(hide = true)]
    UploadOnce {
        #[arg(long)]
        generation: String,
    },
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

/// A removed subcommand's arguments: anything, so the migration error is what the user
/// sees rather than a parse error about a flag.
#[derive(Args, Debug)]
pub struct RemovedCommandArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub rest: Vec<std::ffi::OsString>,
}

/// Where a measuring command writes its recipe (`docs/design/roll-workflow.md`).
#[derive(Args, Debug, Default)]
pub struct RecipeOutArgs {
    /// Write the measurement as a recipe (`"recipe_version": 3`) to PATH, for
    /// `--params`. Refused if PATH exists, unless --force. Not written when the run
    /// fails, including under --strict.
    #[arg(long = "out", value_name = "PATH")]
    pub out: Option<PathBuf>,
    /// Replace an existing --out file.
    #[arg(long, requires = "out")]
    pub force: bool,
}

/// `measure-base`: an input scan, the film-base source flags (so the
/// calibrate-once-from-a-reference workflow works, design-spec §8), and reporting
/// controls.
#[derive(Args, Debug)]
pub struct MeasureBaseArgs {
    /// Input negative scan (SilverFast HDR/HDRi TIFF). With no source flag, an
    /// unexposed frame: the base is the median over its effective area.
    pub input: PathBuf,
    /// Retired: the five-cell grid (`film-base/holder-masked-measurement`). Hidden,
    /// and kept only to emit a migration error.
    #[arg(long, hide = true)]
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
    /// Treat estimation warnings (a non-uniform area or `--base-region`, decode
    /// notes, …) as a hard error. `measure-base` produces the
    /// `Dmin` a roll is calibrated on, so a script baking the result into a
    /// recipe wants a plausible-looking-but-bad base to fail loudly rather than
    /// be echoed back.
    #[arg(long)]
    pub strict: bool,
    #[command(flatten)]
    pub out: RecipeOutArgs,
    #[command(flatten)]
    pub memory: MemoryArgs,
    #[command(flatten)]
    pub report: ReportArgs,
}

/// `measure-roll --midtone-neutral`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum MidtoneMode {
    Auto,
    On,
    Off,
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
    /// 0.4–1.3 stops) — a frame it empties is left out of the exposure too — and a frame
    /// whose white comes within 0.5 stop of it warns as near film saturation. Without it the run warns and nothing is checked for saturation,
    /// and `--strict` refuses before decoding anything.
    #[arg(long, value_name = "PATH")]
    pub leader: Option<PathBuf>,
    /// The roll's recipe (`"recipe_version": 3`): the film base and
    /// the decode the gains, the white and the exposure are measured under. Its
    /// `scene_correction` and `look` values are not read — this command measures the white
    /// balance, the midtone line, the white and the exposure — though the recipe must still
    /// load (a retired or unknown key there is
    /// refused). Repeatable, and `-` reads stdin, as on `convert`. `--out` always writes the
    /// decode (`reconstruction`), since the gains hold only under it, and the recipe's
    /// `input` and `measure` keys when stated.
    #[arg(long = "params", value_name = "JSON")]
    pub recipe_in: Vec<PathBuf>,
    /// The roll's unexposed frame: measure the film base from it first — the median
    /// over its effective area, as `hanten measure-base` does with no source flag — and
    /// decode every frame with it.
    /// Refused beside `--film-base` or a recipe stating `calibration.film_base`.
    #[arg(long, value_name = "PATH")]
    pub unexposed: Option<PathBuf>,
    /// The roll's film base (Dmin) as `R,G,B`, over the recipe's. Required one way or
    /// the other — this, the recipe, or `--unexposed` — and explicit: a base estimated
    /// per frame would measure each frame under a different decode.
    #[arg(long = "film-base", value_name = "R,G,B", value_parser = parse_rgb)]
    pub film_base: Option<[f32; 3]>,
    #[command(flatten)]
    pub measure: MeasureOverrides,
    /// Write no small lift, a taste adjustment: a low-key frame renders at the roll's
    /// exposure. Still reported (`frames[].lift_ev`); `convert --small-lift off` turns a
    /// written one off without re-measuring.
    #[arg(long)]
    pub no_small_lift: bool,
    /// The midtone neutral, a correction: `auto` (the default) writes the roll's midtone
    /// line when the roll has 10 frames or more and enough bands of brightness voted;
    /// `on` writes it on a shorter roll of 3 or more frames, if enough bands count; `off`
    /// writes none, and measures the whites without it. `convert --midtone-neutral off`
    /// turns a written one off without re-measuring, the whites still measured after it.
    #[arg(
        long,
        value_enum,
        ignore_case = true,
        default_value = "auto",
        value_name = "AUTO|ON|OFF"
    )]
    pub midtone_neutral: MidtoneMode,
    /// Removed: it left out both lifts. Hidden, kept only for its migration error.
    #[arg(long = "no-frame-lift", hide = true)]
    pub removed_no_frame_lift: bool,
    /// Write no thin-frame lift, a taste adjustment: a thin frame — its white low after the
    /// roll's exposure, its shadows on the film base — renders its small lift instead of the
    /// steeper slope and more exposure that raise its white a stop with the base held. Still
    /// reported (`frames[].thin_lift`); `convert --thin-lift off` turns a written one off.
    #[arg(long)]
    pub no_thin_lift: bool,
    /// Treat warnings (a capped holder march, decode notes) as a hard error, and
    /// refuse to measure without `--leader`.
    #[arg(long)]
    pub strict: bool,
    #[command(flatten)]
    pub out: RecipeOutArgs,
    #[command(flatten)]
    pub memory: MemoryArgs,
    #[command(flatten)]
    pub report: ReportArgs,
}

/// `convert`: input, output, and every conversion knob (design-spec §9).
#[derive(Args, Debug)]
pub struct ConvertArgs {
    /// Input negative scan (SilverFast HDR/HDRi TIFF).
    pub input: PathBuf,
    /// Output positive path. The suffix is optional: leave it off and Hanten
    /// appends the resolved destination's container (see --container). A stated
    /// suffix is never rewritten, so it must be one that destination writes.
    #[arg(short = 'o', long, value_name = "PATH")]
    pub output: PathBuf,
    #[command(flatten)]
    pub knobs: ConversionFlags,

    /// A JSON recipe, or `-` for stdin. Repeatable: the recipes layer in order, a
    /// later one winning key by key (a `null` states nothing), and individual
    /// `--flag`s win over them all.
    #[arg(long = "params", value_name = "JSON")]
    pub recipe_in: Vec<PathBuf>,
    /// Write the effective (resolved) parameters to JSON, once the run has succeeded.
    #[arg(long, value_name = "JSON")]
    pub dump_params: Option<PathBuf>,
    /// Treat warnings (clipping, no roll measurement, …) as hard errors.
    #[arg(long)]
    pub strict: bool,
    /// Fix any stochastic step for reproducibility (none in Step 1; reserved).
    #[arg(long, value_name = "N")]
    pub seed: Option<u64>,
    /// Also write the fixed decode's film RGB — before the NC film RGB v1 3×3 — as an
    /// untagged 32-bit float TIFF: the dye layers' values, for measuring the decode.
    /// Operational flag — not a recipe key; never affects the output image.
    #[arg(long, value_name = "PATH")]
    pub export_film_rgb: Option<PathBuf>,
    /// Also write what the destination's encoder receives — the linear rendition before
    /// any transfer, and a gain map's codes before its JPEG — as an untagged TIFF, one
    /// page per buffer: what an independent decode of the output is checked against.
    /// Operational flag — not a recipe key; never affects the output image.
    #[arg(long, value_name = "PATH")]
    pub export_pre_encode: Option<PathBuf>,

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

    #[command(flatten)]
    pub memory: MemoryArgs,
    #[command(flatten)]
    pub report: ReportArgs,
}

/// Every conversion knob as a flag, plus the retired ones kept hidden only to be
/// refused (`reject_removed_flags`). Shared by `convert` and `roll`, so the two cannot
/// grow different surfaces.
///
/// Stage knobs are grouped into flattened `*Overrides` structs; each field is an
/// `Option` (or a presence flag) so [`recipe::merge`] can tell "explicitly passed"
/// from "left at the recipe / default value".
#[derive(Args, Debug, Default)]
pub struct ConversionFlags {
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
    pub removed_roll: RemovedRollFlags,
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

    /// Removed: see [`reject_new_flow`].
    #[arg(long = "new-flow", hide = true)]
    pub new_flow: bool,
}

/// `hanten roll`: convert a batch of frames from ONE shared, frozen recipe so the
/// whole roll is color-consistent and reproducible (design-spec §8, §12 item 6).
///
/// This is the batch-**apply** half of plan→recipe→apply: it replays a *provided*
/// frozen recipe — resolved like `convert`'s, from `--params` layers and the same
/// flags — over N frames. It deliberately owns no auto-cascade that *generates* the
/// recipe — that is `core/auto-calibration`. Frame-local params can be overridden per
/// frame via a `--frames` manifest.
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
    /// The roll's recipe, or `-` for stdin. Repeatable, as on `convert`: the recipes
    /// layer in order — typically the measured file (`calibration`, `roll`) and a look
    /// — and individual `--flag`s win over them all. A frame's manifest `params` wins
    /// over both.
    #[arg(long = "params", value_name = "JSON")]
    pub recipe_in: Vec<PathBuf>,
    #[command(flatten)]
    pub knobs: ConversionFlags,
    /// Also write each frame's film RGB, as `convert --export-film-rgb` does, to
    /// `<input-stem>_film-rgb.tiff` beside that frame's output. Takes no path, so put
    /// it after the inputs. Operational flag — not a recipe key; never affects the
    /// output image.
    #[arg(long, num_args = 0..=1, value_name = "NONE")]
    pub export_film_rgb: Option<Option<PathBuf>>,
    /// Treat any frame's warnings as a hard error (after the roll report is
    /// emitted), like `convert --strict`.
    #[arg(long)]
    pub strict: bool,
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
    #[arg(long, value_name = "RANGE")]
    pub range: Option<Range>,
    /// How samples are stored: `native` (the gamut's own display curve — the
    /// default), `linear` (no transfer; a 32-bit float TIFF), `pq` or `hlg` (Rec.2100
    /// signals). Recipe key `output.display.transfer`.
    #[arg(long, value_name = "TRANSFER")]
    pub transfer: Option<Transfer>,
    /// The primaries to render into: `display-p3` (the default), `adobe-rgb`, `srgb`
    /// or `bt2020` (HDR only). Recipe key `output.display.gamut`.
    #[arg(long, value_name = "GAMUT")]
    pub gamut: Option<Gamut>,
    /// The file container: `tiff` (the default) or `jpeg` (HDR with a gain map).
    /// Destinations: SDR `native` TIFF in Display P3, Adobe RGB or sRGB; HDR as a
    /// `linear` float TIFF in any gamut (stated with `--gamut`), or BT.2020 `pq`/`hlg` in
    /// a 16-bit TIFF; HDR as a JPEG with an ISO 21496-1 gain map on a Display P3
    /// (`--range hdr` alone) or sRGB base. An SDR JPEG is not written yet. Recipe key
    /// `output.display.container`.
    #[arg(long, value_name = "CONTAINER")]
    pub container: Option<Container>,
    /// Write the fixed decode's linear ACEScg, unclamped 32-bit float TIFF, with no
    /// rendering stage (recipe `output`: `"film-master"`). Refuses a rendering stage
    /// the recipe or flags ask for (scene correction, the look, fit range), and the
    /// roll flags (`--roll-white-balance`, `--roll-white`, `--roll-exposure`,
    /// `--roll-frame-exposure`, `--roll-thin-slope`, `--roll-thin-exposure`,
    /// `--roll-midtone-line`, `--small-lift on`, `--thin-lift on`, `--midtone-neutral on`,
    /// `--neutral-balance on`), which only a rendering applies — refused under a recipe's film master too. A
    /// recipe's `roll` section is spared, since a measurement is not a stage asked for,
    /// and so is a switch turned off.
    #[arg(
        long = "film-master",
        conflicts_with_all = [
            "range", "transfer", "gamut", "container", "roll_white_balance", "roll_white",
            "roll_exposure", "roll_frame_exposure", "roll_thin_slope", "roll_thin_exposure",
            "roll_midtone_line",
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
    /// Retired: the IR plane is no longer exported. Hidden, and kept only to emit a
    /// migration error.
    #[arg(long, hide = true, num_args = 0..=1, default_missing_value = "", allow_hyphen_values = true)]
    pub export_ir: Option<String>,
}

/// Film-base / Dmin overrides (design-spec §9, stage 2).
///
/// The two source flags are mutually exclusive (clap rejects passing both); either
/// replaces the recipe's `calibration.film_base` entirely. `convert` and `roll` require
/// one of them **or** the recipe key, because `Dmin` sets black point and colour
/// balance together.
#[derive(Args, Debug, Default)]
pub struct FilmBaseOverrides {
    /// Explicit per-channel base transmission — a `Dmin` measured once per roll
    /// (`hanten measure-base` on the unexposed frame).
    #[arg(long, value_name = "R,G,B", value_parser = parse_rgb,
          conflicts_with = "base_region")]
    pub film_base: Option<[f32; 3]>,
    /// A region of unexposed film to read the base from, at its 97th percentile.
    #[arg(long, value_name = "X,Y,W,H", value_parser = parse_region)]
    pub base_region: Option<[u32; 4]>,
    /// Retired with the rebate search (`film-base/holder-masked-measurement`).
    /// Hidden, and kept only to emit a migration error.
    #[arg(long, hide = true)]
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
    /// with mid-grey pinned — a slope of `log2(1/0.18) / STOPS` — which `--contrast`
    /// multiplies, unless a thin lift applies (`--roll-thin-slope`). It drops a frame's
    /// thin lift (slope and exposure).
    #[arg(long, value_name = "STOPS")]
    pub roll_white: Option<f32>,
    /// The roll's exposure in EV, as `hanten measure-roll` measured it (recipe key
    /// `roll.exposure`): a neutral gain that brings the roll's median frame to a normal
    /// level. Added to `--exposure`, which then adjusts it rather than replacing it.
    #[arg(long, value_name = "EV", allow_hyphen_values = true)]
    pub roll_exposure: Option<f32>,
    /// This frame's small lift in EV, added to `--roll-exposure` (recipe key
    /// `roll.frame_exposure`; a `roll.frames` entry's `exposure`): the lift `hanten
    /// measure-roll` gives a low-key frame. A thin lift replaces it.
    #[arg(long, value_name = "EV", allow_hyphen_values = true)]
    pub roll_frame_exposure: Option<f32>,
    /// Whether the small lift applies: `on` (the default) or `off`, which keeps it in the
    /// recipe (recipe key `roll.small_lift`). A taste adjustment.
    #[arg(long, value_enum, ignore_case = true, value_name = "ON|OFF")]
    pub small_lift: Option<recipe::Switch>,
    /// A thin frame's own slope, in place of the one `--roll-white` places (recipe key
    /// `roll.thin_slope`; a `roll.frames` entry's `thin_slope`): the steeper slope `hanten
    /// measure-roll` gives a thin frame. `--contrast` multiplies it.
    #[arg(long, value_name = "SLOPE")]
    pub roll_thin_slope: Option<f32>,
    /// The exposure in EV solved with `--roll-thin-slope`, added to `--roll-exposure` in
    /// place of the small lift (recipe key `roll.thin_exposure`; a `roll.frames` entry's
    /// `thin_exposure`). Applies only beside the thin slope.
    #[arg(long, value_name = "EV", allow_hyphen_values = true)]
    pub roll_thin_exposure: Option<f32>,
    /// Whether the thin lift applies: `on` (the default) or `off`, which renders a thin
    /// frame with its small lift and keeps the thin one in the recipe (recipe key
    /// `roll.thin_lift`). A taste adjustment.
    #[arg(long, value_enum, ignore_case = true, value_name = "ON|OFF")]
    pub thin_lift: Option<recipe::Switch>,
    /// The roll's midtone line as `hanten measure-roll` measured it (recipe key
    /// `roll.midtone_line`): red's and blue's slope and offset, the lowest and highest
    /// voted band, and the scene stop where the correction fades to zero. It removes the
    /// cast a poor development leaves in the midtones, keyed on `--roll-white-balance`.
    #[arg(
        long,
        value_name = "RS,RO,BS,BO,LO,HI,END",
        value_parser = parse_midtone_line,
        allow_hyphen_values = true
    )]
    pub roll_midtone_line: Option<MidtoneLine>,
    /// Whether the midtone line applies: `on` (the default) or `off`, which keeps it in
    /// the recipe (recipe key `roll.midtone_neutral`) and the whites measured after it. A
    /// correction with a switch, since a roll dominated by one scene colour can mislead it.
    #[arg(long, value_enum, ignore_case = true, value_name = "ON|OFF")]
    pub midtone_neutral: Option<recipe::Switch>,
    /// Whether the roll's white balance applies: `on` (the default) or `off`, which keeps
    /// the gains in the recipe (recipe key `roll.neutral_balance`) and takes the midtone
    /// line with them, since it is measured after them (a typed `--midtone-neutral on` is
    /// refused beside it); the whites stay as measured. A correction with a switch, since a
    /// roll dominated by one scene colour can mislead it.
    #[arg(long, value_enum, ignore_case = true, value_name = "ON|OFF")]
    pub neutral_balance: Option<recipe::Switch>,
}

impl RollOverrides {
    /// The typed roll flags, by name.
    pub(crate) fn typed(&self) -> Vec<&'static str> {
        [
            ("--roll-white-balance", self.roll_white_balance.is_some()),
            ("--roll-white", self.roll_white.is_some()),
            ("--roll-exposure", self.roll_exposure.is_some()),
            ("--roll-frame-exposure", self.roll_frame_exposure.is_some()),
            ("--roll-thin-slope", self.roll_thin_slope.is_some()),
            ("--roll-thin-exposure", self.roll_thin_exposure.is_some()),
            // `off` asks for nothing, so no branch refuses it.
            (
                "--small-lift on",
                self.small_lift == Some(recipe::Switch::On),
            ),
            ("--thin-lift on", self.thin_lift == Some(recipe::Switch::On)),
            ("--roll-midtone-line", self.roll_midtone_line.is_some()),
            (
                "--midtone-neutral on",
                self.midtone_neutral == Some(recipe::Switch::On),
            ),
            (
                "--neutral-balance on",
                self.neutral_balance == Some(recipe::Switch::On),
            ),
        ]
        .into_iter()
        .filter_map(|(flag, typed)| typed.then_some(flag))
        .collect()
    }

    /// Whether any roll flag was typed.
    pub(crate) fn any(&self) -> bool {
        !self.typed().is_empty()
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
    /// the rest of the picture. Added to the roll's exposure (`--roll-exposure`, which
    /// `hanten measure-roll` measures), so it adjusts the roll's rather than replacing it.
    #[arg(long, allow_hyphen_values = true)]
    pub exposure: Option<f32>,
}

/// Look overrides (recipe section `look`).
#[derive(Args, Debug, Default)]
pub struct LookOverrides {
    /// Contrast, as a multiplier on the base slope (recipe key `look.contrast`; 1 keeps
    /// the base): 1.2 is 20% more contrast than the roll's, 0.9 is flatter. The base is
    /// a thin frame's slope (`--roll-thin-slope`), else the roll's (`--roll-white`), else the fallback ≈ 1.41, as if the roll's white
    /// were 1.75 stops up (`direct`: its pinned ≈ 1.41). Luminance only: each pixel's
    /// ACEScg luminance becomes `0.18 · (Y / 0.18)^slope`, pivoted at mid-grey, and its
    /// colour is kept (`--saturation` sets that); slope 1 reproduces the scene's own
    /// contrast. Runs after scene correction, so an `--exposure` is expanded with the
    /// rest of the picture.
    #[arg(long, value_name = "CONTRAST", allow_hyphen_values = true)]
    pub contrast: Option<f32>,
    /// Saturation, as a multiplier on the default colour (recipe key `look.saturation`; 1
    /// keeps it): each pixel's colour ratios are raised to the power
    /// `base × 1.15 × SATURATION` with its luminance kept (`direct`: × 1 in place of 1.15).
    /// The base is `--contrast`'s, except a thin frame's slope (`--roll-thin-slope`), which
    /// never reaches colour; so colour does not move with `--contrast`.
    #[arg(long, value_name = "SATURATION", allow_hyphen_values = true)]
    pub saturation: Option<f32>,
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
    /// the whole slope, `--density-gamma` times the look's slope (recipe key
    /// `look.highlight_desaturation.band`, default `0.015,0.025`).
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
    /// stops: where the base lands before this is set by the look's slope, and a
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

/// `--roll-frame-slope` and `--frame-lift`, retired when a thin frame's lift became its own
/// pair with its own switch (`nf-calibration/taste-vs-quality`). Hidden, kept only for
/// their migration errors.
#[derive(Args, Debug, Default)]
pub struct RemovedRollFlags {
    #[arg(long, hide = true, value_name = "SLOPE", num_args = 0..=1, default_missing_value = "", allow_hyphen_values = true)]
    pub roll_frame_slope: Option<String>,
    #[arg(long = "frame-lift", hide = true, value_name = "ON|OFF", num_args = 0..=1, default_missing_value = "")]
    pub removed_frame_lift: Option<String>,
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

/// The reuse-ready forms of a measured film base, both present or both absent: the
/// flag for a single `convert`, and the recipe value `--out` writes.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReuseReady {
    /// Ready-to-paste `--film-base R,G,B` flag for the measured base; the values
    /// round-trip to the exact measured `f32`s.
    #[serde(rename = "film_base_flag")]
    pub flag: String,
    /// The same measurement as the `calibration.film_base` value `--out` writes.
    #[serde(skip)]
    pub source: FilmBaseSource,
}

/// What the `hdr-linear-tiff` encoder wrote, and the luminance semantics the file
/// cannot state for itself. Serialize-only.
///
/// **This block is authoritative for the HDR semantics, and deliberately so.** The
/// embedded ICC profile describes the colorimetry (the gamut's primaries, D65, a linear
/// TRC) but its PCS stops at the media white, so no v4 profile can express that
/// `1.0` is 203 cd/m² and that highlights legitimately run to
/// `linear_headroom`. Anything consuming these files for luminance must read this,
/// not the profile — `interoperability` says so in the artifact itself rather than
/// leaving it to documentation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct HdrLinearTiffResult {
    /// Stable identifier of the pixel contract
    /// ([`hdr::LinearLabels::pixel_contract`]).
    pub pixel_contract: &'static str,
    /// Bits per sample as written (32).
    pub bits_per_sample: u16,
    /// TIFF `SampleFormat` as written (3 = IEEE float).
    pub sample_format: u16,
    /// Whether the file was written as BigTIFF.
    pub bigtiff: bool,
    /// Size of the embedded linear ICC profile, in bytes.
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
    /// This frame's **measured** CTA-861.3 MaxCLL / MaxFALL in cd/m²
    /// ([`hdr::ContentLightLevel`]), not the mastering policy above.
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
    /// MatrixCoefficients is 0: ICC.1:2022 §10.3 mandates it for an RGB data space.
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
    /// This frame's **measured** CTA-861.3 MaxCLL / MaxFALL in cd/m²
    /// ([`hdr::ContentLightLevel`]).
    ///
    /// PQ only: they are absolute light levels, which HLG — a relative signal whose
    /// peak the display decides — cannot state. TIFF has no content-light tag, so
    /// this is the only place a consumer learns the image's actual peak.
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
    /// The look's controls as applied: the `contrast` multiplier, the base slope it
    /// multiplied and where that came from, the resulting `slope`, the grade and highlight
    /// desaturation. Absent for the film master.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub look: Option<recipe::LookReport>,
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
    /// The subcommand that produced this report (`convert`/`inspect`/`measure-base`).
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
    /// The resolved recipe (`convert`): the `params` of a `--dump-params` file, so it
    /// reloads through `--params` to this run. `identity.params_hash` hashes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipe: Option<Recipe>,
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
    /// Where the film base came from ([`FilmBaseProvenance`]): the effective area, a
    /// stated region, or an explicit value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub film_base_source: Option<FilmBaseProvenance>,
    /// The per-channel percentile the base was read at: `0.5` over the effective
    /// area, `0.97` over a stated region. Absent for an explicit base, which reads
    /// no pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub film_base_percentile: Option<f32>,
    /// The declared film chemistry, echoed back. It gates nothing
    /// (`ir-usability-detection`); it is recorded so a declaration a user made is
    /// visible in the artifact the run produced — without it the flag would be parsed
    /// and dropped, accepted-and-ignored, which this project treats as a bug.
    /// `inspect` / `measure-base` echo the `--film-type` flag; `convert` (and each `roll`
    /// frame, as `FrameStatus::Ok::film_type`) echo the resolved recipe's
    /// `input.film_type`. Omitted for `unknown` (the default), even when stated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub film_type: Option<FilmType>,
    /// The measured IR usability verdict (`inspect` / `measure-base`, only on a scan
    /// carrying an IR plane): the interior IR transmission and whether it clears the bar for
    /// telling the opaque holder from film on **this frame**. Reported so the
    /// verdict — and the threshold behind it — is falsifiable from a run rather
    /// than only from the source (`ir-usability-detection`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ir_separability: Option<film_base::IrSeparability>,
    /// The resolved **effective measurement area** (every command that decodes —
    /// `inspect`, `measure-base`, `convert`, and each `roll` frame via
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
    /// Reuse-ready forms of the measured base (`measure-base`), only when it is usable
    /// as an explicit base (each channel in `(0, 1]`). Flattened, so the flag is the
    /// top-level key `film_base_flag`; the recipe half is what `--out` writes.
    #[serde(flatten)]
    pub reuse: Option<ReuseReady>,
    /// Path the film RGB was exported to, when `--export-film-rgb` was given: the
    /// fixed decode's output before the NC film RGB v1 3×3, as an f32 TIFF with no
    /// ICC profile. Its channels are the dye layers, not a colour space.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub film_rgb_exported: Option<PathBuf>,
    /// Path the encoder's input buffers were exported to, when `--export-pre-encode`
    /// was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_encode_exported: Option<PathBuf>,
    /// Encode-time sample loss (clipped / non-finite counts), for `convert`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loss: Option<EncodeReport>,
    /// Per-channel mean of the samples as written (`convert`) — the comparison
    /// basis `nctool compare` diffs across two builds (per-channel mean ΔRGB is
    /// the difference of these means). Report-only, like `loss`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_stats: Option<OutputStats>,
    /// Non-fatal warnings (clipping, no roll measurement, …).
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

/// Parse `--roll-midtone-line`'s seven numbers, in [`MidtoneLine`]'s field order.
fn parse_midtone_line(s: &str) -> std::result::Result<MidtoneLine, String> {
    let [rs, ro, bs, bo, lo, hi, end] = parse_floats::<7>(s)?;
    Ok(MidtoneLine {
        red: [rs, ro],
        blue: [bs, bo],
        bands: [lo, hi],
        fade_end_stops: end,
    })
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
    /// One per layer whose envelope records another `pipeline_version`
    /// ([`pipeline_version_warning`]). Provenance only — never applied.
    provenance_warnings: Vec<String>,
}

/// The envelope every recipe document hanten writes is wrapped in — `--dump-params`,
/// `hanten params`, `measure-base --out`, `measure-roll --out` — so a replay on another
/// `pipeline_version` warns ([`pipeline_version_warning`]). `params` is last, so the
/// recipe's text is [`Recipe::params_hash`]'s input indented two spaces deeper.
#[derive(Serialize)]
struct RecipeEnvelope<'a, T> {
    meta: Identity,
    params: &'a T,
}

impl<'a, T> RecipeEnvelope<'a, T> {
    /// `params` stamped with this build's identity.
    fn new(params: &'a T) -> Self {
        Self {
            meta: Identity::new(),
            params,
        }
    }
}

/// The read side of [`RecipeEnvelope`] (and of the sidecars builds before
/// `pipeline_version` 8 wrote): identity beside the recipe, never inside it, since every
/// recipe struct is `deny_unknown_fields`. `meta` is kept as a raw `Value` on purpose: it is
/// provenance, so an older build must not reject a newer build's extra `meta` fields,
/// and nothing in it may influence the conversion. `params` is likewise raw here so the
/// *identical* body checks (migration errors, the typed `deny_unknown_fields` parse)
/// apply to an enveloped and a bare recipe alike. `deny_unknown_fields` at this level
/// keeps a third sibling key from being silently ignored.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipeEnvelopeIn {
    #[serde(default)]
    meta: Option<serde_json::Value>,
    params: serde_json::Value,
}

/// The `--params` value that reads the recipe from stdin.
const STDIN_RECIPE: &str = "-";

fn is_stdin_recipe(p: &Path) -> bool {
    p.as_os_str() == STDIN_RECIPE
}

/// The `--params` files a run reads from disk — every layer but stdin — which no write
/// target may overwrite.
fn recipe_files(paths: &[PathBuf]) -> impl Iterator<Item = &PathBuf> {
    paths.iter().filter(|p| !is_stdin_recipe(p))
}

/// Load the `--params` layers, in order, into one recipe; the defaults when there are
/// none. Each layer is checked on its own first ([`read_layer`]), so a fault is named
/// against its file, and then they compose, later winning ([`recipe::compose`]).
///
/// `-` reads stdin, and only once: a second `-` is refused rather than read as empty.
fn load_recipes(paths: &[PathBuf]) -> Result<LoadedRecipe> {
    if paths.iter().filter(|p| is_stdin_recipe(p)).count() > 1 {
        return Err(NcError::Usage(
            "--params - is given more than once, but stdin can be read only once: save \
             the other recipe to a file and pass its path"
                .into(),
        ));
    }
    let mut layers = Vec::with_capacity(paths.len());
    let mut provenance_warnings = Vec::new();
    for p in paths {
        let (txt, name) = if is_stdin_recipe(p) {
            let txt = std::io::read_to_string(std::io::stdin())
                .map_err(|e| NcError::Usage(format!("cannot read the recipe on stdin: {e}")))?;
            (txt, "on stdin (--params -)".to_string())
        } else {
            let txt = std::fs::read_to_string(p)
                .map_err(|e| NcError::Usage(format!("cannot read recipe {}: {e}", p.display())))?;
            (txt, p.display().to_string())
        };
        let layer = read_layer(&txt, &name)?;
        provenance_warnings.extend(pipeline_version_warning(layer.meta_pipeline_version, &name));
        layers.push(layer.body);
    }
    let recipe = recipe::compose(&layers).map_err(|e| {
        NcError::Usage(format!(
            "the --params recipes each load, but not together: {e}"
        ))
    })?;
    Ok(LoadedRecipe {
        recipe,
        provenance_warnings,
    })
}

/// One `--params` layer as read: its recipe body, and `meta.pipeline_version` when the
/// file is an envelope.
struct Layer {
    body: serde_json::Value,
    meta_pipeline_version: Option<u32>,
}

/// Check one recipe document on its own, named `recipe {name}` in every message.
/// Invalid or unknown-key JSON is a usage error; a document written for the removed
/// chain, and keys that retired before it, get migration errors naming where each knob
/// went ([`recipe::check_body`]) rather than opaque serde messages.
///
/// Accepts **both** shapes: the envelope `{ "meta": …, "params": {…recipe…} }` —
/// identity read for provenance and otherwise ignored — and a bare recipe object (a
/// hand-written recipe, or a report's `recipe`). The two are told apart by the
/// presence of a top-level `params` key, which is not (and must never become) a recipe
/// key.
fn read_layer(txt: &str, name: &str) -> Result<Layer> {
    // Parse to a raw Value first to pick the shape and to run the migration checks on
    // the recipe *body*; the typed parse below still owns shape and unknown-key
    // validation. Unparseable JSON falls through to the typed parse's error (its
    // message names the recipe).
    let mut value: Option<serde_json::Value> = serde_json::from_str(txt).ok();
    let context = format!("recipe {name}");
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
    let envelope = split_envelope(value.as_ref(), &context)?;
    let (mut envelope_body, meta_pipeline_version) = match envelope {
        Some((body, meta_version)) => (Some(body), meta_version),
        None => (None, None),
    };
    // The recipe *body*: an envelope's `params`, else the whole document.
    let mut stripped = false;
    if let Some(v) = envelope_body.as_mut().or(value.as_mut()) {
        stripped = recipe::strip_retired_nulls(v);
        recipe::check_body(v, true, &context)?;
    }
    let usage = |e| NcError::Usage(format!("invalid recipe {name}: {e}"));
    // The typed parse, for its diagnostics: from the Value when it differs from the text
    // (an envelope's `params`, or stripped keys), else from the text, so a bare recipe's
    // line/column-bearing serde errors survive.
    let body = match envelope_body.or_else(|| value.take_if(|_| stripped)) {
        Some(v) => {
            serde_json::from_value::<Recipe>(v.clone()).map_err(usage)?;
            v
        }
        None => {
            serde_json::from_str::<Recipe>(txt).map_err(usage)?;
            value.expect("text that parses as a recipe parses as JSON")
        }
    };
    Ok(Layer {
        body,
        meta_pipeline_version,
    })
}

/// Split a loaded document into `(recipe body JSON, meta.pipeline_version)` when it
/// is an envelope; `None` when it is a bare recipe and the caller should use the file
/// text as-is.
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
                "{context}: has a `meta` block but no `params` — an envelope \
                 is `{{\"meta\": {{…}}, \"params\": {{…recipe…}}}}`; a bare recipe \
                 object must not contain `meta`"
            )));
        }
        return Ok(None);
    }
    let envelope: RecipeEnvelopeIn = serde_json::from_value(value.unwrap().clone())
        .map_err(|e| NcError::Usage(format!("{context}: invalid recipe envelope: {e}")))?;
    if !envelope.params.is_object() {
        return Err(NcError::Usage(format!(
            "{context}: envelope `params` must be a recipe OBJECT, got {}. A non-object \
             `params` would convert with all-default parameters instead of the recipe \
             this file claims to carry",
            json_kind(&envelope.params)
        )));
    }
    // `meta`, when the document has the key at all, must be an OBJECT. Checked
    // against the raw JSON rather than `envelope.meta`, because serde folds
    // `"meta": null` into the same `None` an omitted key produces — and an omitted
    // `meta` is legal (a bare recipe wrapped by hand).
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
            "{context}: envelope `meta` must be an object, got {}. A malformed `meta` \
             carries no readable provenance, and treating it as absent would silently \
             skip the pipeline_version skew check this envelope exists to enable — \
             omit `meta` entirely if the recipe has no provenance to record",
            json_kind(meta)
        )));
    }
    let meta_pipeline_version = meta_pipeline_version(envelope.meta.as_ref(), context)?;
    Ok(Some((envelope.params, meta_pipeline_version)))
}

/// The `pipeline_version` recorded in an envelope's `meta`, when present.
///
/// Present-but-unreadable is a **loud error**, not `None`. `None` means "this file
/// records no version" and suppresses the skew check entirely, so silently mapping a
/// `1.0`, a `"1"`, or a negative number onto it would disable the very warning the
/// label exists to raise — a recipe round-tripped through a tool that emits `1.0`
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
fn pipeline_version_warning(loaded_version: Option<u32>, name: &str) -> Option<String> {
    let recorded = loaded_version?;
    (recorded != version::PIPELINE_VERSION).then(|| {
        format!(
            "recipe {name} was produced by pipeline_version {recorded}, but this build is \
             pipeline_version {} — the parameters still apply, but the default conversion \
             behavior changed between them, so the output will not match the original",
            version::PIPELINE_VERSION
        )
    })
}

/// Map the (clap-mutually-exclusive) film-base flags to a [`FilmBaseSource`],
/// or `None` when none was passed. Shared by `convert`'s merge ([`recipe::merge`]) and
/// `measure-base`, so they resolve the source identically.
pub(crate) fn film_base_source_override(o: &FilmBaseOverrides) -> Option<FilmBaseSource> {
    if let Some(v) = o.film_base {
        Some(FilmBaseSource::Explicit(v))
    } else {
        o.base_region.map(FilmBaseSource::Region)
    }
}

/// What retired with `--auto-base` and the recipe's `"auto"` (`recipe::check_body`),
/// and the measurement that replaces it. Each caller appends the remedy its surface
/// accepts.
pub(crate) const AUTO_BASE_RETIRED: &str = "the automatic film base searched each edge for a \
     thin unexposed rebate, and it retired with that search. Measure the roll's base once \
     from its unexposed frame with `hanten measure-base <unexposed-frame>`";

/// Validate that an explicit film base is a per-channel transmission in `(0, 1]`
/// — the one invariant that must hold wherever an explicit base enters (a recipe
/// via [`validate_shared`], or the `--film-base` flag on `measure-base`). Non-positive /
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

/// Refuse a typed `--midtone-neutral on` when the roll's white balance is off: the line is
/// measured after the gains and goes off with them. Only the typed `on` is refused, since a
/// recipe's `"on"` is the default. It runs before [`reject_roll_flags_nothing_applies`],
/// whose `--rendering default` remedy would meet it next; where nothing applies the roll,
/// dropping the `on` is the one remedy that works.
fn reject_a_line_without_its_gains(args: &ConversionFlags, r: &Recipe) -> Result<()> {
    use recipe::Switch::{Off, On};
    if args.roll.midtone_neutral != Some(On) || r.roll.neutral_balance != Some(Off) {
        return Ok(());
    }
    let nothing_applies =
        r.output == OutputSection::FilmMaster || r.rendering == crate::rendering::Rendering::Direct;
    let (off, other) = if args.roll.neutral_balance == Some(Off) {
        ("--neutral-balance off", "drop --neutral-balance off")
    } else {
        (
            "the recipe's `roll.neutral_balance` \"off\"",
            "pass --neutral-balance on",
        )
    };
    let remedy = if nothing_applies {
        "drop --midtone-neutral on".to_string()
    } else {
        format!("drop --midtone-neutral on, or {other}")
    };
    Err(NcError::Usage(format!(
        "--midtone-neutral on asks for the roll's midtone line, which is measured after its \
         white balance and goes off with it under {off}: {remedy}"
    )))
}

/// Refuse typed roll flags when nothing will apply them: a recipe's film master (a typed
/// `--film-master` conflicts at the parser) or a `direct` rendering. The film master is
/// named first. A presence rule, so it runs before `recipe::validate`, whose value rules
/// would otherwise refuse first with remedies that cannot work. A recipe's `roll` section
/// is spared.
fn reject_roll_flags_nothing_applies(args: &ConversionFlags, r: &Recipe) -> Result<()> {
    let direct = r.rendering == crate::rendering::Rendering::Direct;
    if !args.roll.any() || (r.output != OutputSection::FilmMaster && !direct) {
        return Ok(());
    }
    let flags = args.roll.typed();
    let typed = match flags.as_slice() {
        [.., last] if flags.len() > 1 => {
            format!("{} and {last}", flags[..flags.len() - 1].join(", "))
        }
        _ => flags.join(""),
    };
    let (them, apply) = if flags.len() > 1 {
        ("them", "apply")
    } else {
        ("it", "applies")
    };
    let message = if r.output == OutputSection::FilmMaster {
        // Under `direct` too, either remedy alone would meet the film master + `direct`
        // refusal next, so each carries `--rendering default`. Only a switch typed `on`
        // reaches here beside a typed `--film-master`; the other roll flags conflict.
        let (master, choose) = if args.destination.film_master {
            ("--film-master writes", "drop --film-master")
        } else {
            (
                "the recipe's `output` is \"film-master\", which writes",
                "choose a rendered destination (--range, --transfer, --gamut or --container)",
            )
        };
        let (drop, rendered) = if direct {
            (
                format!("drop {typed} and pass --rendering default"),
                format!("{choose} with --rendering default"),
            )
        } else {
            (format!("drop {typed}"), choose.to_string())
        };
        format!(
            "{typed} {apply} the roll's measurements through the rendering stages, but \
             {master} the fixed decode's linear ACEScg with no rendering stage and would \
             ignore {them}. Either {drop}, or {rendered}"
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
/// `roll` composes the same pieces itself (`validate_roll_recipe`): it has no `-o`.
pub fn validate_convert(r: &Recipe, args: &ConvertArgs) -> Result<()> {
    recipe::validate(r, KnobNames::FlagAndKey)?;
    resolve_output_path(
        &args.output,
        OutputTarget::resolve(r, KnobNames::FlagAndKey, args.knobs.destination.film_master)?,
        SuffixContext::Convert,
    )?;
    validate_shared(r)
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

impl SuffixContext<'_> {
    /// The prefix naming the frame a roll message is about; empty for `convert`.
    fn frame(self) -> String {
        match self {
            SuffixContext::RollFrame(input) => format!("frame {}: ", input.display()),
            SuffixContext::Convert => String::new(),
        }
    }
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
    // First: `out.tiff/` has a known suffix but names a directory, and would fail only
    // at the write, after the render.
    if let Some(reason) = Unappendable::of(given) {
        return Err(unappendable_error(
            reason,
            given,
            container.canonical(),
            context,
        ));
    }
    if let Some(why) = given.extension().and_then(Container::removed_suffix) {
        return Err(NcError::Usage(format!(
            "{}the output path {}: {why}. Name it `.{}`, or leave the suffix off and \
             Hanten supplies it",
            context.frame(),
            given.display(),
            container.canonical()
        )));
    }
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
        _ => Ok(append_suffix(given, container.canonical())),
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

/// The diagnosis for a path that names no file to write. Like
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
            "frame {}: the manifest's explicit output {} {}, not a file Hanten can write \
             the `.{ext}` output to — give the entry's `output` a file name, or drop its \
             `output` key to take the derived name inside the out-dir",
            input.display(),
            given.display(),
            reason.what()
        ),
        // `convert`, either provenance: the remedy is the path in `-o`. The directory
        // case also points at roll, because `--out-dir positives/` is where the
        // trailing separator comes from in the first place.
        _ => {
            let mut msg = format!(
                "the output path {} {}, not a file Hanten can write the `.{ext}` output \
                 to — give a path ending in a file name",
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
/// The caller has refused either [`Unappendable`] shape — completing a directory path
/// writes its sibling, which on `roll` puts the whole roll outside the `--out-dir` the
/// user named, at exit 0, with the report agreeing.
fn append_suffix(given: &Path, ext: &str) -> PathBuf {
    let mut completed = given
        .file_name()
        .expect("resolve_output_path refuses a path with no file name")
        .to_os_string();
    completed.push(".");
    completed.push(ext);
    given.with_file_name(completed)
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
    // A frame's path comes from its manifest entry, whose `params` win over every
    // flag, so the remedy names the recipe keys it can state there.
    let frame = context.frame();
    let names = match context {
        SuffixContext::RollFrame(_) => KnobNames::KeyOnly,
        SuffixContext::Convert => KnobNames::FlagAndKey,
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
    /// `--film-master` was typed; only a `convert` suffix remedy reads it.
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

/// The **one** spelling of "no film base was stated", so the two places that can
/// report it ([`validate_shared`] and [`convert_frame`]'s totality guard) cannot drift
/// into two differently-worded diagnoses of the same condition. `convert` and `roll`
/// take the same flags, so one remedy serves both.
pub const MISSING_FILM_BASE: &str = "no film base selected: pass --film-base R,G,B (a Dmin \
     measured once per roll with `hanten measure-base <unexposed-frame>`), or --base-region \
     X,Y,W,H to read it from a region of unexposed film. Recipe key: \
     `calibration.film_base`, which `hanten measure-roll <frames> --unexposed \
     <unexposed-scan> --out roll.json` writes for `--params`.";

/// Validate the recipe's **shared** sections — the input, the film base and the
/// measurement region, which the decode and the film-base stage read before any
/// rendering stage — at the CLI boundary, so the pure stages can trust their inputs.
/// Every failure is a [`NcError::Usage`] (exit 2).
///
/// Shared verbatim by `convert` and `roll` (and each `roll` per-frame override); the
/// stage sections' rules are [`recipe::validate`]'s.
pub fn validate_shared(r: &Recipe) -> Result<()> {
    // Film base: an explicit base is a per-channel transmission in (0, 1] — the
    // decoded scan is [0, 1]-normalized, so a value above 1 (e.g. a "90" typo for
    // "0.90") would silently render every real sample denser than the base; a
    // sampled region must have non-zero extent.
    // The *unstated* case is deliberately not handled here — it is the last rule in
    // this function. "You have not chosen a film base" is the least specific
    // diagnosis there is, so letting it run first would pre-empt every rule below
    // (and `reject_roll_unsupported_input`) on a config that has both problems, reporting
    // the vaguer one. Flag-shape first.
    match r.calibration.film_base {
        Some(FilmBaseSource::Explicit(b)) => validate_explicit_film_base(&b)?,
        Some(FilmBaseSource::Region([_, _, w, h])) if w == 0 || h == 0 => {
            return Err(NcError::Usage(
                "--base-region width and height must be > 0".into(),
            ));
        }
        Some(FilmBaseSource::Region(_)) | None => {}
    }

    // Measurement region: a *value* rule, so `roll` and every per-frame override reach
    // it too — a stage-only check would let a whole roll decode before failing per
    // frame. The bound itself is `types::check_measure_inset`, the one definition
    // `film_base::effective_area` also calls.
    check_measure_inset(r.measure.inset)?;

    // Last, deliberately: `calibration.film_base` has no default, and `Dmin` is the
    // divisor of the density conversion, so it must be stated.
    if r.calibration.film_base.is_none() {
        return Err(NcError::Usage(MISSING_FILM_BASE.into()));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Output helpers
// ---------------------------------------------------------------------------

/// Serialize a value as pretty JSON to a file; an I/O failure is a write error.
///
/// Staged and committed at once, so a failure mid-write cannot leave a truncated document
/// at `path`, and outside a conversion's artifact set (`io/transactional-output-writes`):
/// `--report-file` must land even when `--strict` then fails the run, and in `roll` it is
/// a roll-level artifact no single frame's set could hold.
fn write_json<T: Serialize>(path: &Path, value: &T, log: &Log) -> Result<()> {
    commit_json(stage_json(path, value)?, log)
}

/// Stage `value` as pretty JSON at `path`, for [`commit_json`].
fn stage_json<T: Serialize>(path: &Path, value: &T) -> Result<staged::Staged> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| NcError::Other(format!("serializing JSON: {e}")))?;
    staged::stage_bytes(path, json.as_bytes())
}

/// Commit a document [`stage_json`] staged.
fn commit_json(doc: staged::Staged, log: &Log) -> Result<()> {
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
    for note in doc.commit()? {
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
            if print_stdout(&json, "the report")? == Delivery::ReaderGone {
                log.info("stdout's reader closed; the report was not read");
            }
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
    stdio::stderr_line(format_args!("hanten: lcms2 error [{code}]: {msg}"));
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
            stdio::stderr_line(format_args!("hanten: {msg}"));
        }
    }

    /// Warning line — shown unless `--quiet` (the report keeps it either way).
    fn warn(&self, msg: &str) {
        if !self.quiet {
            stdio::stderr_line(format_args!("hanten: warning: {msg}"));
        }
    }

    /// Advisory line — shown unless `--quiet`. Not a warning: its report field is not
    /// `warnings`, so `--strict` ignores it.
    fn note(&self, msg: &str) {
        if !self.quiet {
            stdio::stderr_line(format_args!("hanten: note: {msg}"));
        }
    }

    /// Warning line shown *regardless* of `--quiet`. For fail-soft telemetry
    /// failures, which are deliberately kept out of the JSON report (so `--strict`
    /// can't promote them) and would otherwise vanish entirely under `--quiet` —
    /// an opted-in feature failing must never be silent. Ordinary warnings use
    /// [`warn`](Self::warn), which `--quiet` suppresses since the report still
    /// records them.
    fn warn_always(&self, msg: &str) {
        stdio::stderr_line(format_args!("hanten: warning: {msg}"));
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
    let started = Instant::now();
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            // A refused `convert` is a usage failure in `parse`, recorded only under
            // persistent consent and without reading any argument text.
            if e.use_stderr()
                && std::env::args_os().nth(1).as_deref() == Some(OsStr::new("convert"))
            {
                telemetry::managed::record_parse_failure(started);
            }
            e.exit()
        }
    };
    match cli.command {
        Command::Params(args) => run_params(&args),
        Command::Convert(args) => run_convert(args),
        Command::Roll(args) => run_roll(args),
        Command::Inspect(args) => run_inspect(args),
        Command::MeasureBase(args) => run_measure_base(args),
        Command::Estimate(_) => Err(NcError::Usage(
            "`hanten estimate` was renamed `hanten measure-base`, with the same flags; \
             `--out PATH` now writes the measured base as a recipe for `--params`. To \
             measure a roll's base with its white balance, midtone line, white and \
             exposure, use `hanten measure-roll --unexposed <unexposed.tif>`"
                .into(),
        )),
        Command::MeasureRoll(args) => run_measure_roll(args),
        Command::Telemetry(args) => run_telemetry(args),
    }
}

/// `hanten telemetry …` (`telemetry::maintenance`).
fn run_telemetry(args: TelemetryArgs) -> Result<()> {
    use telemetry::maintenance as m;
    match args.command {
        TelemetryCommand::Enable { queue, yes } => m::enable(queue.as_deref(), yes),
        TelemetryCommand::Disable => m::disable(),
        TelemetryCommand::Status => m::status(),
        TelemetryCommand::Preview => m::preview(),
        TelemetryCommand::Flush => m::flush(),
        TelemetryCommand::Purge { yes } => m::purge(yes),
        TelemetryCommand::UploadOnce { generation } => m::upload_once(&generation),
    }
}

/// `hanten params` — print the full default recipe, in its [`RecipeEnvelope`], to stdout.
fn run_params(args: &ParamsArgs) -> Result<()> {
    reject_new_flow(args.new_flow)?;
    let json = serde_json::to_string_pretty(&RecipeEnvelope::new(&Recipe::default()))
        .map_err(|e| NcError::Other(format!("serializing params: {e}")))?;
    print_stdout(&json, "params")?;
    Ok(())
}

/// Print `json` to stdout through [`stdio::stdout_line`]: a closed pipe is not a
/// failure (the run continues and keeps its exit code), any other write error is
/// exit 5. `what` names the document in that error.
fn print_stdout(json: &str, what: &str) -> Result<Delivery> {
    stdio::stdout_line(json).map_err(|e| NcError::Write(format!("writing {what} to stdout: {e}")))
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
/// equal to the output or an export (truncates a just-written artifact) — all of
/// which would otherwise "succeed" with exit 0. Fail loudly up front instead.
/// Comparison is case-insensitivity-aware (see [`keys_collide`]) so a
/// case-only difference can't slip a second write onto the same file on a
/// case-insensitive filesystem.
fn ensure_write_targets_distinct(input: &Path, targets: &[(&str, &Path)]) -> Result<()> {
    ensure_write_targets_spare(input, "the input scan", targets)
}

/// [`ensure_write_targets_distinct`] for a read file that is not the scan (a recipe,
/// a leader), named by `what` in the refusal.
fn ensure_write_targets_spare(input: &Path, what: &str, targets: &[(&str, &Path)]) -> Result<()> {
    let input_key = collision_key(input);
    let mut seen: Vec<(&str, PathBuf)> = Vec::with_capacity(targets.len());
    for (label, path) in targets {
        let key = collision_key(path);
        if keys_collide(&key, &input_key) {
            return Err(NcError::Usage(format!(
                "{label} ({}) would overwrite {what}",
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
fn reject_removed_flags(args: &ConversionFlags, has_recipe: bool, on_roll: bool) -> Result<()> {
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
    if args.input_opts.export_ir.is_some() {
        return Err(NcError::Usage(
            "--export-ir was removed: the IR plane is no longer exported. Drop the flag.".into(),
        ));
    }
    if args.film_base.auto_base {
        return Err(NcError::Usage(format!(
            "--auto-base was removed: {AUTO_BASE_RETIRED} and pass the `--film-base R,G,B` it \
             reports, or read a region of unexposed film with --base-region X,Y,W,H."
        )));
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
    // `--roll-frame-slope` first: its message migrates a `--frame-lift` beside it too.
    if let Some(slope) = &args.removed_roll.roll_frame_slope {
        return Err(NcError::Usage(removed_frame_slope_message(
            slope, args, on_roll,
        )));
    }
    if let Some(remedy) = removed_frame_lift_remedy(&args.removed_roll) {
        return Err(NcError::Usage(format!(
            "--frame-lift was removed: it switched both a frame's lifts, which now each have \
             their own switch, --small-lift (recipe `roll.small_lift`) and --thin-lift (recipe \
             `roll.thin_lift`). To render as before, {remedy}."
        )));
    }
    if let Some(message) = removed_output_message(args, has_recipe) {
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
            "--display-black (recipe `fit_range.display_black`), which places black",
        ),
        (
            "--clip-high",
            args.simple.clip_high.is_some(),
            "--exposure (recipe `scene_correction.exposure`), which sets brightness",
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

/// The remedy for a typed retired `--frame-lift`, which switched both lifts: both
/// switches now. `None` when it was not typed.
fn removed_frame_lift_remedy(flags: &RemovedRollFlags) -> Option<String> {
    let value = flags.removed_frame_lift.as_deref()?;
    Some(if value.eq_ignore_ascii_case("off") {
        "replace --frame-lift off with --small-lift off --thin-lift off (it turned both lifts \
         off)"
            .into()
    } else if value.eq_ignore_ascii_case("on") {
        // Not a no-op: it beat a recipe's "off".
        "replace --frame-lift on with --small-lift on --thin-lift on (it turned both lifts on)"
            .into()
    } else {
        "drop --frame-lift and pass --small-lift and --thin-lift (on|off)".into()
    })
}

/// The migration error for `--roll-frame-slope S`: the thin pair, with a typed
/// `--roll-frame-exposure` as its exposure, and a typed `--frame-lift` migrated in the same
/// message. `roll` refuses a frame's lift as a flag, so there it names the per-frame keys.
fn removed_frame_slope_message(slope: &str, args: &ConversionFlags, on_roll: bool) -> String {
    let slope = if slope.is_empty() { "S" } else { slope };
    let exposure = args.roll.roll_frame_exposure;
    let lift = removed_frame_lift_remedy(&args.removed_roll)
        .map_or(String::new(), |r| format!(", and {r}"));
    let remedy = if on_roll {
        let (pair, entry) = match exposure {
            Some(e) => (format!("`thin_slope` {slope} and `thin_exposure` {e}"), ""),
            None => (
                format!("`thin_slope` {slope}"),
                " (with its `exposure`, if any, as `thin_exposure`)",
            ),
        };
        format!(
            "state that frame's {pair} in the shared recipe's `roll.frames` entry{entry}, or \
             as `roll.thin_slope` / `roll.thin_exposure` in a --frames manifest's \
             `params`{lift}"
        )
    } else {
        format!(
            "pass --roll-thin-slope {slope}{}{lift}",
            match exposure {
                Some(e) => format!(
                    " --roll-thin-exposure {e} in place of --roll-frame-exposure {e} (beside a \
                     slope it was the thin lift's exposure)"
                ),
                // The frame's entry `exposure` rendered beside the old slope.
                None => " with, as --roll-thin-exposure, the frame's `roll.frames` entry \
                         `exposure` if it has one"
                    .to_owned(),
            },
        )
    };
    format!(
        "--roll-frame-slope was removed: a thin frame's lift is its own pair, beside the small \
         lift. To render as before, {remedy}; or re-run `hanten measure-roll`."
    )
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
            "--linear-range was removed with the print stage: the levels remap retired, \
             since its gain is `--exposure` (recipe `scene_correction.exposure`) and its \
             black is `--display-black` (recipe `fit_range.display_black`). There is no \
             alias."
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
fn removed_output_message(args: &ConversionFlags, has_recipe: bool) -> Option<String> {
    let flags = &args.removed_output;
    const AXES: &str = "a destination is four separate knobs — --range, --transfer, \
                        --gamut, --container (recipe `output.display`) — or --film-master";
    if let Some(name) = &flags.output_preset {
        return Some(format!(
            "--output-preset was removed with the chain its presets named: {AXES}. {} \
             There is no alias.",
            preset_counterpart(name, args, has_recipe)
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
                 with a `--gamut`, or `--film-master`). Drop the flag."
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
    ("display-p3", "--range sdr --gamut display-p3"),
    ("compatibility", "--range sdr --gamut srgb"),
    ("film-master", "--film-master"),
    ("hdr-linear-tiff", "--transfer linear --gamut bt2020"),
    ("hdr-pq-tiff", "--transfer pq"),
    ("hdr-hlg-tiff", "--transfer hlg"),
    // The AVIF container went later (`docs/design/avif-removal.md`): the same signal
    // as a TIFF.
    ("hdr-pq", "--transfer pq"),
    ("hdr-hlg", "--transfer hlg"),
    // Neither is the same file: the map is per-channel and ISO-only, so a reader that
    // knows only the Ultra HDR v1 XMP shows the SDR base.
    (
        "gain-map-hdr",
        "--range hdr --gamut display-p3 --container jpeg",
    ),
    (
        "ultra-hdr-v1",
        "--range hdr --gamut display-p3 --container jpeg",
    ),
];

/// What replaces a removed output preset, as a sentence — for a name with no counterpart
/// (`legacy`, `custom`, which retired before the chain did, or a typo), how to choose.
fn preset_counterpart(name: &str, args: &ConversionFlags, has_recipe: bool) -> String {
    let Some(&(_, flags)) = PRESET_COUNTERPARTS.iter().find(|(n, _)| *n == name) else {
        return "Drop the flag, or state the axes you want.".into();
    };
    match name {
        "gain-map-hdr" | "ultra-hdr-v1" => format!(
            "For `{name}`, the nearest is {flags}: its gain map is per-channel and carries \
             ISO 21496-1 metadata only, without the Ultra HDR v1 XMP."
        ),
        "hdr-pq" | "hdr-hlg" => format!(
            "For `{name}`, the nearest is {flags}: the same Rec.2100 signal as a 16-bit \
             TIFF, since Hanten no longer writes AVIF (`docs/design/avif-removal.md`)."
        ),
        // `direct` is refused beside the film master: name the way back when it may be in
        // play (typed, or from a recipe this refusal runs too early to read).
        "film-master" => {
            let direct = args.rendering.rendering == Some(crate::rendering::Rendering::Direct);
            let rendering = if direct {
                ", with --rendering default in place of --rendering direct"
            } else if has_recipe {
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
/// `convert` and `measure-base --d-max-region` so they say the same thing. The remedy is
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
/// provenance.
#[derive(Clone, Copy, Debug, Default)]
struct InputFromCli {
    transfer: bool,
    meaning: bool,
}

impl InputFromCli {
    /// No CLI input assertions (`measure-roll`, which takes no input flags).
    fn none() -> Self {
        Self::default()
    }

    fn of(flags: &InputOverrides) -> Self {
        Self {
            transfer: flags.input_transfer.is_some(),
            meaning: flags.input_meaning.is_some(),
        }
    }

    /// The flags' provenance under a `roll` frame's manifest `params`, which win over
    /// them: an axis the override states is the manifest's, not the CLI's.
    fn under(self, overrides: &serde_json::Value) -> Self {
        let states = |ptr| overrides.pointer(ptr).is_some_and(|v| !v.is_null());
        Self {
            transfer: self.transfer && !states("/input/transfer"),
            meaning: self.meaning && !states("/input/meaning"),
        }
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
        positive_mode: info.is_silverfast_positive_mode(),
    }
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
/// Shared by `convert`/`roll` and by `inspect`/`measure-base` so the four commands
/// gate identically — each with its own profile, since `inspect`/`measure-base` stop
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
    log_memory_preflight(&mem, log);
    if let Some(msg) = memory::warn_message(&mem) {
        push_warning_buf(warnings, log, msg);
    }
    Ok(mem)
}

/// The preflight's `-v` progress line.
fn log_memory_preflight(mem: &MemoryReport, log: &Log) {
    log.info(format_args!(
        "memory preflight: estimated peak {} bytes, budget {} bytes ({:?})",
        mem.estimate.estimated_peak_bytes, mem.budget_bytes, mem.budget_source
    ));
}

/// The film-base sampling a resolved [`FilmBaseSource`] will perform, for the
/// memory model's film-base phase: an explicit base reads no pixels, a region
/// materializes exactly its rectangle.
fn sample_plan(source: &FilmBaseSource) -> SamplePlan {
    match source {
        FilmBaseSource::Explicit(_) => SamplePlan::none(),
        FilmBaseSource::Region([_, _, w, h]) => SamplePlan::rect(*w as u64 * *h as u64),
    }
}

/// The per-frame conversion core: **stage-0 memory preflight** → decode →
/// film-base estimate → render → encode. Pure of the operational
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
    // `convert`'s side exports; `roll` has neither flag.
    exports: Exports<'_>,
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
    // lives in another module — sharing `MISSING_FILM_BASE` so this unreachable
    // spelling cannot drift into a second, thinner diagnosis of the same condition.
    let base_source = recipe
        .calibration
        .film_base
        .clone()
        .ok_or_else(|| NcError::Usage(MISSING_FILM_BASE.into()))?;
    // Validated before anything was decoded; resolved again here from the same recipe,
    // so what renders is what was checked.
    let destination = recipe::destination(recipe, KnobNames::FlagAndKey)?;

    let mut report = Report {
        command: Some(command),
        identity: Some(Identity::new().with_params_hash(recipe.params_hash())),
        input: Some(input.to_path_buf()),
        output: Some(output.to_path_buf()),
        film_base_source: Some(FilmBaseProvenance::from(&base_source)),
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
        run_profile(destination),
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
    // decode, input semantics, effective area and its warnings, the polarity check —
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

    // Stage 2 — film-base estimate. Resolved before the render so its quality
    // warning (a non-uniform region) is pushed — and so
    // echoed to stderr — *before* the fallible render runs, and ride out in the
    // JSON report on a successful run. (A hard render failure propagates its error
    // and exit code like every other error path and emits no report; the stderr
    // warnings still stand.)
    let base = clock.time(StageKind::FilmBase, || {
        film_base::estimate(&image, &base_source)
    })?;
    report.film_base = Some(base.base);
    report.film_base_percentile = base.percentile;
    for w in base.warnings {
        push_warning_buf(warnings, log, w);
    }

    // The effective measurement area, resolved **unconditionally** and always
    // reported: that is what makes `--measure-inset` observable rather than
    // accepted-and-ignored, which the project forbids. The march costs less than
    // run-to-run noise even on an 18.7 MP frame, so there is nothing to save by
    // skipping it.
    //
    // Its one consumer here is the polarity check, a warning that an empty region
    // skips — so an **empty** region is a warning rather than a refusal, with no
    // `report.effective_area`. A consumer added here must decide whether an empty
    // region becomes fatal for it.
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
            let polarity = clock.time(StageKind::FilmBase, || {
                film_base::polarity_warning(&image, area.region, &base.base)
            })?;
            if let Some(w) = polarity {
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
                    "{} The render does not read the region, so it is unaffected; the \
                     polarity check is skipped and the report omits `effective_area` \
                     (--measure-inset has no effect on this run).",
                    e.message()
                ),
            );
        }
    }

    render_frame(
        DecodedFrame {
            recipe,
            destination,
            image,
            base: base.base,
            exports,
            output,
            report,
            read_inputs,
        },
        clock,
        log,
        warnings,
    )
}

const CHANNEL_NAMES: [&str; 3] = ["red", "green", "blue"];

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
    // A channel at 0 everywhere is in range, so no loss counter sees it.
    let black = outcome.stats.black_channels();
    if loss.total_samples > 0 && !black.is_empty() {
        let channels = match black.as_slice() {
            [c] => format!("{} channel", CHANNEL_NAMES[*c]),
            [a, b] => format!("{} and {} channels", CHANNEL_NAMES[*a], CHANNEL_NAMES[*b]),
            _ => "red, green and blue channels".into(),
        };
        push_warning_buf(
            warnings,
            log,
            format!(
                "no written sample is above 0 in the {channels}: the frame renders black there"
            ),
        );
    }
    // A non-finite sample is a numerical fault, not routine gamut clipping — make
    // sure it is never fully silenced (the `--quiet --report none` combination
    // would otherwise suppress both channels of the warning above).
    if loss.non_finite > 0 && log.quiet {
        stdio::stderr_line(format_args!(
            "hanten: warning: {} non-finite (NaN/inf) output sample(s) — numerical fault",
            loss.non_finite
        ));
    }
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
        // MatrixCoefficients 0: an RGB ICC profile requires it (ICC.1:2022 §10.3).
        cicp: [metadata.cicp_color_primaries, metadata.cicp_transfer, 0],
        full_range: metadata.full_range,
        max_quantization_error_codes: summary.max_quantization_error_codes,
        rms_quantization_error_codes: summary.rms_quantization_error_codes,
        reference_white_nits: metadata.linear.reference_white_nits,
        target_peak_nits: metadata.linear.target_peak_nits,
        tone_curve: metadata.linear.tone_curve,
        // PQ only: HLG is a relative signal, so absolute content-light values would
        // be a false claim rather than a missing one.
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
             deliverable, and see the gain-map destination for delivery",
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
        interoperability: hdr::linear_labels(summary.gamut).interoperability,
    });
}

/// The last `pipeline_version` that wrote a sidecar.
const LAST_SIDECAR_PIPELINE_VERSION: u64 = 7;

/// Whether `path` holds one of nc's sidecars, recognised by its provenance rather
/// than by key names: the `{meta, params}` envelope with `params` an object and
/// `meta` carrying the identity every sidecar stamps (`nc_version`,
/// `pipeline_version`, `target`), from a build that wrote sidecars. Every recipe
/// document a later build writes has the same envelope ([`RecipeEnvelope`]), so the
/// version is what keeps a `--dump-params` file named `<output>.json` from being
/// deleted. Deleting is destructive, so anything short of that — missing, unreadable,
/// or a different file that merely shares the shape — is not ours to remove.
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
        && meta["pipeline_version"]
            .as_u64()
            .is_some_and(|v| v <= LAST_SIDECAR_PIPELINE_VERSION)
        && meta["target"].is_string()
}

/// The memory profile a destination is sized with: one per shape of buffers
/// it holds, not one per destination; each sharing is measured (`pipeline::memory`).
fn run_profile(destination: recipe::Destination) -> RunProfile {
    match destination {
        recipe::Destination::FilmMaster => RunProfile::F32Tiff,
        recipe::Destination::Display(d) => match d.encoding {
            Encoding::SdrTiff | Encoding::HdrCodedTiff(_) => RunProfile::U16Tiff,
            Encoding::HdrLinearTiff => RunProfile::F32Tiff,
            Encoding::GainMapJpeg => RunProfile::GainMapJpeg,
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
    /// Display-linear in the destination's gamut, clamped to the peak.
    HdrLinear(hdr::LinearHdr, hdr::PeakClamp),
    /// A Rec.2100 signal for the 16-bit TIFF.
    HdrCoded(hdr::RenderedHdr, hdr::PeakClamp),
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
                DestinationPixels::HdrCoded(r, _) => Some(r.metadata().content_light),
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
}

/// Render the fixed decode's ACEScg for `destination`: nothing for the film master; the
/// chain, then the destination's transfer, for a rendered one. A rendered destination
/// also decodes the film base itself, one pixel through the same decode and 3×3 —
/// display black's reference (`chain::render`); the film master never reads it.
///
/// With `export`, the buffers the encoder receives are staged there too
/// (`--export-pre-encode`): linear, before any transfer or quantization, written from
/// the buffers the render already holds.
fn render_destination(
    aces: AcesCgImage,
    base: &FilmBase,
    recipe: &Recipe,
    destination: recipe::Destination,
    export: Option<&Path>,
    clock: &mut impl StageClock,
) -> Result<(DestinationRender, Option<staged::Staged>)> {
    let d = match destination {
        recipe::Destination::FilmMaster => {
            // Profile only — no transform: the tag names the space the pixels are in.
            let (icc, staged) = clock.time(StageKind::Destination, || {
                let page = PreEncodePage::rgb_f32(
                    "film-master",
                    "acescg",
                    aces.width(),
                    aces.height(),
                    aces.rgb(),
                );
                let staged = export_pages(export, &[page])?;
                Ok::<_, NcError>((color::icc_profile(&color::OutputSpace::AcesCg)?, staged))
            })?;
            let render = DestinationRender::FilmMaster {
                image: aces.into_linear(),
                icc,
            };
            return Ok((render, staged));
        }
        recipe::Destination::Display(d) => d,
    };
    let film_base = clock.time(StageKind::Reconstruction, || {
        fixed::decode_film_base(base, &recipe.reconstruction).map(working_space::map_nc_film_rgb_v1)
    })?;
    // One match on the encoding, so a new row cannot reach an encoder it was not written
    // for: the gain map renders a pair, every other destination one rendition.
    let mut staged = None;
    let render = match d.encoding {
        Encoding::GainMapJpeg => {
            let (render, gain_map_staged) =
                render_gain_map(aces, film_base, recipe, d, export, clock)?;
            staged = gain_map_staged;
            render
        }
        Encoding::SdrTiff => render_one(aces, film_base, recipe, d, clock, |r| {
            let page = PreEncodePage::f32("sdr-linear", r.gamut.name(), &r.linear);
            staged = export_pages(export, &[page])?;
            let (image, icc) = color::encode_display_linear(r.linear, r.gamut)?;
            Ok(DestinationPixels::Sdr { image, icc })
        })?,
        Encoding::HdrLinearTiff => render_one(aces, film_base, recipe, d, clock, |r| {
            let (hdr, clamp) = r.hdr()?;
            staged = export_pages(export, &[hdr_page(&hdr)])?;
            Ok(DestinationPixels::HdrLinear(hdr, clamp))
        })?,
        Encoding::HdrCodedTiff(transfer) => render_one(aces, film_base, recipe, d, clock, |r| {
            let (hdr, clamp) = r.hdr()?;
            staged = export_pages(export, &[hdr_page(&hdr)])?;
            Ok(DestinationPixels::HdrCoded(
                hdr::encode_transfer(hdr, transfer)?,
                clamp,
            ))
        })?,
    };
    Ok((render, staged))
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
    /// The HDR hand-off, in the rendition's gamut. A Rec.2100 encoder refuses any but
    /// BT.2020 (`hdr::encode_transfer`).
    fn hdr(self) -> Result<(hdr::LinearHdr, hdr::PeakClamp)> {
        hdr::from_new_chain(self.linear, self.gamut, self.tone_curve, self.gamut_mapping)
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
/// buffer is dropped once the map is built (`RunProfile::GainMapJpeg` sums them, under
/// the memory model's retention rule). `export` stages both linear renditions and the map's
/// codes, in that order.
fn render_gain_map(
    aces: AcesCgImage,
    film_base: AcesCgImage,
    recipe: &Recipe,
    d: Resolved,
    export: Option<&Path>,
    clock: &mut impl StageClock,
) -> Result<(DestinationRender, Option<staged::Staged>)> {
    let peak = d.range.peak()?;
    // Neither JPEG stores the IR plane, so it is dropped before the pair splits the
    // graded image and copies it.
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
    let (base, icc, map, clamp, report, staged) = clock.time(StageKind::Destination, || {
        let clamp = hdr::clamp_to_peak(&mut hdr_linear.rgb)?;
        let ratios = gain_ratio::between(&sdr_linear, &hdr_linear, gain_encode::OFFSET)?;
        let map = gain_encode::encode(&ratios)?;
        let staged = export_pages(
            export,
            &[
                PreEncodePage::f32("sdr-linear", gamut.name(), &sdr_linear),
                PreEncodePage::f32("hdr-linear", gamut.name(), &hdr_linear),
                PreEncodePage {
                    buffer: "gain-map-codes",
                    space: "log2-gain-window",
                    width: map.width,
                    height: map.height,
                    samples: PreEncodeSamples::U8(&map.rgb),
                },
            ],
        )?;
        drop(hdr_linear);
        let report = GainMapResult {
            range: ratios.range(),
            width: map.width,
            height: map.height,
            base_fit_range,
        };
        drop(ratios);
        let (base, icc) = color::encode_display_linear(sdr_linear, gamut)?;
        Ok::<_, NcError>((base, icc, map, clamp, report, staged))
    })?;
    let render = DestinationRender::Rendered {
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
    };
    Ok((render, staged))
}

/// Stage `pages` at `export`, when one was asked for.
fn export_pages(
    export: Option<&Path>,
    pages: &[PreEncodePage<'_>],
) -> Result<Option<staged::Staged>> {
    export
        .map(|path| encode::export_pre_encode(pages, path))
        .transpose()
}

/// An HDR rendition's page: display-linear, 1.0 = the 203 cd/m² reference white,
/// after the clamp to the peak.
fn hdr_page(hdr: &hdr::LinearHdr) -> PreEncodePage<'_> {
    PreEncodePage::f32("hdr-linear", hdr.gamut().name(), hdr.image())
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
            let icc = color::hdr_linear_icc(hdr.gamut())?;
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
        DestinationPixels::GainMap {
            base,
            icc,
            map,
            headroom,
            ..
        } => iso_gain_map::encode(&base, &icc, &map, headroom, output)?,
    })
}

/// A frame's side exports, staged with the primary and committed before it.
#[derive(Clone, Copy, Default)]
struct Exports<'a> {
    /// `--export-film-rgb`: the fixed decode before the 3×3.
    film_rgb: Option<&'a Path>,
    /// `--export-pre-encode`: the buffers the destination's encoder receives.
    pre_encode: Option<&'a Path>,
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
    exports: Exports<'a>,
    output: &'a Path,
    report: Report,
    read_inputs: &'a [&'a Path],
}

/// The render, encode and commit for one frame: the fixed decode (`algo::fixed`) → NC
/// film RGB v1 → `pipeline::chain` → the destination the recipe's `output` resolves to
/// (`crate::destination`), or straight to the film master.
///
/// The optional side exports and the primary are staged, then committed
/// together with the primary last. No sidecar is written.
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
        exports,
        output,
        mut report,
        read_inputs,
    } = frame;
    let decode_params = recipe.reconstruction;

    // Reconstruction: the fixed decode, which consumes the scan, then the pinned NC
    // film RGB v1 3×3. The film RGB export, if asked for, is staged between them: it
    // writes the decode's buffer before the in-place 3×3, so it adds no image buffer,
    // and it is timed as `encode`.
    let (film, decoded) = clock.time(StageKind::Reconstruction, || {
        fixed::decode(image, &base, &decode_params)
    })?;
    let mut pending: Vec<staged::Staged> = Vec::new();
    if let Some(path) = exports.film_rgb {
        pending.push(clock.time(StageKind::Encode, || encode::encode_film_rgb(&film, path))?);
        report.film_rgb_exported = Some(path.to_path_buf());
    }
    let aces = clock.time(StageKind::Reconstruction, || {
        Ok(working_space::map_nc_film_rgb_v1(film))
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
    let (render, pre_encode) =
        render_destination(aces, &base, recipe, destination, exports.pre_encode, clock)?;
    if let Some(staged) = pre_encode {
        pending.push(staged);
        report.pre_encode_exported = exports.pre_encode.map(Path::to_path_buf);
    }
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
        look: rendered.map(|r| recipe::LookReport {
            slope: recipe.resolved_slope(),
            section: r.look,
        }),
        fit_range: rendered.map(|r| r.fit_range),
        destination: match destination {
            recipe::Destination::FilmMaster => OutputSection::FilmMaster,
            recipe::Destination::Display(d) => OutputSection::Display(d.axes()),
        },
        peak_clamp,
        gain_map: render.gain_map(),
        removed_sidecar: None,
    });

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
    if let Some(path) = exports.film_rgb {
        log.info(format_args!("wrote film RGB {}", path.display()));
    }
    if let Some(path) = exports.pre_encode {
        log.info(format_args!("wrote pre-encode buffers {}", path.display()));
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
/// chain → encode. Warnings are
/// collected into the report and echoed to stderr; `--strict` promotes any of
/// them to a non-zero exit.
///
/// Telemetry (opt-in) is emitted here, once the run's outcome is fixed: a success
/// event, or a failure event carrying what [`ConvertAttempt`] had learned.
fn run_convert(args: ConvertArgs) -> Result<()> {
    // Persistent consent, captured before anything runs and held until the process
    // ends, with the panic hook it installs. Ahead of the clock, so its bounded lock
    // waits never count as the run's time.
    telemetry::panic::enter(telemetry::EventStage::Setup);
    let managed = telemetry::managed::begin().map(telemetry::managed::keep_for_process);
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
    if telemetry_requested(&args) || managed.is_some() {
        emit_telemetry(
            &args,
            &log,
            started,
            &attempt,
            result.as_ref().err(),
            telemetry_log.as_deref(),
            managed,
        );
    }
    // Last, once the outcome, report and event are fixed.
    if let Some(managed) = managed {
        managed.launch_helper();
    }
    result
}

/// Where a `convert` is, for a failure event's `stage`.
#[derive(Clone, Copy, Debug, Default)]
enum ConvertPhase {
    /// Before the frame: recipe, validation, output path, the write-target guard, the
    /// `--dump-params` staging.
    #[default]
    Setup,
    /// Inside [`convert_frame`]; its stage clock says where.
    Frame,
    /// After the frame: the report, the `--strict` gate and the `--dump-params` commit.
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
    /// The resolved output path, once known — a path a failure event's sinks must not
    /// land on.
    output: Option<PathBuf>,
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
    reject_deprecated_input_flags(&args.knobs.input_opts)?;
    // Removed flags run first, so a retired spelling is diagnosed as retired before any
    // rule reasons about the values the recipe resolves.
    reject_removed_flags(&args.knobs, !args.recipe_in.is_empty(), false)?;
    // A recipe written for the removed chain is refused inside the load, by name.
    let loaded = load_recipes(&args.recipe_in)?;
    // The frame's own roll values first, then the flags win over every recipe.
    let stated_roll = loaded.recipe.roll.clone();
    let recipe = recipe::merge(loaded.recipe.for_frame(&args.input), &args.knobs);
    // Flag-presence rules, ahead of every value rule that could refuse first.
    reject_a_line_without_its_gains(&args.knobs, &recipe)?;
    reject_roll_flags_nothing_applies(&args.knobs, &recipe)?;
    let typed = &args.knobs.roll;
    if typed.roll_frame_exposure.is_some()
        && typed.roll_thin_slope.is_none()
        && typed.roll_thin_exposure.is_none()
    {
        reject_a_small_lift_beside_a_thin_one(
            &recipe,
            "--roll-frame-exposure",
            ThinLiftNames::FLAGS,
        )?;
    }
    if typed.roll_thin_slope.is_some() && typed.roll_thin_exposure.is_none() {
        reject_a_thin_slope_over_a_small_lift(&recipe, "--roll-thin-slope", ThinLiftNames::FLAGS)?;
    }
    // The table as stated: `for_frame` moved this frame's entry into `roll.white_stops`,
    // where `validate` would name the wrong key. Only over a sound decode, as in
    // `validate`: a bad linearization is its own fault, not an entry's.
    if recipe.reconstruction.check().is_ok() {
        recipe::validate_roll_frames(
            &stated_roll,
            recipe.reconstruction.linearization,
            [recipe.look.contrast, recipe.look.saturation],
            KnobNames::FlagAndKey,
        )?;
    }
    validate_convert(&recipe, args)?;
    // Last of the value rules: whether the values combine into a render. The frame's own
    // entry is passed so a fault it causes is named as the entry.
    let own = args
        .input
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| (n, &stated_roll));
    recipe::validate_render(&recipe, own, KnobNames::FlagAndKey)?;

    // The path nc actually writes: `-o out` under the default becomes `out.tiff`.
    // Resolved **here**, before anything derives from it — the write-target guard, the
    // report's `output` and telemetry's `output_bytes` must all see the completed path,
    // never the stem. (`validate_convert` ran the same rule and discarded the value;
    // this is the one call that keeps it.)
    let target = OutputTarget::resolve(
        &recipe,
        KnobNames::FlagAndKey,
        args.knobs.destination.film_master,
    )?;
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
    let targets = write_targets(args, &output, telemetry_log);
    ensure_write_targets_distinct(&args.input, &targets)?;
    // `--dump-params X --params X` is allowed only when X is the sole layer: it
    // rewrites the recipe it replays. Over one of several, it would fold the others in.
    if let Some(dump) = &args.dump_params
        && args.recipe_in.len() > 1
        && let Some(layer) = recipe_files(&args.recipe_in)
            .find(|p| keys_collide(&collision_key(p), &collision_key(dump)))
    {
        return Err(NcError::Usage(format!(
            "--dump-params ({}) would overwrite the --params layer {}: with several layers \
             it would rewrite that layer into the whole resolved run (its base, roll values \
             and roll.frames table), carrying them into later runs. Dump to another path",
            dump.display(),
            layer.display()
        )));
    }
    let spare_recipes: Vec<_> = targets
        .iter()
        .copied()
        .filter(|(label, _)| {
            matches!(
                *label,
                "--output" | "--report-file" | "--export-film-rgb" | "--export-pre-encode"
            )
        })
        .collect();
    for recipe_file in recipe_files(&args.recipe_in) {
        ensure_write_targets_spare(recipe_file, "the --params recipe", &spare_recipes)?;
    }
    if let Some(msg) = telemetry_sink_collision(args, &output, telemetry_log) {
        return Err(NcError::Usage(msg));
    }
    attempt.guarded = true;

    // The resolved recipe, which reloads through `--params` to the same run. Staged here,
    // so a bad path fails before the decode, and committed only once the run has passed,
    // so a failed one leaves a replayed layer as it was (dropping it removes the temp).
    let dump = args
        .dump_params
        .as_deref()
        .map(|path| stage_json(path, &RecipeEnvelope::new(&recipe)))
        .transpose()?;
    // The guard above cannot resolve a dangling symlink, so a dump linked to an artifact
    // written before it commits (or one linked to the dump) is caught here, by where it
    // lands.
    if let (Some(dump), Some(dump_path)) = (&dump, &args.dump_params) {
        let earlier = [
            ("--output", Some(output.as_path())),
            ("--export-film-rgb", args.export_film_rgb.as_deref()),
            ("--report-file", args.report.report_file.as_deref()),
        ];
        for (label, path) in earlier {
            if let Some(path) = path.filter(|p| dump.lands_on(p)) {
                return Err(NcError::Usage(format!(
                    "--dump-params ({}) and {label} ({}) resolve to the same file — one \
                     would silently overwrite the other. This can happen when a symlinked \
                     path points at another artifact's path",
                    dump_path.display(),
                    path.display()
                )));
            }
        }
    }
    // `--seed` is reserved (no stochastic step in Step 1) but accepted so the
    // documented flag isn't rejected; nothing consumes it yet.
    let _ = args.seed;

    // Replaying a recipe written under a *different* behavioral `pipeline_version`
    // still applies its parameters, but the default render has changed underneath
    // them — so the pixels won't match the original. Loud and `--strict`-promotable
    // rather than a silently-different image: exposing exactly that mismatch is why
    // `pipeline_version` exists. Pushed before the conversion so it is on stderr before
    // any work happens; note that on a *failed* frame no report is emitted, so stderr
    // is the only place it appears there. (`roll` differs: it records per-frame
    // failures and still emits its report, so the roll-level warning survives a bad
    // frame.)
    for msg in loaded.provenance_warnings {
        push_warning_buf(&mut attempt.warnings, log, msg);
    }
    // What the run's recipe falls back on, or states that nobody may have chosen — a fact
    // about the run's recipe, not the frame; a typed style flag never warns.
    for msg in recipe.recipe_warnings(recipe::TypedStyle::of(&args.knobs)) {
        push_warning_buf(&mut attempt.warnings, log, msg);
    }

    // The per-frame pipeline core (decode → film-base → render → encode), shared
    // byte-for-byte with `roll`. Operational concerns the two orchestrators layer
    // differently — report emission, `--strict` gating, telemetry — stay out here.
    attempt.phase = ConvertPhase::Frame;
    telemetry::panic::enter(telemetry::EventStage::Preflight);
    let frame = convert_frame(
        "convert",
        &args.input,
        &output,
        Exports {
            film_rgb: args.export_film_rgb.as_deref(),
            pre_encode: args.export_pre_encode.as_deref(),
        },
        &recipe,
        InputFromCli::of(&args.knobs.input_opts),
        &recipe_files(&args.recipe_in)
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
    telemetry::panic::enter(telemetry::EventStage::Finalize);

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
    if let Some(dump) = dump {
        commit_json(dump, log)?;
    }
    Ok(())
}

/// Every path a `convert` writes, labelled for [`ensure_write_targets_distinct`]:
/// the output, `--dump-params`, `--report-file`, `--export-film-rgb`,
/// `--export-pre-encode`, and telemetry's sinks (`--telemetry-file` unless it is `-`,
/// and the resolved log).
fn write_targets<'a>(
    args: &'a ConvertArgs,
    output: &'a Path,
    telemetry_log: Option<&'a Path>,
) -> Vec<(&'static str, &'a Path)> {
    let mut targets: Vec<(&str, &Path)> = vec![("--output", output)];
    if let Some(p) = &args.dump_params {
        targets.push(("--dump-params", p));
    }
    if let Some(p) = args.report.report_file.as_deref() {
        targets.push(("--report-file", p));
    }
    if let Some(p) = args.export_film_rgb.as_deref() {
        targets.push(("--export-film-rgb", p));
    }
    if let Some(p) = args.export_pre_encode.as_deref() {
        targets.push(("--export-pre-encode", p));
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
    /// Which input axes the CLI asserted for this frame, its `overrides` accounted for.
    input_from_cli: InputFromCli,
    /// Where `--export-film-rgb` writes this frame's film RGB ([`film_rgb_export_name`]).
    film_rgb: Option<PathBuf>,
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
        hdr_linear_tiff: Option<Box<HdrLinearTiffResult>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        hdr_coded_tiff: Option<Box<HdrCodedTiffResult>>,
        /// Where `--export-film-rgb` wrote this frame's film RGB — mirrors the
        /// single-frame `Report` field.
        #[serde(skip_serializing_if = "Option::is_none")]
        film_rgb_exported: Option<PathBuf>,
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
    out_dir.join(format!(
        "{}_positive.{}",
        input_stem(input),
        target.container().canonical()
    ))
}

/// A frame's `roll --export-film-rgb` path: `<input-stem>_film-rgb.tiff` in its
/// output's directory, so it sits beside the frame's output, explicit or derived.
fn film_rgb_export_name(input: &Path, output: &Path) -> PathBuf {
    output.with_file_name(format!("{}_film-rgb.tiff", input_stem(input)))
}

fn input_stem(input: &Path) -> String {
    input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "frame".to_string())
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

/// Load a `--frames` manifest. A read failure or invalid/unknown-key JSON is a
/// usage error (a config mistake), like [`load_recipes`].
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

/// `roll`'s whole gate for one recipe — the roll's, or a frame's after its override
/// merged: the roll-specific rejections first, then the stage sections' rules, then the
/// shared sections, whose missing-base rule is the least specific diagnosis there is.
/// A frame's `context` prefixes a stage rule's message and it names the key alone, since
/// the fault is in its manifest entry; the roll's recipe (`None`) names flag and key
/// unprefixed, as on `convert`. `own` is [`recipe::validate_render`]'s.
fn validate_roll_recipe(
    r: &Recipe,
    frame_context: Option<&str>,
    own: Option<(&str, &recipe::RollSection)>,
) -> Result<()> {
    // No destination list here: every destination is roll-capable, since a derived name
    // takes its frame's suffix and a manifest path goes through `resolve_output_path`.
    reject_roll_unsupported_input(r)?;
    let names = match frame_context {
        Some(_) => KnobNames::KeyOnly,
        None => KnobNames::FlagAndKey,
    };
    let in_context = |e: NcError| {
        NcError::Usage(match frame_context {
            Some(context) => format!("{context}: {}", e.message()),
            None => e.message().to_string(),
        })
    };
    recipe::validate(r, names).map_err(in_context)?;
    validate_shared(r)?;
    recipe::validate_render(r, own, names).map_err(in_context)
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
/// is frame-local: `roll.white_stops` is a clamped frame's own white (`roll.frames`, or a
/// manifest's `params`), and `input` describes each file, not the roll.
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
        // `rendering` or `output` row's. A frame's own `neutral_balance` off is frame-local.
        compare: |f, r| {
            if !(f.applies_roll_to_scene_correction() && r.applies_roll_to_scene_correction())
                || f.roll.applied_white_balance().is_none()
            {
                return Ok(None);
            }
            changed(&f.roll.white_balance, &r.roll.white_balance)
        },
    },
    RollWide {
        key: "roll.midtone_line",
        breaks: "this frame's midtones are corrected apart from the roll's (a frame's own \
                 is `roll.midtone_neutral` \"off\")",
        compare: |f, r| {
            if !(f.applies_roll_to_scene_correction() && r.applies_roll_to_scene_correction())
                || f.roll.applied_midtone_line().is_none()
            {
                return Ok(None);
            }
            changed(&f.roll.midtone_line, &r.roll.midtone_line)
        },
    },
    RollWide {
        key: "roll.exposure",
        breaks: "this frame is exposed apart from the roll's measured exposure (a frame's \
                 own adjustment is `scene_correction.exposure`, which adds to it)",
        compare: |f, r| {
            if !(f.applies_roll_to_scene_correction() && r.applies_roll_to_scene_correction()) {
                return Ok(None);
            }
            changed(&f.roll.exposure, &r.roll.exposure)
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
///
/// A frame resolves as `convert` would: `stated` (the `--params` layers) with its own
/// `roll.frames` entry, then the flags, then its manifest `params`. `shared` is the
/// roll's recipe with the flags applied, which the roll-wide warnings compare against.
fn resolve_frames(
    args: &RollArgs,
    stated: &Recipe,
    shared: &Recipe,
    roll_warnings: &mut Vec<String>,
    log: &Log,
) -> Result<Vec<PlannedFrame>> {
    let own = |input: &Path| recipe::merge(stated.clone().for_frame(input), &args.knobs);
    let from_cli = InputFromCli::of(&args.knobs.input_opts);
    let out_dir = args.out_dir.as_path();
    let film_rgb = |input: &Path, output: &Path| {
        args.export_film_rgb
            .is_some()
            .then(|| film_rgb_export_name(input, output))
    };
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
            for mf in manifest.frames {
                let mut own = own(&mf.input);
                let (recipe, overrides) = match mf.params {
                    Some(mut ov) => {
                        // A per-frame override carrying a removed key gets the same
                        // migration guidance as the shared recipe, not an opaque
                        // `deny_unknown_fields` serde error.
                        let context =
                            format!("frame {}: per-frame `params` override", mf.input.display());
                        recipe::strip_retired_nulls(&mut ov);
                        recipe::check_body(&ov, false, &context)?;
                        if ov.pointer("/roll/frames").is_some() {
                            return Err(NcError::Usage(format!(
                                "{context}: `roll.frames` names frames, so it belongs in the \
                                 shared recipe; state this frame's own white as \
                                 `roll.white_stops` and its lift as `roll.frame_exposure`, or \
                                 `roll.thin_slope` and `roll.thin_exposure`, here"
                            )));
                        }
                        // Its white beats the thin lift beneath it, as a typed one does.
                        if ov
                            .pointer("/roll/white_stops")
                            .is_some_and(|v| !v.is_null())
                        {
                            own.roll.drop_thin_lift();
                        }
                        // This frame's recipe as JSON, so the partial override
                        // deep-merges onto it and deserializes back with
                        // `deny_unknown_fields`. What holds a null is left out of the
                        // merge, then put back where the full document has no such key,
                        // so a mistyped key is named as unknown before a null is refused.
                        let mut v = serde_json::to_value(&own).map_err(|e| {
                            NcError::Other(format!("serializing shared recipe: {e}"))
                        })?;
                        recipe::merge_json(&mut v, &without_nulls(&ov));
                        let invalid = |e: serde_json::Error| {
                            NcError::Usage(format!(
                                "frame {}: invalid params override: {e}",
                                mf.input.display()
                            ))
                        };
                        let r: Recipe = serde_json::from_value(v.clone()).map_err(invalid)?;
                        if let Some(key) = first_null_key(&ov) {
                            restore_unknown(&mut v, &ov);
                            serde_json::from_value::<Recipe>(v).map_err(invalid)?;
                            return Err(NcError::Usage(format!(
                                "{context}: `{key}` is null, and a `null` states nothing — \
                                 it cannot unset the roll's value. Omit `{key}` to use the \
                                 roll's value"
                            )));
                        }
                        let states = |key: &str| {
                            ov.pointer(&format!("/roll/{key}"))
                                .is_some_and(|v| !v.is_null())
                        };
                        if states("frame_exposure")
                            && !states("thin_slope")
                            && !states("thin_exposure")
                        {
                            reject_a_small_lift_beside_a_thin_one(
                                &r,
                                &format!("{context}: `roll.frame_exposure`"),
                                ThinLiftNames::KEYS,
                            )?;
                        }
                        if states("thin_slope") && !states("thin_exposure") {
                            reject_a_thin_slope_over_a_small_lift(
                                &r,
                                &format!("{context}: `roll.thin_slope`"),
                                ThinLiftNames::KEYS,
                            )?;
                        }
                        let name = mf.input.file_name().and_then(|n| n.to_str());
                        validate_roll_recipe(&r, Some(&context), name.map(|n| (n, &stated.roll)))?;
                        for msg in roll_wide_breaks(&mf.input, &r, shared)? {
                            log.warn(&msg);
                            roll_warnings.push(msg);
                        }
                        (r, Some(ov))
                    }
                    None => (own, None),
                };
                let output = resolve_frame_output(
                    mf.output.as_deref(),
                    &mf.input,
                    out_dir,
                    OutputTarget::resolve(&recipe, KnobNames::KeyOnly, false)?,
                )?;
                planned.push(PlannedFrame {
                    film_rgb: film_rgb(&mf.input, &output),
                    input: mf.input,
                    output,
                    recipe,
                    input_from_cli: overrides.as_ref().map_or(from_cli, |ov| from_cli.under(ov)),
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
            let target = OutputTarget::resolve(shared, KnobNames::FlagAndKey, false)?;
            for input in inputs {
                let output = default_output_name(&input, out_dir, target);
                let recipe = own(&input);
                planned.push(PlannedFrame {
                    film_rgb: film_rgb(&input, &output),
                    input,
                    output,
                    recipe,
                    overrides: None,
                    input_from_cli: from_cli,
                });
            }
        }
    }
    Ok(planned)
}

/// The path of the first `null` in `v` (`look.contrast`, `roll.white_balance[1]`),
/// depth first in key order. A frame's `params` refuses one: the merge skips a `null`,
/// so a hand-written one (an attempt to unset) would silently do nothing.
fn first_null_key(v: &serde_json::Value) -> Option<String> {
    use serde_json::Value;
    let join = |head: String, rest: String| match rest.starts_with('[') {
        true => format!("{head}{rest}"),
        false => format!("{head}.{rest}"),
    };
    match v {
        Value::Object(map) => map.iter().find_map(|(k, v)| match v {
            Value::Null => Some(k.clone()),
            _ => first_null_key(v).map(|rest| join(k.clone(), rest)),
        }),
        Value::Array(items) => items.iter().enumerate().find_map(|(i, v)| match v {
            Value::Null => Some(format!("[{i}]")),
            _ => first_null_key(v).map(|rest| join(format!("[{i}]"), rest)),
        }),
        _ => None,
    }
}

/// `v` with every member that holds a `null` anywhere left out, so what remains can be
/// checked for unknown keys before [`first_null_key`] refuses the null.
fn without_nulls(v: &serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Object(map) => map
            .iter()
            .filter(|(_, v)| !v.is_null() && (v.is_object() || first_null_key(v).is_none()))
            .map(|(k, v)| (k.clone(), without_nulls(v)))
            .collect(),
        _ => v.clone(),
    }
}

/// Put back into `doc` (a full recipe document) the members of `overlay` it has no key
/// for — what [`without_nulls`] dropped at an unknown key — so deserializing `doc` names
/// that key as unknown. A variant switch is not an unknown key: its new tag stays out,
/// as the merge would replace the value, and the null in it is refused by path.
fn restore_unknown(doc: &mut serde_json::Value, overlay: &serde_json::Value) {
    if recipe::is_variant_switch(doc, overlay) {
        return;
    }
    let (serde_json::Value::Object(d), serde_json::Value::Object(o)) = (doc, overlay) else {
        return;
    };
    for (k, v) in o {
        match d.get_mut(k) {
            Some(existing) => restore_unknown(existing, v),
            None => {
                d.insert(k.clone(), v.clone());
            }
        }
    }
}

/// Guard every roll write target (per-frame outputs and exports, `--report-file`)
/// against every input scan and against one another — so a same-stem collision or
/// a target aimed at an input fails loudly up front rather than clobbering a scan
/// or a just-written sibling. The roll-input analogue of
/// [`ensure_write_targets_distinct`] (multiple inputs, case-insensitivity-aware).
fn ensure_roll_targets_distinct(
    inputs: &[(&Path, &str)],
    targets: &[(String, PathBuf, RollTarget)],
) -> Result<()> {
    let input_keys: Vec<(PathBuf, &str)> = inputs
        .iter()
        .map(|(p, what)| (collision_key(p), *what))
        .collect();
    let mut seen: Vec<(&str, PathBuf, RollTarget)> = Vec::with_capacity(targets.len());
    for (label, path, kind) in targets {
        let key = collision_key(path);
        if let Some((_, what)) = input_keys.iter().find(|(ik, _)| keys_collide(ik, &key)) {
            return Err(NcError::Usage(format!(
                "{label} ({}) would overwrite {what}",
                path.display()
            )));
        }
        if let Some((other, _, other_kind)) = seen.iter().find(|(_, k, _)| keys_collide(k, &key)) {
            let remedy = film_rgb_clash_remedy(*kind, *other_kind)
                .map(|r| {
                    format!(
                        ": a frame's film RGB export is <input-stem>_film-rgb.tiff in its \
                         output's folder, so {r}"
                    )
                })
                .unwrap_or_default();
            return Err(NcError::Usage(format!(
                "{label} ({}) collides with {other}{remedy}",
                path.display()
            )));
        }
        seen.push((label.as_str(), key, *kind));
    }
    Ok(())
}

/// Who owns a roll write target; frames are numbered in plan order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RollTarget {
    Output(usize),
    FilmRgb(usize),
    Other,
}

/// The fix for two clashing roll targets when a film RGB export is one of them. An
/// export's name is derived from its frame's output, so only moving or renaming an
/// output separates them.
fn film_rgb_clash_remedy(a: RollTarget, b: RollTarget) -> Option<&'static str> {
    use RollTarget::{FilmRgb, Output};
    match (a, b) {
        (Output(i), FilmRgb(j)) | (FilmRgb(j), Output(i)) if i == j => {
            Some("rename this frame's output")
        }
        (Output(_), FilmRgb(_)) | (FilmRgb(_), Output(_)) => {
            Some("rename that output, or give the two frames' outputs different folders")
        }
        (FilmRgb(_), FilmRgb(_)) => Some("give the two frames' outputs different folders"),
        _ => None,
    }
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
            hdr_linear_tiff: report.hdr_linear_tiff.map(Box::new),
            hdr_coded_tiff: report.hdr_coded_tiff.map(Box::new),
            film_rgb_exported: report.film_rgb_exported,
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

/// How [`reject_a_small_lift_beside_a_thin_one`]'s remedies spell each knob.
struct ThinLiftNames {
    thin_off: &'static str,
    small_on: &'static str,
    small_off: &'static str,
    thin_exposure: &'static str,
}

impl ThinLiftNames {
    const FLAGS: Self = Self {
        thin_off: "--thin-lift off",
        small_on: "--small-lift on",
        small_off: "--small-lift off",
        thin_exposure: "--roll-thin-exposure",
    };
    const KEYS: Self = Self {
        thin_off: "`roll.thin_lift` \"off\"",
        small_on: "`roll.small_lift` \"on\"",
        small_off: "`roll.small_lift` \"off\"",
        thin_exposure: "`roll.thin_exposure`",
    };
}

/// The mirror of [`reject_a_small_lift_beside_a_thin_one`]: a thin slope typed (or stated
/// in a manifest's `params`) with no thin exposure replaces the frame's small lift, whose
/// exposure would be dropped silently. The caller spares one stated with its exposure.
fn reject_a_thin_slope_over_a_small_lift(
    r: &Recipe,
    stated: &str,
    names: ThinLiftNames,
) -> Result<()> {
    if !r.applies_roll_to_scene_correction()
        || r.roll.applied_thin_slope().is_none()
        || r.roll.applied_thin_exposure().is_some()
    {
        return Ok(());
    }
    let Some(ev) = r
        .roll
        .frame_exposure
        .filter(|_| r.roll.small_lift != Some(recipe::Switch::Off))
    else {
        return Ok(());
    };
    Err(NcError::Usage(format!(
        "{stated} replaces this frame's small lift of {ev} EV, and with no thin exposure \
         beside it that exposure would be dropped. To keep it, add {} {ev}; to drop it, use \
         {}",
        names.thin_exposure, names.small_off
    )))
}

/// A small lift typed (or stated in a manifest's `params`) would be ignored beside a thin
/// lift from elsewhere: a presence rule. Stated with a thin value in the same place it
/// is the measured shape (`frames[].flag`, a dump), the fallback for `--thin-lift off`, so
/// the caller spares it, as it does a recipe file's.
fn reject_a_small_lift_beside_a_thin_one(
    r: &Recipe,
    stated: &str,
    names: ThinLiftNames,
) -> Result<()> {
    let Some(slope) = r.roll.applied_thin_slope() else {
        return Ok(());
    };
    if !r.applies_roll_to_scene_correction() {
        return Ok(());
    }
    let off = if r.roll.small_lift == Some(recipe::Switch::Off) {
        format!("{} and {}", names.thin_off, names.small_on)
    } else {
        names.thin_off.to_owned()
    };
    Err(NcError::Usage(format!(
        "{stated} is this frame's small lift, but its thin lift (slope {slope}) applies in \
         place of it, so the small lift would be ignored. To render the small lift instead, \
         use {off}; to change the thin lift's exposure, use {}",
        names.thin_exposure
    )))
}

/// `roll`: a frame's lift (`roll.frame_exposure`, `roll.thin_slope`, `roll.thin_exposure`)
/// in the shared recipe or typed lifts every frame alike, the bright ones too — one frame's
/// lift (a `convert --dump-params` file carries it) or a whole-roll adjustment in the wrong
/// key. Spared under `direct` and the film master, which apply no roll value; not under a
/// lift switched off, which a manifest can undo.
fn reject_a_lift_for_every_frame(args: &ConversionFlags, shared: &Recipe) -> Result<()> {
    if !shared.applies_roll_to_scene_correction() {
        return Ok(());
    }
    let r = &shared.roll;
    let typed = &args.roll;
    let lifts = [
        (
            "--roll-frame-exposure",
            "frame_exposure",
            typed.roll_frame_exposure.is_some(),
            r.frame_exposure.is_some(),
            "--exposure",
        ),
        (
            "--roll-thin-slope",
            "thin_slope",
            typed.roll_thin_slope.is_some(),
            r.thin_slope.is_some(),
            "--contrast",
        ),
        (
            "--roll-thin-exposure",
            "thin_exposure",
            typed.roll_thin_exposure.is_some(),
            r.thin_exposure.is_some(),
            "--exposure",
        ),
    ];
    // A typed one first: it is what the user can drop.
    let Some((flag, key, whole)) = lifts
        .iter()
        .find(|k| k.2)
        .or_else(|| lifts.iter().find(|k| k.3))
        .map(|&(flag, key, typed, _, whole)| {
            let name = if typed {
                flag.to_owned()
            } else {
                format!("the recipe's `roll.{key}`")
            };
            (name, key, whole)
        })
    else {
        return Ok(());
    };
    Err(NcError::Usage(format!(
        "{flag} is one frame's own lift, and on `roll` it would lift every frame alike. \
         State each frame's in the shared recipe's `roll.frames` (as `hanten measure-roll \
         --out` writes it) or as `roll.{key}` in a --frames manifest's `params`; to move the \
         whole roll, use {whole}"
    )))
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
    // As on `convert`: removed flags first, then the recipes, then the flags over them.
    reject_deprecated_input_flags(&args.knobs.input_opts)?;
    reject_removed_flags(&args.knobs, !args.recipe_in.is_empty(), true)?;
    if let Some(Some(path)) = &args.export_film_rgb {
        return Err(NcError::Usage(format!(
            "--export-film-rgb takes no path on roll: each frame's film RGB is written as \
             <input-stem>_film-rgb.tiff beside its output. Drop {}, or, if it is a scan, put \
             the flag after the inputs",
            path.display()
        )));
    }
    let loaded = load_recipes(&args.recipe_in)?;
    // The roll's recipe, flags applied: what the roll-wide warnings compare each frame
    // against, so a value a flag set is not read as a frame's break. Each frame resolves
    // from the recipes alone (`resolve_frames`), since its `roll.frames` entry comes
    // before the flags.
    let stated = loaded.recipe;
    let shared = recipe::merge(stated.clone(), &args.knobs);
    reject_a_line_without_its_gains(&args.knobs, &shared)?;
    reject_roll_flags_nothing_applies(&args.knobs, &shared)?;
    reject_a_lift_for_every_frame(&args.knobs, &shared)?;

    // Validated once up front so a broken recipe fails loudly before any frame is
    // touched. Roll-specific rejections run first and the missing-base rule last
    // (`validate_roll_recipe`): a recipe that is both baseless and roll-invalid should
    // surface the roll problem first, or the user adds a base only to meet a second
    // error.
    validate_roll_recipe(&shared, None, None)?;

    // A roll's headline guarantee is one frozen, roll-fixed film base shared by
    // every frame. Only an *explicit* base delivers that: a region re-reads `Dmin`
    // from each frame's own pixels, so the roll is neither
    // frozen nor color-consistent even though the report still prints "one shared
    // recipe". Warn loudly (report + stderr, `--strict`-promotable) rather than
    // hard-failing, so a best-effort batch stays usable.
    let mut roll_warnings: Vec<String> = Vec::new();
    // A frozen recipe replayed under a different behavioral `pipeline_version` than
    // it was captured under is a roll-level fact (one shared recipe, N frames), so
    // it rides `roll_warnings` rather than any single frame's list.
    for msg in loaded.provenance_warnings {
        log.warn(&msg);
        roll_warnings.push(msg);
    }
    // The shared recipe's fallbacks and possible leftovers, once for the roll; a typed
    // style flag never warns. A per-frame override is that frame's explicit choice, so it
    // does not warn either.
    for msg in shared.recipe_warnings(recipe::TypedStyle::of(&args.knobs)) {
        log.warn(&msg);
        roll_warnings.push(msg);
    }
    // `validate_shared` above already rejected `None`, so only a region reaches here.
    if matches!(
        shared.calibration.film_base,
        Some(FilmBaseSource::Region(_))
    ) {
        let msg = "roll film base is NOT frozen: calibration.film_base is a `region`, so \
             every frame reads its own Dmin — the roll is not color-consistent and the \
             shared recipe is not truly shared. Measure the base once — `hanten measure-roll \
             <frames> --unexposed <unexposed-frame> --out roll.json`, or `hanten measure-base \
             <unexposed-frame> --out base.json` — and pass that file as --params, or its base \
             as --film-base R,G,B."
            .to_string();
        log.warn(&msg);
        roll_warnings.push(msg);
    }

    // Resolve the plan. A per-frame override that changes a roll-wide value appends
    // its own roll-level warning here (warn-and-continue, like the not-frozen warning
    // above), so `roll_warnings` is passed in to collect it.
    let planned = resolve_frames(&args, &stated, &shared, &mut roll_warnings, &log)?;

    // Guard every write target (per-frame outputs and exports, the report file) against every
    // input and against one another before writing anything. The `--frames` manifest
    // is a read input too — a write target aimed at it (e.g. `--report-file` equal to
    // the manifest path) must be rejected, not silently clobbered — so include it in
    // the protected read set.
    let mut inputs: Vec<(&Path, &str)> = planned
        .iter()
        .map(|p| (p.input.as_path(), "an input scan"))
        .collect();
    inputs.extend(args.frames.as_deref().map(|p| (p, "the --frames manifest")));
    inputs.extend(recipe_files(&args.recipe_in).map(|p| (p.as_path(), "the --params recipe")));
    let mut targets: Vec<(String, PathBuf, RollTarget)> = Vec::new();
    for (i, pf) in planned.iter().enumerate() {
        targets.push((
            format!("output for {}", pf.input.display()),
            pf.output.clone(),
            RollTarget::Output(i),
        ));
        if let Some(film_rgb) = &pf.film_rgb {
            targets.push((
                format!("film RGB export for {}", pf.input.display()),
                film_rgb.clone(),
                RollTarget::FilmRgb(i),
            ));
        }
    }
    if let Some(rf) = args.report.report_file.as_deref() {
        targets.push((
            "--report-file".to_string(),
            rf.to_path_buf(),
            RollTarget::Other,
        ));
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

    let read_files: Vec<&Path> = recipe_files(&args.recipe_in)
        .chain(args.frames.iter())
        .map(PathBuf::as_path)
        .collect();
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
            Exports {
                film_rgb: pf.film_rgb.as_deref(),
                pre_encode: None,
            },
            &pf.recipe,
            pf.input_from_cli,
            &read_files,
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
/// channels, bit depth, IR presence, scanner metadata) and the effective
/// measurement area. It measures no base: that is `measure-base`'s. No output image is
/// written.
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
    // scans it can diagnose comfortably. It gathers no film-base sample.
    let budget = args.memory.budget();
    report.memory = Some(preflight_memory(
        &args.input,
        RunProfile::DecodeOnly,
        SamplePlan::none(),
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

    // The IR usability verdict, and the effective measurement area it gates: the
    // holder march plus the static inset. The verdict is measured
    // (`ir-usability-detection`) — `--film-type` takes no part in it. Best-effort like
    // every other `inspect` diagnostic.
    let ir_separability = film_base::ir_separability(&image);
    let ir_usable = ir_separability.is_some_and(|s| s.usable);
    report.ir_separability = ir_separability;
    report.film_type = args.film_type.filter(|&t| t != FilmType::Unknown);
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

    // Each note says why the plane did not measure the holder, from what the march
    // returned (`effective_area.holder`), never predicted from the inputs.
    if info.ir_present && !image.ir_verified {
        push_warning(
            &mut report,
            &log,
            "an IR plane is present, but it is identified by shape alone (no \
             NewSubfileType=4 marker) and not trusted for holder measurement; the \
             effective area is the inset alone"
                .into(),
        );
    } else if info.ir_present && !ir_usable {
        push_warning(
            &mut report,
            &log,
            format!(
                "the IR plane cannot separate the film holder on this frame (interior \
                 IR transmission {:.4}); the effective area is the inset alone",
                ir_separability.map_or(0.0, |s| s.interior_median)
            ),
        );
    } else if info.ir_present
        && report
            .effective_area
            .is_some_and(|a: film_base::EffectiveArea| a.holder.is_none())
    {
        // Usable and trusted, yet the march declined: a frame too small to hold one
        // probe band.
        push_warning(
            &mut report,
            &log,
            "input carries an IR plane, but the frame is too small to measure the \
             holder on; the effective area is the inset alone"
                .into(),
        );
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

/// One film-base measurement's evidence: `measure-base`'s report fields, and
/// `measure-roll --unexposed`'s `unexposed` object. **One function produces it**
/// ([`measure_base`]), so the two commands cannot drift into different measurements.
#[derive(Clone, Debug, Serialize)]
struct BaseMeasurement {
    input: PathBuf,
    memory: MemoryReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    film_type: Option<FilmType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ir_separability: Option<film_base::IrSeparability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    effective_area: Option<film_base::EffectiveArea>,
    film_base: FilmBase,
    film_base_source: FilmBaseProvenance,
    #[serde(skip_serializing_if = "Option::is_none")]
    film_base_percentile: Option<f32>,
    #[serde(flatten)]
    reuse: Option<ReuseReady>,
}

/// What [`measure_base`] measures: which file, from what, and the evidence's inputs.
struct BaseRequest<'a> {
    input: &'a Path,
    /// A stated region or explicit base; `None` measures the frame's effective area at
    /// the median — the unexposed-frame workflow.
    source: Option<&'a FilmBaseSource>,
    /// The static inset of the effective area.
    inset: f32,
    /// The declared chemistry, echoed (it gates nothing).
    film_type: Option<FilmType>,
}

/// Measure the film base of `input`: memory preflight, decode, the IR verdict and
/// effective area as evidence, then the base — the area's median with no source, else
/// the stated source. Each warning is pushed to `warnings`, prefixed with the input
/// when `label` is set (a command measuring several files).
fn measure_base(
    req: &BaseRequest,
    budget: memory::Budget,
    label: bool,
    log: &Log,
    warnings: &mut Vec<String>,
) -> Result<BaseMeasurement> {
    let mut own = Vec::new();
    let result = measure_base_into(req, budget, log, &mut own);
    for w in own {
        let w = if label {
            format!("{}: {w}", req.input.display())
        } else {
            w
        };
        push_warning_buf(warnings, log, w);
    }
    result
}

/// [`measure_base`]'s body; its warnings are collected unprefixed and not yet logged.
fn measure_base_into(
    req: &BaseRequest,
    budget: memory::Budget,
    log: &Log,
    warnings: &mut Vec<String>,
) -> Result<BaseMeasurement> {
    let BaseRequest {
        input,
        source,
        inset,
        film_type,
    } = *req;
    // Decode-only: a stated region is gathered whole; the effective area is counted
    // into a fixed-size histogram. Collected here and echoed once by the caller,
    // prefixed, so not logged twice.
    let quiet = Log {
        verbose: log.verbose,
        quiet: true,
    };
    let memory = preflight_memory(
        input,
        RunProfile::DecodeOnly,
        source.map_or(SamplePlan::none(), sample_plan),
        budget,
        memory::detect_total_ram(),
        &quiet,
        warnings,
    )?;
    log_memory_preflight(&memory, log);

    let (image, info) = decode_within(input, budget.bytes())?;
    log.info(format_args!(
        "decoded {:?} {}x{} (ir={})",
        info.format, info.width, info.height, info.ir_present
    ));
    warnings.extend(info.warnings.iter().cloned());

    // The calibration command reports the IR verdict itself, not just a warning
    // about it: it decides whether the holder was measured.
    let ir_separability = film_base::ir_separability(&image);
    let area = film_base::effective_area(&image, inset);
    let (effective_area, film_base_source, est) = match source {
        // A stated source does not read the area, so a failure to resolve it is a
        // diagnostic, not a refusal.
        Some(source) => {
            let area = match area {
                Ok(area) => Some(area),
                Err(e) => {
                    // `message()`, not `{e}`: `Display` prefixes the kind.
                    warnings.push(format!(
                        "effective-area resolution skipped — {}",
                        e.message()
                    ));
                    None
                }
            };
            (
                area,
                FilmBaseProvenance::from(source),
                film_base::estimate(&image, source)?,
            )
        }
        // The base *is* the area's median, so an empty area refuses.
        None => {
            let area = area?;
            // The measurement then rests on the inset alone, which the user sizes —
            // true of a scan with no IR plane as much as of one whose plane declined.
            if area.holder.is_none() {
                warnings.push(holder_not_measured_note(&image, ir_separability));
            }
            (
                Some(area),
                FilmBaseProvenance::EffectiveArea,
                film_base::measure_area(&image, &area)?,
            )
        }
    };
    if let Some(area) = effective_area {
        warnings.extend(film_base::effective_area_warnings(&area));
    }
    warnings.extend(est.warnings);
    let film_base = est.base;

    // Reuse-ready only when the measurement passes the explicit-base validation
    // `convert` applies: a base outside `(0, 1]` on any channel (a channel `> 1`;
    // `<= 0` already errored at birth) is still reported, never as reusable. A warning
    // does not withhold it: `--strict` is the hard gate. (Design-spec §8.)
    let reuse = match reuse_ready(<[f32; 3]>::from(film_base)) {
        Some((flag, source)) => Some(ReuseReady { flag, source }),
        None => {
            warnings.push(format!(
                "measured base {:?} is not usable as an explicit --film-base \
                 (channels must be in (0, 1]) — was the sampled area unexposed \
                 film base? No reuse-ready output emitted",
                <[f32; 3]>::from(film_base)
            ));
            None
        }
    };

    Ok(BaseMeasurement {
        input: input.to_path_buf(),
        memory,
        film_type: film_type.filter(|&t| t != FilmType::Unknown),
        ir_separability,
        effective_area,
        film_base,
        film_base_source,
        film_base_percentile: est.percentile,
        reuse,
    })
}

/// Why the film holder went unmeasured, for a measurement that then rests on the
/// inset alone.
fn holder_not_measured_note(image: &LinearImage, sep: Option<film_base::IrSeparability>) -> String {
    let why = if image.ir.is_none() {
        "the scan has no IR plane".to_string()
    } else if !image.ir_verified {
        "its IR plane is identified by shape alone (no NewSubfileType=4 marker) and not \
         trusted"
            .to_string()
    } else if let Some(s) = sep.filter(|s| !s.usable) {
        format!(
            "its IR plane cannot separate the holder on this frame (interior IR \
             transmission {:.4})",
            s.interior_median
        )
    } else {
        "the frame is too small to measure the holder on".to_string()
    };
    format!(
        "the film holder was not measured — {why} — so the effective area is the \
         inset alone; raise --measure-inset if the holder reaches past it"
    )
}

/// `hanten measure-base` — measure the film base (`Dmin`) alone and emit it as JSON,
/// with a paste-ready `--film-base` flag when it is usable as an explicit base, and
/// with `--out` as a recipe. `--strict` promotes warnings to a failing exit after the
/// report is emitted, and then writes no recipe.
///
/// With no source flag it measures the frame's **effective area** at the median
/// (`film_base::measure_area`) — the unexposed-frame workflow. `--base-region` reads
/// that region at p97 instead, and `--film-base` echoes a value back through the same
/// checks.
///
/// **No fingerprint watches the unstated default.** `version::PIPELINE_FINGERPRINTS`
/// hashes what a *conversion* runs (a stated region), so changing the area
/// measurement would move every `measure-base` result with the drift gate green —
/// verify such a change by hand.
fn run_measure_base(args: MeasureBaseArgs) -> Result<()> {
    let started = Instant::now();
    let log = Log::new(&args.report);

    if args.d_max_region.is_some() {
        return Err(NcError::Usage(removed_dmax_message(
            "--d-max-region",
            "measured the roll reference density off a light-struck leader",
        )));
    }
    if args.grid {
        return Err(NcError::Usage(
            "--grid was removed: with no source flag `measure-base` measures the whole \
             effective area of the frame at its median, which is what the grid was for. \
             Drop the flag."
                .into(),
        ));
    }
    if args.film_base.auto_base {
        return Err(NcError::Usage(
            "--auto-base was removed with the rebate search: with no source flag \
             `measure-base` measures the frame's effective area. Drop the flag, and give it \
             the roll's unexposed frame."
                .into(),
        ));
    }

    // A bad `--measure-inset` is a *usage* error (exit 2), not a diagnostic that
    // degrades to a warning: this command resolves no recipe, so `validate` never
    // sees the flag. Checked before the decode, so a 160 MB read is not wasted on a
    // typo.
    if let Some(f) = args.measure.measure_inset {
        check_measure_inset(f)?;
    }

    let mut targets = Vec::new();
    if let Some(out) = args.out.out.as_deref() {
        targets.push(("--out", out));
    }
    if let Some(rf) = args.report.report_file.as_deref() {
        targets.push(("--report-file", rf));
    }
    ensure_write_targets_distinct(&args.input, &targets)?;
    check_recipe_out(&args.out)?;
    let source = film_base_source_override(&args.film_base);
    // Guard an explicit base with the same check `convert` applies: a bad
    // `--film-base` must fail loudly rather than be echoed back. Region bounds are
    // checked by `film_base::estimate`.
    if let Some(FilmBaseSource::Explicit(b)) = &source {
        validate_explicit_film_base(b)?;
    }

    let mut warnings = Vec::new();
    let req = BaseRequest {
        input: &args.input,
        source: source.as_ref(),
        inset: args.measure.measure_inset.unwrap_or(DEFAULT_MEASURE_INSET),
        film_type: args.film_type,
    };
    let m = measure_base(&req, args.memory.budget(), false, &log, &mut warnings)?;
    let recipe = m.reuse.as_ref().map(|r| MeasuredRecipe {
        recipe_version: recipe::RecipeVersion,
        input: None,
        calibration: recipe::Calibration {
            film_base: Some(r.source.clone()),
        },
        roll: None,
        measure: None,
        reconstruction: None,
    });
    let report = Report {
        command: Some("measure-base"),
        // Same contract as `inspect`: build identity on every report, no
        // `params_hash` because no recipe was resolved. A measured `Dmin` is
        // routinely frozen into a roll recipe, so which build measured it matters.
        identity: Some(Identity::new()),
        input: Some(m.input),
        memory: Some(m.memory),
        film_type: m.film_type,
        ir_separability: m.ir_separability,
        effective_area: m.effective_area,
        film_base: Some(m.film_base),
        film_base_source: Some(m.film_base_source),
        film_base_percentile: m.film_base_percentile,
        reuse: m.reuse,
        warnings,
        elapsed_ms: Some(elapsed_ms(started)),
        ..Report::default()
    };
    // Emit the report before the `--strict` gate so the machine-readable record (the
    // measured base) lands even when a warning then fails the run.
    emit_report(
        &report,
        args.report.report,
        args.report.report_file.as_deref(),
        &log,
    )?;
    if args.strict && !report.warnings.is_empty() {
        return Err(NcError::Other(format!(
            "--strict: {} warning(s) present (see report){}",
            report.warnings.len(),
            if args.out.out.is_some() {
                "; no recipe written"
            } else {
                ""
            }
        )));
    }
    if let Some(out) = args.out.out.as_deref() {
        let recipe = recipe.ok_or_else(|| {
            NcError::Other(format!(
                "the measured base is not usable as an explicit film base (see the \
                 report's warnings); {} not written",
                out.display()
            ))
        })?;
        write_recipe_out(out, &recipe, &log)?;
    }
    Ok(())
}

/// The recipe a measuring command writes with `--out`: only what the measurement
/// holds, so a later layer is pinned by nothing else. `measure-base` states
/// `calibration`; `measure-roll` states `calibration`, `roll` and the decode its gains
/// were measured through, plus `input` and `measure` when they are not the defaults.
/// The decode is stated even at its default, so a moved default cannot change it.
#[derive(Debug, Serialize)]
struct MeasuredRecipe {
    recipe_version: recipe::RecipeVersion,
    #[serde(skip_serializing_if = "Option::is_none")]
    input: Option<InputParams>,
    calibration: recipe::Calibration,
    #[serde(skip_serializing_if = "Option::is_none")]
    roll: Option<recipe::RollSection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    measure: Option<MeasureParams>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reconstruction: Option<fixed::DecodeParams>,
}

/// Refuse an existing `--out` file unless `--force`, before anything is decoded.
fn check_recipe_out(out: &RecipeOutArgs) -> Result<()> {
    match out.out.as_deref() {
        Some(path) if !out.force && path.exists() => Err(NcError::Usage(format!(
            "--out {} exists; pass --force to replace it",
            path.display()
        ))),
        _ => Ok(()),
    }
}

/// Write a `--out` recipe: pretty JSON with a trailing newline, staged so a failed
/// write leaves no partial file.
fn write_recipe_out(path: &Path, recipe: &MeasuredRecipe, log: &Log) -> Result<()> {
    let mut json = serde_json::to_string_pretty(&RecipeEnvelope::new(recipe))
        .map_err(|e| NcError::Other(format!("serializing recipe: {e}")))?;
    json.push('\n');
    for note in staged::stage_bytes(path, json.as_bytes())?.commit()? {
        log.warn_always(&note);
    }
    log.info(format_args!("wrote recipe {}", path.display()));
    Ok(())
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
    /// The `--unexposed` frame's base measurement, the evidence `measure-base` reports.
    #[serde(skip_serializing_if = "Option::is_none")]
    unexposed: Option<BaseMeasurement>,
    /// The film base every input was decoded with.
    film_base: FilmBase,
    /// The decode the gains belong to: they are measured at its output.
    decode: fixed::DecodeReport,
    /// The leader guard, when `--leader` was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    leader: Option<MeasuredLeader>,
    frames: Vec<MeasuredFrame>,
    white_balance: RollWhiteBalance,
    /// The roll's midtone line (`nf-scene-correction/midtone-neutral`).
    midtone_neutral: MeasuredMidtone,
    /// How far to trust the white balance and the line
    /// (`nf-scene-correction/correction-confidence`).
    confidence: MeasuredConfidence,
    /// The roll's white and the slope that places it (`nf-calibration/roll-white-rule`).
    white: MeasuredRollWhite,
    /// The roll's exposure (`nf-calibration/roll-exposure`).
    exposure: MeasuredRollExposure,
    /// Each frame's lift (`nf-calibration/frame-level-trim`).
    small_lift: MeasuredSmallLift,
    /// The thin-frame lift (`nf-calibration/thin-frame-lift`).
    thin_lift: MeasuredThinLift,
    /// The gains, the white and the exposure in the forms a user freezes them in.
    reuse: RollReuse,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
    elapsed_ms: f64,
}

/// The report's `confidence`: each measured correction's grade
/// ([`correction_confidence::assess`]), and advice when one is in doubt. Advisory, not a
/// warning: `--strict` ignores it, since a whole short roll has no more frames to give.
#[derive(Debug, Serialize)]
struct MeasuredConfidence {
    white_balance: correction_confidence::Assessment,
    /// Absent when no line was written.
    #[serde(skip_serializing_if = "Option::is_none")]
    midtone_neutral: Option<correction_confidence::Assessment>,
    /// [`Self::advice_for`], also printed as a `note:` line.
    #[serde(skip_serializing_if = "Option::is_none")]
    advice: Option<String>,
}

impl MeasuredConfidence {
    fn new(
        white_balance: correction_confidence::Assessment,
        midtone_neutral: Option<correction_confidence::Assessment>,
    ) -> Self {
        let mut c = Self {
            white_balance,
            midtone_neutral,
            advice: None,
        };
        c.advice = c.advice_for();
        c
    }

    /// One message naming every correction in doubt, its frame count, and what to compare.
    fn advice_for(&self) -> Option<String> {
        let doubt = |a: &correction_confidence::Assessment| {
            (a.tier == correction_confidence::Tier::InDoubt).then_some(a.frames)
        };
        let gains = doubt(&self.white_balance);
        let line = self.midtone_neutral.as_ref().and_then(doubt);
        let (what, it) = match (gains, line) {
            (None, None) => return None,
            (Some(g), Some(l)) if g == l => (
                format!("the roll's white balance and midtone line were measured from {g} frames"),
                "them",
            ),
            (Some(g), Some(l)) => (
                format!(
                    "the roll's white balance was measured from {g} frames and its midtone \
                     line from {l}"
                ),
                "them",
            ),
            (Some(g), None) => (
                format!("the roll's white balance was measured from {g} frames"),
                "it",
            ),
            (None, Some(l)) => (
                format!("the roll's midtone line was measured from {l} frames"),
                "it",
            ),
        };
        let compare = match (gains, self.midtone_neutral) {
            (Some(_), Some(_)) => {
                "`--neutral-balance off` (`convert`, `roll`), which turns the line off with it"
            }
            (Some(_), None) => "`--neutral-balance off` (`convert`, `roll`)",
            (None, _) => "`--midtone-neutral off` (`convert`, `roll`)",
        };
        Some(format!(
            "{what}: under {}, which frames a roll holds can move {it} as far as the cast a \
             whole roll leaves, so trust {it} less. Compare a frame rendered with {compare}, \
             or measure with more of the roll's frames if it has more",
            self.white_balance.confident_from
        ))
    }
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
    /// leader guard and after the roll's colour correction — in scene stops above
    /// mid-grey; absent when no pixel was usable.
    #[serde(skip_serializing_if = "Option::is_none")]
    white_stops: Option<f32>,
    /// The same before the roll's colour correction: what the saturation check reads.
    #[serde(skip_serializing_if = "Option::is_none")]
    decoded_white_stops: Option<f32>,
    /// How far the decoded white sits under the leader, in scene stops; only with
    /// `--leader`.
    #[serde(skip_serializing_if = "Option::is_none")]
    leader_distance_stops: Option<f32>,
    /// The frame's level — the log-average of its luma — in scene stops from mid-grey;
    /// absent when no pixel was usable or the leader guard took every one.
    #[serde(skip_serializing_if = "Option::is_none")]
    level_stops: Option<f32>,
    /// The frame's part in the roll's white.
    white_role: roll_white::FrameRole,
    /// The frame's tonal shape (`roll_white::frame_tones`): its luma's p5–p95 spread and
    /// the share near the film base; absent when no pixel was usable.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    tones: Option<roll_white::FrameTones>,
    /// One surface filling the frame (`roll_white::FLAT_SPREAD_STOPS`): it gets no lift.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    flat: bool,
    /// The frame's lift in EV (`roll_white::small_lift`), from its white and the roll's
    /// exposure, 0 on a flat frame; absent without a white. Measured under
    /// `--no-small-lift` too, which only keeps it out of the recipe.
    #[serde(skip_serializing_if = "Option::is_none")]
    lift_ev: Option<f32>,
    /// The thin-frame lift a qualifying frame gets (`roll_white::thin_lift`); measured
    /// under `--no-thin-lift` too, which only keeps it out of the recipe.
    #[serde(skip_serializing_if = "Option::is_none")]
    thin_lift: Option<roll_white::ThinLift>,
    /// This frame's own `convert` flags, when they differ from `reuse.flag` (a clamped
    /// white or a lift written to the recipe).
    #[serde(skip_serializing_if = "Option::is_none")]
    flag: Option<String>,
    memory: MemoryReport,
}

/// Which lifts the recipe carries, each on unless its `--no-*` flag: the small one under
/// `--no-small-lift` is left out, the thin one under `--no-thin-lift`. A thin frame
/// carries both, so `convert --thin-lift off` renders its small lift.
#[derive(Clone, Copy)]
struct Lifts {
    small: bool,
    thin: bool,
}

/// The lifts the recipe carries for one frame ([`MeasuredFrame::written`]).
#[derive(Clone, Copy, Default, PartialEq)]
struct Written {
    /// The small lift, `roll.frame_exposure` — never a zero one.
    exposure: Option<f32>,
    /// The thin lift's slope and exposure, `roll.thin_slope` and `roll.thin_exposure`.
    thin: Option<(f32, f32)>,
}

impl MeasuredFrame {
    fn written(&self, lifts: Lifts) -> Written {
        Written {
            exposure: self.lift_ev.filter(|&ev| lifts.small && ev > 0.0),
            thin: self
                .thin_lift
                .filter(|_| lifts.thin)
                .map(|t| (t.slope, t.exposure)),
        }
    }
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
    /// The slope `roll.white_stops` renders at: the white at diffuse white, mid-grey
    /// pinned, at `look.contrast` 1 and no exposure. The white is measured before the
    /// roll's exposure, which the look expands too, so the roll's white renders
    /// `exposure.ev · slope` stops off diffuse white. The recipe stores `stops`.
    slope: f32,
    /// Frames above the cap, rendered at the cap's slope rather than the roll's.
    /// Disclosed, not warned about: an ordinary bright scene lands here too.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    clamped: Vec<ClampedFrame>,
    rule: WhiteRule,
}

#[derive(Debug, Serialize)]
struct ClampedFrame {
    input: PathBuf,
    white_stops: f32,
    /// Its slope, the cap's — against the roll's `slope`.
    slope: f32,
    /// This frame's own `convert` flags: `reuse.flag` carries the roll's white, which
    /// would undo the clamp on this frame.
    flag: String,
}

/// The midtone line, why there is none, and its rule.
#[derive(Debug, Serialize)]
struct MeasuredMidtone {
    /// `correction` (design-spec §6), switched off at render by `--midtone-neutral off`.
    kind: &'static str,
    mode: MidtoneMode,
    #[serde(flatten)]
    measured: midtone_neutral::Measured,
    /// Under `auto`, a roll with fewer frames gets no line.
    min_frames: usize,
    min_bands: usize,
    fade_stops: f32,
    gate_log2: [f32; 2],
}

/// The roll's exposure and the rule that measured it.
#[derive(Debug, Serialize)]
struct MeasuredRollExposure {
    #[serde(flatten)]
    measured: roll_white::RollExposure,
    /// Where the exposure puts the median frame level, in scene stops from mid-grey.
    target_stops: f32,
    /// The most it moves either way, in EV.
    bound_ev: f32,
}

/// The roll's colour correction: its white-balance gains and its midtone line.
#[derive(Clone, Copy)]
struct RollColour {
    gains: [f32; 3],
    line: Option<MidtoneLine>,
}

/// The `convert` flags that freeze the roll's `colour`, the white `white_stops`, the
/// exposure `ev` and a frame's `written` lifts — `reuse.flag`, and a clamped or lifted
/// frame's own.
fn reuse_flag(colour: RollColour, white_stops: f32, ev: f32, written: Written) -> String {
    let gains = colour.gains;
    let mut flag = format!(
        "--roll-white-balance {},{},{} --roll-white {white_stops} --roll-exposure {ev}",
        gains[0], gains[1], gains[2]
    );
    if let Some(l) = colour.line {
        flag += &format!(
            " --roll-midtone-line {},{},{},{},{},{},{}",
            l.red[0], l.red[1], l.blue[0], l.blue[1], l.bands[0], l.bands[1], l.fade_end_stops
        );
    }
    if let Some(lift) = written.exposure {
        flag += &format!(" --roll-frame-exposure {lift}");
    }
    if let Some((slope, exposure)) = written.thin {
        flag += &format!(" --roll-thin-slope {slope} --roll-thin-exposure {exposure}");
    }
    flag
}

/// The thin-frame lift's rule and whether the recipe carries it.
#[derive(Debug, Serialize)]
struct MeasuredThinLift {
    /// `taste`: a preference, not a correction (design-spec §6); `convert --thin-lift off`
    /// turns it off without re-measuring.
    kind: &'static str,
    /// False under `--no-thin-lift`: each qualifying frame's `thin_lift` is then reported,
    /// not written.
    written: bool,
    /// Frames the recipe gives one.
    lifted: usize,
    /// The decoded film base's luma, in scene stops: what the lift holds (about) still —
    /// `roll_white::thin_lift` says why about.
    base_stops: f32,
    /// A frame qualifies at or under this rendered white…
    white_stops: f32,
    /// …with at least this share of its luma within `near_base_stops` of the base.
    base_share: f32,
    near_base_stops: f32,
    /// About how far the lift raises the white, in rendered stops, unless the slope bound
    /// holds it under.
    lift_stops: f32,
    slope_bound: f32,
    /// Thin frames the slope bound holds under `lift_stops` (each `frames[].thin_lift`
    /// says how far it rises). Disclosed, not warned about, like `white.clamped`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    bounded: Vec<PathBuf>,
    /// Thin frames no thin lift fits — the slope is already at the bound, the white is not
    /// above the base, or the lift would darken the mid-tones; each has its small lift only.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    unlifted: Vec<PathBuf>,
}

/// The per-frame lift's rule and whether the recipe carries it.
#[derive(Debug, Serialize)]
struct MeasuredSmallLift {
    /// `taste`, like `thin_lift.kind`; `convert --small-lift off` turns it off.
    kind: &'static str,
    /// False under `--no-small-lift`: each frame's `lift_ev` is reported, not written.
    written: bool,
    /// Frames the recipe gives the small lift, a thin frame's included (its thin lift
    /// replaces it while on).
    lifted: usize,
    bound_ev: f32,
    /// A frame whose rendered white (its white plus the roll's exposure) is at or under
    /// this gets the whole bound; at or over `none_stops`, no lift.
    full_stops: f32,
    none_stops: f32,
    /// A frame whose luma spans fewer stops from p5 to p95 is flat, and gets no lift.
    flat_spread_stops: f32,
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

/// The gains, the white and the exposure as `convert` flags. The recipe form is the
/// `--out` file.
#[derive(Debug, Serialize)]
struct RollReuse {
    /// For `convert`, on every frame but a clamped or lifted one, which takes its own
    /// `frames[].flag`.
    flag: String,
}

/// Which scan [`decode_for_roll_white`] reads. Only a picture frame gets the polarity
/// warning: its thresholds were measured on picture frames, and a leader can show the
/// cut tongue or backlight.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RollScan {
    Leader,
    Frame,
}

/// One input decoded into linear ACEScg at the recipe's decode, plus its effective
/// area: the front half of `convert_frame`, gated the same way
/// (memory preflight, input semantics, polarity warning on a [`RollScan::Frame`]) and
/// stopping before scene correction — the point the roll's gains will be applied at.
///
/// `on_film` reads the decode's film RGB just before the working-space map, for what is
/// measured there (the roll's white); it runs on the frame's effective area as found.
///
/// **A copy, and it must stay in step.** `convert_frame`'s front half is tangled with
/// its report, so this repeats its gates rather than sharing them: a
/// refusal or measurement-region warning added there belongs here too, or gains get
/// frozen from frames `convert` would refuse.
// One over clippy's argument cap; the scan kind is one more one-off value.
#[allow(clippy::too_many_arguments)]
fn decode_for_roll_white<T>(
    input: &Path,
    scan: RollScan,
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
    let area = film_base::effective_area(&image, recipe.measure.inset);
    if scan == RollScan::Frame
        && let Ok(a) = &area
        && let Some(w) = film_base::polarity_warning(&image, a.region, base)?
    {
        push_warning_buf(warnings, log, format!("{}: {w}", input.display()));
    }
    let (film, decoded) = fixed::decode(image, base, &recipe.reconstruction)?;
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
    colour: RollColour,
    ev: f32,
) -> Result<MeasuredRollWhite> {
    let stops: Vec<Option<f32>> = frames.iter().map(|f| f.white_stops).collect();
    let placed = roll_white::place_roll_white(&stops)?;
    for (f, role) in frames.iter_mut().zip(&placed.roles) {
        f.white_role = *role;
    }
    let cap_slope = roll_white::slope_for(roll_white::WHITE_CAP_STOPS);
    Ok(MeasuredRollWhite {
        stops: placed.stops,
        bound: placed.bound,
        from: frames
            .iter()
            .find(|f| f.white_role == roll_white::FrameRole::SetsRoll)
            .map(|f| f.input.clone()),
        slope: roll_white::slope_for(placed.stops),
        clamped: frames
            .iter()
            .filter(|f| f.white_role == roll_white::FrameRole::Clamped)
            .map(|f| ClampedFrame {
                input: f.input.clone(),
                white_stops: f.white_stops.expect("a clamped frame has a white"),
                slope: cap_slope,
                flag: reuse_flag(colour, roll_white::WHITE_CAP_STOPS, ev, Written::default()),
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

/// Each frame's own `convert` flags, where they differ from `reuse.flag` — the frames
/// [`frame_table`] gives an entry — and each clamped frame's, with its lift when written.
fn frame_flags(
    frames: &mut [MeasuredFrame],
    white: &mut MeasuredRollWhite,
    colour: RollColour,
    ev: f32,
    lifts: Lifts,
) {
    for f in frames.iter_mut() {
        let clamp = own_white(f, white);
        let written = f.written(lifts);
        f.flag = (clamp.is_some() || written != Written::default())
            .then(|| reuse_flag(colour, clamp.unwrap_or(white.stops), ev, written));
    }
    for c in &mut white.clamped {
        let written = frames
            .iter()
            .find(|f| f.input == c.input)
            .map_or(Written::default(), |f| f.written(lifts));
        c.flag = reuse_flag(colour, roll_white::WHITE_CAP_STOPS, ev, written);
    }
}

/// A clamped frame's own white, the cap — unless the roll's white is the cap itself.
fn own_white(f: &MeasuredFrame, white: &MeasuredRollWhite) -> Option<f32> {
    (f.white_role == roll_white::FrameRole::Clamped && white.bound != roll_white::WhiteBound::Cap)
        .then_some(roll_white::WHITE_CAP_STOPS)
}

/// The recipe's `roll.frames`: each clamped frame at its own white ([`own_white`]); each
/// lifted frame with the lifts written ([`MeasuredFrame::written`]). Keyed by file name,
/// which [`refuse_shared_file_names`] made unique.
fn frame_table(
    frames: &[MeasuredFrame],
    white: &MeasuredRollWhite,
    lifts: Lifts,
) -> BTreeMap<String, recipe::FrameRoll> {
    frames
        .iter()
        .filter_map(|f| {
            let written = f.written(lifts);
            let entry = recipe::FrameRoll {
                white_stops: own_white(f, white),
                exposure: written.exposure,
                thin_slope: written.thin.map(|t| t.0),
                thin_exposure: written.thin.map(|t| t.1),
            };
            let name = f.input.file_name()?.to_str()?.to_owned();
            (entry != recipe::FrameRoll::default()).then_some((name, entry))
        })
        .collect()
}

/// `roll.frames` keys a frame by its file name, so two inputs sharing one — or one that
/// has none a recipe can state — would make the table ambiguous.
fn refuse_shared_file_names(inputs: &[PathBuf]) -> Result<()> {
    let mut seen: Vec<(&str, &Path)> = Vec::new();
    for input in inputs {
        let Some(name) = input.file_name().and_then(|n| n.to_str()) else {
            return Err(NcError::Usage(format!(
                "{}: --out keys a frame by its file name in `roll.frames`, and this path has \
                 no UTF-8 file name",
                input.display()
            )));
        };
        if let Some((_, first)) = seen.iter().find(|(n, _)| *n == name) {
            return Err(NcError::Usage(format!(
                "{} and {} share the file name {name:?}; --out keys a frame by its file name \
                 in `roll.frames`, so rename one",
                first.display(),
                input.display()
            )));
        }
        seen.push((name, input));
    }
    Ok(())
}

/// `hanten measure-roll` — measure what a roll shares once: with `--unexposed` its film
/// base ([`measure_base`]), then its white balance, midtone line, white and exposure over its picture frames;
/// report them, and with `--out` write them as one recipe `roll --params` renders alone.
fn run_measure_roll(args: MeasureRollArgs) -> Result<()> {
    let started = Instant::now();
    let log = Log::new(&args.report);
    if args.removed_no_frame_lift {
        return Err(NcError::Usage(
            "--no-frame-lift was removed: it left out both a frame's lifts, which now each \
             have their own flag. To measure as before, pass --no-small-lift --no-thin-lift."
                .into(),
        ));
    }

    let loaded = load_recipes(&args.recipe_in)?;
    let mut recipe = loaded.recipe;
    // A measured base is not overridden: the unexposed frame is refused beside any
    // other statement of the base, before a rule about that base could refuse first.
    if let Some(unexposed) = &args.unexposed {
        let stated = if args.film_base.is_some() {
            Some("--film-base")
        } else if recipe.calibration.film_base.is_some() {
            Some("the recipe's `calibration.film_base`")
        } else {
            None
        };
        if let Some(stated) = stated {
            return Err(NcError::Usage(format!(
                "--unexposed {} measures the film base, and {stated} states one; the base \
                 is measured or stated, never both. Drop one of them",
                unexposed.display()
            )));
        }
    }
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
    // The unexposed frame likewise: it is film base, not picture.
    let mut seen: Vec<(PathBuf, &Path)> = Vec::new();
    let references = [
        ("--leader", "leader", args.leader.as_deref()),
        ("--unexposed", "unexposed frame", args.unexposed.as_deref()),
    ];
    for (flag, what, path) in references {
        let Some(path) = path else { continue };
        let key = collision_key(path);
        if let Some(input) = args
            .inputs
            .iter()
            .find(|i| keys_collide(&collision_key(i), &key))
        {
            return Err(NcError::Usage(format!(
                "{} is both the {flag} and an input frame; every input is pooled as \
                 picture, so leave the {what} out of the frames",
                input.display()
            )));
        }
    }
    if let (Some(leader), Some(unexposed)) = (&args.leader, &args.unexposed)
        && keys_collide(&collision_key(leader), &collision_key(unexposed))
    {
        return Err(NcError::Usage(format!(
            "{} is both the --leader and the --unexposed frame; the leader is fully \
             exposed and the unexposed frame is not exposed at all",
            leader.display()
        )));
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
    // After the exact repeat above: a file named twice is that fault, not a clash.
    if args.out.out.is_some() {
        refuse_shared_file_names(&args.inputs)?;
    }
    let stated_base = match (&args.unexposed, &recipe.calibration.film_base) {
        (Some(_), _) => None,
        (None, Some(FilmBaseSource::Explicit(b))) => {
            validate_explicit_film_base(b)?;
            Some(FilmBase::from(*b))
        }
        (None, _) => {
            return Err(NcError::Usage(
                "measure-roll needs the roll's film base: `--unexposed <unexposed.tif>` to \
                 measure it here, or stated explicitly — `--film-base R,G,B`, or \
                 `calibration.film_base` as `{\"explicit\": [r, g, b]}` in the recipe (what \
                 `hanten measure-base --out` writes). A base estimated per frame would \
                 measure each frame under a different decode"
                    .into(),
            ));
        }
    };
    let mut targets = Vec::new();
    if let Some(out) = args.out.out.as_deref() {
        targets.push(("--out", out));
    }
    if let Some(rf) = args.report.report_file.as_deref() {
        targets.push(("--report-file", rf));
    }
    let read = args
        .inputs
        .iter()
        .map(|p| (p.as_path(), "an input scan"))
        .chain(args.leader.as_deref().map(|p| (p, "the --leader scan")))
        .chain(
            args.unexposed
                .as_deref()
                .map(|p| (p, "the --unexposed scan")),
        )
        .chain(recipe_files(&args.recipe_in).map(|p| (p.as_path(), "the --params recipe")));
    for (input, what) in read {
        ensure_write_targets_spare(input, what, &targets)?;
    }
    check_recipe_out(&args.out)?;

    let budget = args.memory.budget();
    let mut warnings = Vec::new();
    for msg in loaded.provenance_warnings {
        push_warning_buf(&mut warnings, &log, msg);
    }
    let mut decode = None;

    // The unexposed frame first: every other frame is decoded with its base. Measured
    // exactly as `measure-base` measures it with no source flag: its effective area
    // at the median.
    let unexposed = match &args.unexposed {
        Some(path) => {
            let req = BaseRequest {
                input: path,
                source: None,
                inset: recipe.measure.inset,
                film_type: None,
            };
            let m = measure_base(&req, budget, true, &log, &mut warnings)?;
            // Refused before this command's report, so the evidence is measure-base's.
            if m.reuse.is_none() {
                return Err(NcError::Other(format!(
                    "{}: the measured base {:?} is not usable as an explicit film base \
                     (channels must be in (0, 1]) — is it the roll's unexposed frame? \
                     `hanten measure-base` on it reports the evidence",
                    path.display(),
                    <[f32; 3]>::from(m.film_base)
                )));
            }
            Some(m)
        }
        None => None,
    };
    let base = match (&unexposed, stated_base) {
        (Some(m), _) => m.film_base,
        (None, Some(b)) => b,
        (None, None) => unreachable!("refused above"),
    };
    // Where every frame's shadows bottom out, in the luma the frames' tones are read in.
    let base_px =
        working_space::map_nc_film_rgb_v1(fixed::decode_film_base(&base, &recipe.reconstruction)?);
    let base_stops = roll_white::base_stops(
        base_px.rgb()[..3]
            .try_into()
            .expect("the decoded base is one pixel"),
    );

    let leader = match &args.leader {
        Some(path) => {
            // Each error keeps its kind, and so its exit code (a memory refusal stays 6).
            let (aces, _, film_peak, _, memory) = decode_for_roll_white(
                path,
                RollScan::Leader,
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
    // Each frame's film-RGB sample, unguarded: the midtone line's votes and fade end, and
    // the frame's white once the roll's colour correction is known.
    let mut film_samples = Vec::with_capacity(args.inputs.len());
    let mut frames = Vec::with_capacity(args.inputs.len());
    for input in &args.inputs {
        let (aces, area, film_sample, decoded, memory) = decode_for_roll_white(
            input,
            RollScan::Frame,
            &recipe,
            &base,
            budget,
            &log,
            &mut warnings,
            |film, area| match area {
                Ok(a) => roll_white::frame_sample(film.rgb(), film.width(), a.region)
                    .map_err(|e| e.prefixed(input.display())),
                // Refused just below, with the input named.
                Err(_) => Ok(Vec::new()),
            },
        )?;
        // As decoded: the leader's saturation is the film's, before any correction.
        let white = roll_white::sample_white(&film_sample);
        film_samples.push(film_sample);
        let area = area.map_err(|e| {
            NcError::Usage(format!(
                "{}: {} (every frame is measured over its effective area)",
                input.display(),
                e.message()
            ))
        })?;
        // An unsettled march can leave holder in the region, which the pool would take
        // as picture, and a capped one misread the IR — the same warnings `convert`
        // gives, so `--strict` sees them.
        for w in film_base::effective_area_warnings(&area) {
            push_warning_buf(&mut warnings, &log, format!("{}: {w}", input.display()));
        }
        let level = roll_white::frame_level(aces.rgb(), aces.width(), area.region)
            .map_err(|e| e.prefixed(input.display()))?;
        let tones = roll_white::frame_tones(aces.rgb(), aces.width(), area.region, base_stops)
            .map_err(|e| e.prefixed(input.display()))?;
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
            // Replaced by the corrected white once the roll's colour is measured.
            white_stops: None,
            decoded_white_stops: white.map(roll_white::scene_stops),
            leader_distance_stops,
            // A frame the pool kept nothing of (the leader guard emptied it, or no pixel was
            // usable) is not picture; a leader would pull the roll's exposure toward itself.
            // The guard is not applied to the level itself: it
            // drops the real highlights of a frame near saturation (on 07-24 that moved
            // the roll +0.5 EV).
            level_stops: level.filter(|_| counts.kept > 0),
            // Placed once every frame is measured, below.
            white_role: roll_white::FrameRole::Unmeasured,
            tones,
            flat: tones.is_some_and(|t| t.flat()),
            lift_ev: None,
            thin_lift: None,
            flag: None,
            memory,
        });
        decode.get_or_insert(decoded);
    }
    let gains = roll_white::roll_gains(&pool)?;
    // Freed before the midtone line's ACEScg copy of the samples is made.
    let pooled = pool.len() / 3;
    drop(pool);
    log.info(format_args!("roll white balance {gains:?}"));
    // Unguarded, as reviewed: only the gains take the leader guard. A frame the pool kept
    // nothing of is not picture, as for the exposure.
    let picture = film_samples
        .iter()
        .zip(&frames)
        .filter(|(_, f)| f.counts.kept > 0)
        .map(|(s, _)| s);
    let midtone = if args.midtone_neutral == MidtoneMode::Off {
        midtone_neutral::Measured::asked(picture.filter(|s| !s.is_empty()).count())
    } else {
        let picture: Vec<Vec<f32>> = picture.map(|s| roll_white::acescg_sample(s)).collect();
        midtone_neutral::measure(
            &picture.iter().map(Vec::as_slice).collect::<Vec<_>>(),
            gains,
            roll_white::pooled_white_stops(&picture, gains),
            args.midtone_neutral == MidtoneMode::Auto,
        )
    };
    match (midtone.line, midtone.off_because) {
        (Some(l), _) => log.info(format_args!("midtone line {l:?}")),
        (None, why) => log.info(format_args!("no midtone line: {why:?}")),
    }
    let confidence = MeasuredConfidence::new(
        correction_confidence::assess(frames.iter().filter(|f| f.counts.kept > 0).count()),
        midtone
            .line
            .map(|_| correction_confidence::assess(midtone.frames)),
    );
    if let Some(advice) = &confidence.advice {
        log.note(advice);
    }
    let colour = RollColour {
        gains,
        line: midtone.line,
    };
    // The second pass: each frame's white after the roll's colour correction.
    for (f, sample) in frames.iter_mut().zip(&film_samples) {
        f.white_stops = roll_white::corrected_white(sample, gains, colour.line.as_ref())
            .map(roll_white::scene_stops);
    }
    drop(film_samples);
    let levels: Vec<Option<f32>> = frames.iter().map(|f| f.level_stops).collect();
    let exposure = roll_white::roll_exposure(&levels)?;
    log.info(format_args!(
        "roll exposure {:+.2} EV (median frame level {:+.2} stops)",
        exposure.ev, exposure.level_stops
    ));
    if exposure.bounded {
        push_warning_buf(
            &mut warnings,
            &log,
            format!(
                "the roll's median frame level is {:+.2} stops from mid-grey, so its exposure \
                 is limited to {:+} EV (bound {} EV) and it renders off the normal level; \
                 check the inputs are this roll's picture frames under its film base, or, \
                 if the roll really is that far off, add --exposure when converting \
                 (`convert`, `roll`), which adds to `roll.exposure`",
                exposure.level_stops,
                exposure.ev,
                roll_white::EXPOSURE_BOUND_EV
            ),
        );
    }
    let lifts = Lifts {
        small: !args.no_small_lift,
        thin: !args.no_thin_lift,
    };
    for f in &mut frames {
        let flat = f.flat;
        f.lift_ev = f.white_stops.map(|w| {
            if flat {
                0.0
            } else {
                roll_white::small_lift(w, exposure.ev)
            }
        });
    }
    let mut white = measured_roll_white(&mut frames, args.leader.is_some(), colour, exposure.ev)?;
    // The thin lift starts from the frame's render without it: its own slope (a clamp's,
    // else the roll's) and the roll's exposure plus its small lift.
    let (mut thin_bounded, mut thin_unlifted) = (Vec::new(), Vec::new());
    for f in &mut frames {
        let (Some(w), Some(tones), Some(lift)) = (f.white_stops, f.tones, f.lift_ev) else {
            continue;
        };
        if !roll_white::thin(w, exposure.ev, &tones) {
            continue;
        }
        let k0 = roll_white::slope_for(own_white(f, &white).unwrap_or(white.stops));
        f.thin_lift = roll_white::thin_lift(w, base_stops, exposure.ev, k0, exposure.ev + lift);
        // Disclosed, not warned about: the lift is a taste adjustment, on by default.
        let name = f.input.display();
        match f.thin_lift {
            None => {
                log.info(format_args!(
                    "{name}: thin, but no thin lift is possible (slope {k0})"
                ));
                thin_unlifted.push(f.input.clone());
            }
            Some(t) if t.bounded => {
                log.info(format_args!(
                    "{name}: thin lift held by the slope bound {}: its white rises {:.2} stop",
                    roll_white::THIN_SLOPE_BOUND,
                    t.lift_stops
                ));
                thin_bounded.push(f.input.clone());
            }
            Some(_) => {}
        }
    }
    frame_flags(&mut frames, &mut white, colour, exposure.ev, lifts);
    let small_lift = MeasuredSmallLift {
        kind: "taste",
        written: lifts.small,
        lifted: frames
            .iter()
            .filter(|f| f.written(lifts).exposure.is_some())
            .count(),
        bound_ev: roll_white::LIFT_BOUND_EV,
        full_stops: roll_white::LIFT_FULL_STOPS,
        none_stops: roll_white::LIFT_NONE_STOPS,
        flat_spread_stops: roll_white::FLAT_SPREAD_STOPS,
    };
    let thin_lift = MeasuredThinLift {
        kind: "taste",
        written: lifts.thin,
        lifted: frames
            .iter()
            .filter(|f| f.written(lifts).thin.is_some())
            .count(),
        base_stops,
        white_stops: roll_white::THIN_WHITE_STOPS,
        base_share: roll_white::THIN_BASE_SHARE,
        near_base_stops: roll_white::NEAR_BASE_STOPS,
        lift_stops: roll_white::THIN_LIFT_STOPS,
        slope_bound: roll_white::THIN_SLOPE_BOUND,
        bounded: thin_bounded,
        unlifted: thin_unlifted,
    };
    log.info(format_args!(
        "roll white {:+.2} stops ({:?}), slope {}",
        white.stops, white.bound, white.slope
    ));
    // Everything a roll shares, as one recipe `roll --params` renders alone: the base,
    // the roll section with its clamps, the decode the gains hold under, and the input
    // and measure sections when stated.
    let measured = MeasuredRecipe {
        recipe_version: recipe::RecipeVersion,
        input: Some(recipe.input.clone()).filter(|i| *i != InputParams::default()),
        calibration: recipe::Calibration {
            film_base: Some(FilmBaseSource::Explicit(base.into())),
        },
        roll: Some(recipe::RollSection {
            white_balance: Some(gains),
            midtone_line: midtone.line,
            white_stops: Some(white.stops),
            exposure: Some(exposure.ev),
            frames: frame_table(&frames, &white, lifts),
            ..recipe::RollSection::default()
        }),
        measure: (recipe.measure != MeasureParams::default()).then(|| recipe.measure.clone()),
        reconstruction: Some(recipe.reconstruction),
    };

    let report = MeasureRollReport {
        command: "measure-roll",
        identity: Identity::new(),
        unexposed,
        film_base: base,
        decode: decode.expect("clap requires at least one input"),
        leader,
        frames,
        white_balance: RollWhiteBalance {
            gains,
            percentile: roll_white::PERCENTILE,
            pooled,
        },
        midtone_neutral: MeasuredMidtone {
            kind: "correction",
            mode: args.midtone_neutral,
            measured: midtone,
            min_frames: midtone_neutral::MIN_FRAMES,
            min_bands: midtone_neutral::MIN_BANDS,
            fade_stops: midtone_neutral::FADE_STOPS,
            gate_log2: midtone_neutral::GATE_LOG2,
        },
        confidence,
        reuse: RollReuse {
            flag: reuse_flag(colour, white.stops, exposure.ev, Written::default()),
        },
        white,
        exposure: MeasuredRollExposure {
            measured: exposure,
            target_stops: roll_white::LEVEL_TARGET_STOPS,
            bound_ev: roll_white::EXPOSURE_BOUND_EV,
        },
        small_lift,
        thin_lift,
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
            "--strict: {} warning(s) present (see report){}",
            report.warnings.len(),
            if args.out.out.is_some() {
                "; no recipe written"
            } else {
                ""
            }
        )));
    }
    if let Some(out) = args.out.out.as_deref() {
        write_recipe_out(out, &measured, &log)?;
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
/// (`--telemetry`) and/or a one-off file or stdout (`--telemetry-file`), and the
/// consented upload queue when `managed` holds a snapshot. The managed append is
/// silent but for a `-v` line: no warning the user did not ask for this run.
/// `telemetry_log` is the log path resolved once, so the guarded and the written
/// path are the same. Best-effort — every failure is warned on stderr (a closed
/// stdout pipe is the reader leaving, not a failure) and swallowed, and nothing here enters `report.warnings`, so neither `--strict` nor
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
    managed: Option<&telemetry::managed::Snapshot>,
) {
    let requested = telemetry_requested(args);
    // A telemetry write failure warns but never fails the run. Unlike ordinary
    // warnings, these are deliberately kept out of `report.warnings` (so
    // `--strict` can't promote them), which means the report can't carry them
    // either — so they must show even under `--quiet` (the `non_finite` precedent):
    // an opted-in feature failing silently would defeat the opt-in. The
    // successful-write notices stay `log.info` (visible only under `-v`).
    let warn = |msg: String| {
        if requested {
            log.warn_always(&msg)
        }
    };

    // A run that failed before the write-target guard has not proven its sinks safe:
    // write only if none lands on a file the run read or might have written. The
    // output is the resolved path once known, else `-o` as typed or completed with
    // any suffix.
    let output = attempt.output.as_deref().unwrap_or(&args.output);
    let mut explicit = requested;
    if requested && !attempt.guarded {
        let collision = telemetry_sink_collision(args, output, telemetry_log).or_else(|| {
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
            explicit = false;
        }
    }
    // The consented queue gets the event only where it cannot land on the run's files.
    let managed = managed.filter(|m| managed_queue_clear(args, attempt, output, m.queue()));
    if !explicit && managed.is_none() {
        return;
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

    let managed_wrote = managed.is_some_and(|m| {
        let wrote = m.append(&line).is_ok();
        if wrote {
            log.info(format_args!(
                "telemetry: queued for upload in {}",
                m.queue().display()
            ));
        }
        wrote
    });
    if !explicit {
        return;
    }

    if args.telemetry {
        match telemetry_log {
            // The consented queue already has this event.
            Some(path)
                if managed_wrote
                    && managed.is_some_and(|m| {
                        telemetry::consent::normalize_queue(path).is_ok_and(|p| p == m.queue())
                    }) =>
            {
                log.info(format_args!("telemetry: appended to {}", path.display()));
            }
            Some(path) => {
                if let Err(e) = telemetry::managed::append_explicit(path, &line) {
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
            // `-` = stdout. If the JSON report is also on stdout (the default),
            // stdout then carries the report plus this one line — pair
            // `--telemetry-file -` with `--report none`/`--report-file` when a
            // parser consumes stdout. A closed pipe is the reader leaving, not a
            // telemetry failure.
            match stdio::stdout_line(&line) {
                Ok(Delivery::Written) => {}
                Ok(Delivery::ReaderGone) => {
                    log.info("stdout's reader closed; the telemetry event was not read")
                }
                Err(e) => warn(format!("telemetry: could not write to stdout: {e}")),
            }
        } else if telemetry::managed::is_selected_queue(Path::new(target)) {
            // Overwriting it would erase every queued record.
            warn(format!(
                "telemetry: not written: --telemetry-file {target} is the upload queue"
            ));
        } else if let Err(e) = telemetry::write_oneoff(Path::new(target), &line) {
            warn(format!("telemetry: could not write {target}: {e}"));
        } else {
            log.info(format_args!("telemetry: wrote {target}"));
        }
    }
}

/// Whether the consented upload queue lands on none of the run's files: the input,
/// a `--params` recipe, an output, or the one-off `--telemetry-file`.
fn managed_queue_clear(
    args: &ConvertArgs,
    attempt: &ConvertAttempt,
    output: &Path,
    queue: &Path,
) -> bool {
    let key = collision_key(queue);
    let mut others: Vec<&Path> = write_targets(args, output, None)
        .into_iter()
        .map(|(_, p)| p)
        .collect();
    others.push(&args.input);
    others.extend(recipe_files(&args.recipe_in).map(PathBuf::as_path));
    let completed = attempt.output.is_none() && completes(queue, &args.output);
    !completed && !others.iter().any(|p| keys_collide(&key, &collision_key(p)))
}

/// Where a telemetry sink would land on a file it must not overwrite — the input,
/// the `--params` recipe the run reads, an output, or the other sink — as a message
/// naming both; `None` when every sink is clear. `--params` is guarded here rather
/// than in [`write_targets`], which would refuse `--dump-params X --params X` (allowed
/// when X is the sole layer; `convert_attempt` refuses it over one of several).
fn telemetry_sink_collision(
    args: &ConvertArgs,
    output: &Path,
    telemetry_log: Option<&Path>,
) -> Option<String> {
    let is_sink = |label: &str| matches!(label, "--telemetry-file" | "the telemetry log");
    let targets = write_targets(args, output, telemetry_log);
    let (sinks, mut others): (Vec<_>, Vec<_>) =
        targets.into_iter().partition(|(label, _)| is_sink(label));
    others.push(("the input scan", &args.input));
    others.extend(recipe_files(&args.recipe_in).map(|p| ("--params", p.as_path())));
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
        // panics, to `effective_area`, which no conversion resolves.
        film_base_source: recipe
            .calibration
            .film_base
            .as_ref()
            .map_or(FilmBaseProvenance::EffectiveArea, FilmBaseProvenance::from),
        output_depth: primary_depth(destination).into(),
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

    #[test]
    fn the_confidence_advice_names_only_what_is_in_doubt_with_its_own_count() {
        let warning = |gains: usize, line: Option<usize>| {
            MeasuredConfidence::new(
                correction_confidence::assess(gains),
                line.map(correction_confidence::assess),
            )
            .advice
        };
        assert_eq!(warning(16, Some(16)), None);
        assert_eq!(warning(16, None), None);
        let w = warning(16, Some(15)).unwrap();
        assert!(
            w.starts_with("the roll's midtone line was measured from 15 frames")
                && w.contains("`--midtone-neutral off`")
                && !w.contains("white balance")
                && !w.contains("16 frames"),
            "{w}"
        );
        let w = warning(10, Some(9)).unwrap();
        assert!(
            w.starts_with(
                "the roll's white balance was measured from 10 frames and its midtone line \
                 from 9"
            ) && w.contains("`--neutral-balance off`"),
            "{w}"
        );
        let w = warning(10, None).unwrap();
        assert!(
            w.starts_with("the roll's white balance was measured from 10 frames:")
                && !w.contains("line"),
            "{w}"
        );
    }

    /// `recipe::merge` with a parsed `convert`'s flags.
    fn merged(base: Recipe, args: &ConvertArgs) -> Recipe {
        recipe::merge(base, &args.knobs)
    }

    /// `reject_removed_flags` as a parsed `convert` runs it.
    fn removed(args: &ConvertArgs) -> Result<()> {
        reject_removed_flags(&args.knobs, !args.recipe_in.is_empty(), false)
    }

    /// A recipe whose film base is **stated**.
    ///
    /// `calibration.film_base` has no default, so `validate_shared` rejects an
    /// unstated one. Tests that are not *about* the film base use this, otherwise every
    /// unrelated assertion would trip that rule instead of the one under test.
    fn base_recipe() -> Recipe {
        let mut r = Recipe::default();
        r.calibration.film_base = Some(FilmBaseSource::Explicit([0.9, 0.55, 0.42]));
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
            let err = validate_shared(&cfg).unwrap_err();
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
        let err = format!("{}", validate_shared(&over).unwrap_err());
        assert!(
            err.contains("lower the fraction") && !err.contains("--auto-d-max"),
            "{err}"
        );
        let mut ok = base_recipe();
        ok.measure.inset = 0.0;
        assert!(
            validate_shared(&ok).is_ok(),
            "zero is legal — the stage floors it at one probe step on a measured \
             frame, which is not a usage question"
        );
        ok.measure.inset = crate::types::MAX_MEASURE_INSET;
        assert!(
            validate_shared(&ok).is_ok(),
            "the bound itself is inclusive"
        );
    }

    #[test]
    fn removed_algorithm_and_simple_flags_are_migration_errors() {
        // `--algorithm` is rejected with guidance naming the replacement.
        let err = removed(&parse_convert(&["--algorithm", "sigmoid"])).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("recipe `reconstruction`"), "{err}");
        // The remedy is to drop the flag, not to reach for the removed curve selector.
        assert!(err.to_string().contains("Drop the flag"), "{err}");
        assert!(!err.to_string().contains("--density-curve"), "{err}");

        // `--reconstruction`, whatever its value — density is the only one left.
        for value in ["simple", "density"] {
            let err = removed(&parse_convert(&["--reconstruction", value])).unwrap_err();
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
            let err = removed(&parse_convert(flags)).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{flags:?}");
            assert!(err.to_string().contains("was removed"), "{flags:?}: {err}");
        }

        // A clean invocation passes.
        assert!(removed(&parse_convert(&[])).is_ok());
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
            let err = removed(&parse_convert(flags)).unwrap_err();
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
            let err = removed(&parse_convert(flags)).unwrap_err().to_string();
            assert!(err.contains(want), "{flags:?}: {err}");
            assert!(err.contains("rop the flag"), "{flags:?}: {err}");
            for gone in ["--density-curve exponential", "Pass --", "--film-stock <"] {
                assert!(!err.contains(gone), "{flags:?} advises `{gone}`: {err}");
            }
        }
        // `characteristic` and `sigmoid` get their own history; the plain identity does not.
        let why = |v: &str| {
            removed(&parse_convert(&["--density-curve", v]))
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
            let err = removed(&parse_convert(&flags)).unwrap_err();
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
            let err = removed(&parse_convert(&argv)).unwrap_err();
            assert!(err.to_string().contains("was removed"), "{argv:?}: {err}");
        }
        let remedy = parse_convert(&["--anchor-mid-offset", "0.5"]);
        removed(&remedy).unwrap();
        let r = merged(base_recipe(), &remedy);
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
                Some("--exposure"),
            ),
            (
                vec!["--linear-range", "0.1,0.9"],
                "--linear-range was removed",
                Some("--display-black"),
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
            (
                vec!["--clip-low", "0.1"],
                "--clip-low was removed",
                Some("--display-black"),
            ),
            (
                vec!["--clip-high", "0.9"],
                "--clip-high was removed",
                Some("--exposure"),
            ),
            (
                vec!["--invert-white-balance", "1,1,1"],
                "--invert-white-balance was removed",
                Some("--white-balance"),
            ),
        ] {
            let err = removed(&parse_convert(&argv)).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{argv:?}");
            let msg = err.to_string();
            assert!(msg.contains(names), "{argv:?}: {msg}");
            if let Some(flag) = replacement {
                assert!(msg.contains(flag), "{argv:?}: {msg}");
                assert!(is_convert_flag(flag), "{flag} is not a flag");
            }
            // The removed chain's vocabulary is not advice any more, nor is a closed
            // task.
            assert!(!msg.contains("--new-flow"), "{argv:?}: {msg}");
            assert!(!msg.contains("levels-knob"), "{argv:?}: {msg}");
            assert!(
                !msg.contains("  "),
                "{argv:?}: a sentence is missing: {msg}"
            );
        }
        let msg = removed(&parse_convert(&["--output-preset", "legacy"]))
            .unwrap_err()
            .to_string();
        assert!(msg.contains("Drop the flag, or state the axes"), "{msg}");
        // `display-p3` names its gamut rather than "the default": under `--rendering
        // direct` the default is the HDR float TIFF.
        let msg = removed(&parse_convert(&["--output-preset", "display-p3"]))
            .unwrap_err()
            .to_string();
        assert!(msg.contains("pass --range sdr --gamut display-p3"), "{msg}");
        assert!(removed(&parse_convert(&[])).is_ok());
    }

    #[test]
    fn the_preset_counterparts_resolve_the_same_under_every_rendering() {
        // The refusal runs before the recipe says which rendering applies, so each named
        // set must write one destination under either — and be written today.
        use crate::destination::{Defaults, resolve};
        for &(name, flags) in PRESET_COUNTERPARTS {
            let argv: Vec<&str> = flags.split_whitespace().collect();
            let r = merged(Recipe::default(), &parse_convert(&argv));
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
            removed(&parse_convert(
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
            let err = reject_deprecated_input_flags(&args.knobs.input_opts)
                .and_then(|()| removed(&args))
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

    /// `roll` takes every flag `convert` does — knobs and removed ones alike — but the
    /// single-output plumbing, so a knob added to `ConvertArgs` directly is caught.
    #[test]
    fn roll_takes_every_conversion_flag_convert_does() {
        use clap::CommandFactory;
        const CONVERT_ONLY: &[&str] = &[
            "--output",
            "--dump-params",
            "--seed",
            "--export-pre-encode",
            "--telemetry",
            "--telemetry-file",
        ];
        let cli = Cli::command();
        let longs = |name: &str| -> std::collections::BTreeSet<String> {
            cli.find_subcommand(name)
                .unwrap()
                .get_arguments()
                .filter_map(|a| a.get_long().map(|l| format!("--{l}")))
                .collect()
        };
        let roll = longs("roll");
        let convert = longs("convert");
        let missing: Vec<_> = convert
            .iter()
            .filter(|f| !CONVERT_ONLY.contains(&f.as_str()) && !roll.contains(*f))
            .collect();
        assert!(missing.is_empty(), "`roll` lacks {missing:?}");
        assert!(convert.len() > 60, "only {} flags found", convert.len());
    }

    #[test]
    fn new_flow_is_a_removed_flag_on_every_command_that_took_it() {
        for argv in [
            vec!["hanten", "convert", "in.tif", "-o", "out", "--new-flow"],
            vec!["hanten", "roll", "in.tif", "-o", "dir", "--new-flow"],
            vec!["hanten", "params", "--new-flow"],
        ] {
            let new_flow = match Cli::try_parse_from(&argv).unwrap().command {
                Command::Convert(a) => a.knobs.new_flow,
                Command::Roll(a) => a.knobs.new_flow,
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
        let err = removed(&parse_convert(&["--new-flow", "--print-exposure", "1"])).unwrap_err();
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
            let msg = removed(&parse_convert(&["--display-tone", value]))
                .unwrap_err()
                .to_string();
            assert!(msg.contains("--display-tone was removed"), "{value}: {msg}");
            assert!(msg.contains(remedy), "{value}: {msg}");
            assert!(msg.contains("fit_range.headroom_stops"), "{value}: {msg}");
        }
        let msg = removed(&parse_convert(&["--highlight-compress", "0"]))
            .unwrap_err()
            .to_string();
        assert!(msg.contains("--highlight-compress was removed"), "{msg}");
        assert!(msg.contains("--display-tone-headroom"), "{msg}");
        // Falsifiable: the headroom flag itself is not a removed flag.
        removed(&parse_convert(&["--display-tone-headroom", "4"])).unwrap();
    }

    /// The output target a `convert` command line's destination flags resolve.
    fn target_of(extra: &[&str]) -> OutputTarget {
        let args = parse_convert(extra);
        let r = merged(base_recipe(), &args);
        OutputTarget::resolve(
            &r,
            KnobNames::FlagAndKey,
            args.knobs.destination.film_master,
        )
        .unwrap()
    }

    /// The gain-map JPEG destination (`--range hdr`).
    fn jpeg() -> OutputTarget {
        target_of(&["--range", "hdr"])
    }

    #[test]
    fn a_missing_extension_is_completed_from_the_resolved_container() {
        // An extensionless path is completed from whatever container the destination
        // resolved — the default's TIFF, the gain map's JPEG.
        for (extra, want) in [
            (&[][..], "positive.tiff"),
            (&["--film-master"][..], "positive.tiff"),
            (&["--range", "hdr"][..], "positive.jpg"),
            (&["--transfer", "pq"][..], "positive.tiff"),
        ] {
            let mut args = parse_convert(extra);
            args.output = PathBuf::from("positive");
            validate_convert(&merged(base_recipe(), &args), &args).unwrap();
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
        let err = validate_convert(&merged(base_recipe(), &args), &args)
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
        let err = validate_convert(&merged(base_recipe(), &hdr), &hdr)
            .unwrap_err()
            .to_string();
        assert!(err.contains(".jpg or .jpeg"), "{err}");
        // Control: the matching suffix is accepted.
        hdr.output = PathBuf::from("positive.jpg");
        validate_convert(&merged(base_recipe(), &hdr), &hdr).unwrap();
    }

    #[test]
    fn a_removed_containers_suffix_is_refused_not_completed() {
        // `out.avif` is neither a stem (it would write `out.avif.tiff`) nor a mismatch
        // offering a destination: AVIF is gone, so the remedy is the TIFF.
        for given in ["positive.avif", "positive.AVIF"] {
            let mut args = parse_convert(&["--transfer", "pq"]);
            args.output = PathBuf::from(given);
            let err = validate_convert(&merged(base_recipe(), &args), &args)
                .unwrap_err()
                .to_string();
            assert!(err.contains("no longer writes AVIF"), "{err}");
            assert!(err.contains("Name it `.tiff`"), "{err}");
            assert!(!err.contains("drop .avif"), "{err}");
        }
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
        // The subject is `Recipe::default()`, the `params` of what `hanten params` prints
        // (`hanten_params_writes_the_recipe_convert_reads` covers the envelope), so this
        // must stay on the real default, not on a film-base-stated stand-in.
        let json = serde_json::to_string_pretty(&Recipe::default()).unwrap();
        let back: Recipe = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Recipe::default());

        // ...and the scaffold is deliberately NOT runnable as printed: it states no
        // film base, so `validate_shared` rejects it — `hanten params` emits a template to
        // edit, not a recipe to run.
        let msg = match validate_shared(&back) {
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
        runnable.calibration.film_base = Some(FilmBaseSource::Explicit([0.9, 0.55, 0.42]));
        validate_shared(&runnable).unwrap();
    }

    #[test]
    fn validate_requires_a_stated_film_base() {
        // `calibration.film_base` has no default: `Dmin` is the divisor of the density
        // conversion, so it must be stated. Both stated forms are fine.
        let unstated = Recipe::default();
        assert_eq!(
            unstated.calibration.film_base, None,
            "there must be no default"
        );
        let msg = match validate_shared(&unstated) {
            Err(NcError::Usage(m)) => m,
            other => panic!("an unstated film base must be a usage error, got {other:?}"),
        };
        // The message has to be actionable: name every way out, and how to measure
        // the base it asks for.
        for expected in [
            "--film-base",
            "--base-region",
            "hanten measure-base <unexposed-frame>",
            "calibration.film_base",
        ] {
            assert!(msg.contains(expected), "{expected} missing from: {msg}");
        }

        assert!(
            !msg.contains("--auto-base"),
            "the retired flag is no remedy: {msg}"
        );

        for stated in [
            FilmBaseSource::Region([0, 0, 100, 40]),
            FilmBaseSource::Explicit([0.9, 0.55, 0.42]),
        ] {
            let mut cfg = Recipe::default();
            cfg.calibration.film_base = Some(stated.clone());
            validate_shared(&cfg)
                .unwrap_or_else(|e| panic!("a stated {stated:?} base must be accepted: {e}"));
        }
    }

    #[test]
    fn the_missing_base_message_names_remedies_both_commands_take() {
        // `convert` and `roll` take the same film-base flags, so one message serves
        // both; the measuring command it names accepts what it is shown with.
        let msg = match validate_shared(&Recipe::default()) {
            Err(NcError::Usage(m)) => m,
            other => panic!("an unstated film base must be a usage error, got {other:?}"),
        };
        assert_eq!(msg, MISSING_FILM_BASE);
        for remedy in [
            "--film-base",
            "--base-region",
            "calibration.film_base",
            "hanten measure-base <unexposed-frame>",
            "hanten measure-roll <frames> --unexposed",
            "--params",
        ] {
            assert!(msg.contains(remedy), "{remedy} is not offered: {msg}");
        }
        assert!(!msg.contains("--auto-base"), "{msg}");
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
        let msg = validate_shared(&contradictory).unwrap_err().to_string();
        assert!(
            msg.contains("measure-inset"),
            "the bad value must be diagnosed ahead of the missing base: {msg}"
        );

        // Falsifiable control: with the bad value removed, the same recipe does report
        // the missing base — so the assertion above is about ordering, not about the
        // missing-base rule having stopped working.
        let mut only_unstated = Recipe::default();
        assert!(
            validate_shared(&only_unstated)
                .unwrap_err()
                .to_string()
                .contains("no film base selected")
        );
        only_unstated.calibration.film_base = Some(FilmBaseSource::Explicit([0.9, 0.55, 0.42]));
        validate_shared(&only_unstated).unwrap();
    }

    #[test]
    fn validate_rejects_recipe_smuggled_bad_values() {
        // A recipe can carry values the CLI value-parsers would have rejected,
        // so validate is the only guard for these once they're in the config.
        let mut cfg = base_recipe();
        cfg.calibration.film_base = Some(FilmBaseSource::Explicit([0.9, 0.0, 0.4])); // zero transmission
        assert!(matches!(validate_shared(&cfg), Err(NcError::Usage(_))));

        let mut cfg = base_recipe();
        cfg.calibration.film_base = Some(FilmBaseSource::Explicit([0.9, 90.0, 0.4])); // "90" typo for "0.90"
        assert!(matches!(validate_shared(&cfg), Err(NcError::Usage(_))));
        let mut cfg = base_recipe();
        cfg.calibration.film_base = Some(FilmBaseSource::Explicit([1.0, 1.0, 1.0])); // 1.0 exactly is valid
        validate_shared(&cfg).unwrap();

        let mut cfg = base_recipe();
        cfg.calibration.film_base = Some(FilmBaseSource::Region([0, 0, 0, 0])); // zero-area region
        assert!(matches!(validate_shared(&cfg), Err(NcError::Usage(_))));
    }

    #[test]
    fn export_ir_is_refused_as_removed() {
        let err = removed(&parse_convert(&["--export-ir", "ir.tiff"])).unwrap_err();
        assert!(matches!(err, NcError::Usage(_)), "{err}");
        assert!(err.message().contains("--export-ir was removed"), "{err}");
    }

    #[test]
    fn seed_parses() {
        // The reserved `--seed` flag parses rather than being rejected by clap.
        let args = parse_convert(&["--seed", "42"]);
        assert_eq!(args.seed, Some(42));
    }

    #[test]
    fn merge_keeps_recipe_source_until_a_flag_replaces_it() {
        // No flag → the recipe's mutually-exclusive choice survives.
        let mut recipe = base_recipe();
        recipe.calibration.film_base = Some(FilmBaseSource::Explicit([0.9, 0.5, 0.4]));
        let cfg = merged(recipe.clone(), &parse_convert(&[]));
        assert_eq!(
            cfg.calibration.film_base,
            Some(FilmBaseSource::Explicit([0.9, 0.5, 0.4]))
        );

        // A flag replaces the whole source — no field is left behind to win on
        // precedence (the #5/#6 fix). `--base-region` beats a recipe explicit base.
        let cfg = merged(recipe, &parse_convert(&["--base-region", "0,0,100,40"]));
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
        let cfg = merged(recipe.clone(), &parse_convert(&[]));
        assert_eq!(cfg.input.transfer, TransferAssertion::Auto);
        assert_eq!(cfg.input.meaning, MeaningAssertion::ScannerDevice);

        // `--input-transfer` replaces only the transfer axis.
        let cfg = merged(
            recipe.clone(),
            &parse_convert(&["--input-transfer", "linear"]),
        );
        assert_eq!(cfg.input.transfer, TransferAssertion::Linear);
        assert_eq!(cfg.input.meaning, MeaningAssertion::ScannerDevice);

        // `--input-meaning` replaces only the meaning axis (over a recipe value).
        let cfg = merged(recipe, &parse_convert(&["--input-meaning", "colorimetric"]));
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
        let cfg = merged(recipe.clone(), &parse_convert(&[]));
        assert_eq!(cfg.input.film_type, FilmType::Silver);

        // The flag wins over the recipe.
        let cfg = merged(recipe, &parse_convert(&["--film-type", "chromogenic"]));
        assert_eq!(cfg.input.film_type, FilmType::Chromogenic);

        // Over the default recipe, the flag sets the declared type.
        let cfg = merged(
            base_recipe(),
            &parse_convert(&["--film-type", "chromogenic"]),
        );
        assert_eq!(cfg.input.film_type, FilmType::Chromogenic);
        // ...and the untouched default is `unknown` (the safe off state).
        let cfg = merged(base_recipe(), &parse_convert(&[]));
        assert_eq!(cfg.input.film_type, FilmType::Unknown);
    }

    #[test]
    fn deprecated_assume_linear_is_a_migration_error() {
        // The old combined assertion must never silently assert both axes — it is a
        // loud usage error (exit 2) pointing at the two independent flags.
        let args = parse_convert(&["--assume-linear"]);
        let err = reject_deprecated_input_flags(&args.knobs.input_opts).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("--input-transfer"));
    }

    #[test]
    fn input_profile_stays_rejected_for_convert() {
        // `--input-profile` is reserved (deferred experiment) — rejected loudly
        // (exit 4) rather than silently ignored.
        let args = parse_convert(&["--input-profile", "scanner.icc"]);
        let err = reject_deprecated_input_flags(&args.knobs.input_opts).unwrap_err();
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
                "--base-region",
                "0,0,1,1",
                "--film-base",
                "0.9,0.5,0.4"
            ])
            .is_err()
        );
    }

    /// **Only what was measured is written**, so the file pins nothing a later layer
    /// states — and what is written loads as a recipe with no hand editing.
    #[test]
    fn a_measured_recipe_states_only_its_sections_and_loads() {
        let base = MeasuredRecipe {
            recipe_version: recipe::RecipeVersion,
            input: None,
            calibration: recipe::Calibration {
                film_base: Some(FilmBaseSource::Explicit([0.553, 0.271, 0.159])),
            },
            roll: None,
            measure: None,
            reconstruction: None,
        };
        let json = serde_json::to_string(&base).unwrap();
        assert_eq!(
            json,
            r#"{"recipe_version":3,"calibration":{"film_base":{"explicit":[0.553,0.271,0.159]}}}"#
        );
        let recipe: Recipe = serde_json::from_str(&json).unwrap();
        validate_shared(&recipe).unwrap();

        let roll = MeasuredRecipe {
            roll: Some(recipe::RollSection {
                white_balance: Some([0.5, 1.0, 1.25]),
                white_stops: Some(1.75),
                exposure: Some(1.2),
                frames: [
                    (
                        "f2.tif".to_owned(),
                        recipe::FrameRoll {
                            white_stops: Some(2.0),
                            exposure: None,
                            thin_slope: None,
                            thin_exposure: None,
                        },
                    ),
                    (
                        "f3.tif".to_owned(),
                        recipe::FrameRoll {
                            white_stops: None,
                            exposure: Some(0.2),
                            thin_slope: Some(2.1),
                            thin_exposure: Some(0.6),
                        },
                    ),
                ]
                .into(),
                ..recipe::RollSection::default()
            }),
            ..base
        };
        let recipe: Recipe = serde_json::from_str(&serde_json::to_string(&roll).unwrap()).unwrap();
        assert_eq!(Some(recipe.roll), roll.roll);
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
        // The wire contract: the flag serializes as the flat top-level key
        // `film_base_flag`; the recipe value is `--out`'s, never the report's; and the
        // `ReuseReady` wrapper / `reuse` field name never leaks. `None` emits
        // neither. Locks the `#[serde(flatten)]` + rename shape so a refactor
        // can't silently change the agent-facing JSON.
        let with = Report {
            reuse: Some(ReuseReady {
                flag: "--film-base 0.5,0.25,0.125".to_string(),
                source: FilmBaseSource::Explicit([0.5, 0.25, 0.125]),
            }),
            ..Report::default()
        };
        let v = serde_json::to_value(&with).unwrap();
        assert_eq!(v["film_base_flag"], "--film-base 0.5,0.25,0.125");
        // The old keys are gone, not renamed in place: a consumer still reading one
        // must fail loudly rather than silently see nothing.
        assert!(v.get("calibration").is_none());
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
        let loaded = load_recipes(&[]).unwrap();
        assert_eq!(loaded.recipe, Recipe::default());

        // Missing file → Usage (exit 2), not Other.
        let missing = std::env::temp_dir().join("nc-no-such-recipe-xyz.json");
        assert!(matches!(load_recipes(&[missing]), Err(NcError::Usage(_))));

        // Malformed JSON and unknown keys both map to Usage.
        for (tag, body) in [
            ("malformed", "{ not json"),
            (
                "unknown-key",
                r#"{"recipe_version":3,"reconstruction":{"scal":[1,1,1]}}"#,
            ),
        ] {
            let got = load_recipe_body(tag, body);
            assert!(
                matches!(got, Err(NcError::Usage(_))),
                "{tag} should be Usage"
            );
        }

        // A valid partial recipe loads and fills defaults.
        let got = load_recipe_body("ok", r#"{"recipe_version":3,"look":{"contrast":1.3}}"#)
            .unwrap()
            .recipe;
        assert_eq!(got.look.contrast, 1.3);
        assert_eq!(got.reconstruction, Recipe::default().reconstruction);
    }

    /// One recipe document as a lone `--params` loads it.
    #[derive(Debug)]
    struct Loaded {
        recipe: Recipe,
        meta_pipeline_version: Option<u32>,
    }

    /// Load `body` as a lone `--params` layer named `tag`.
    fn load_recipe_body(tag: &str, body: &str) -> Result<Loaded> {
        let layer = read_layer(body, tag)?;
        Ok(Loaded {
            recipe: recipe::compose([&layer.body]).map_err(|e| NcError::Usage(e.to_string()))?,
            meta_pipeline_version: layer.meta_pipeline_version,
        })
    }

    #[test]
    fn load_recipe_accepts_the_envelope_and_the_bare_shape() {
        // Identity lives in a `meta` envelope beside the recipe, so both an enveloped
        // and a bare document load — and to the *same* recipe.
        let bare = r#"{"recipe_version":3,"look":{"contrast":1.3}}"#;
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
        assert_eq!(wrapped.recipe.look.contrast, 1.3);
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
        assert_eq!(pipeline_version_warning(None, "r.json"), None);
        // The current version ⇒ no warning.
        assert_eq!(
            pipeline_version_warning(Some(version::PIPELINE_VERSION), "r.json"),
            None
        );
        // Any other version ⇒ a warning naming both numbers, so the operator can
        // see which direction the skew runs.
        let other = version::PIPELINE_VERSION.wrapping_add(1);
        let msg = pipeline_version_warning(Some(other), "r.json").expect("mismatch must warn");
        assert!(msg.contains("recipe r.json"), "names the layer: {msg}");
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
                "`{reserved}` is reserved for the recipe envelope but is now a recipe key: \
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
        // An OMITTED `meta` stays legal — a hand-wrapped bare recipe has no provenance
        // to record, and that is not a malformed envelope.
        assert_eq!(
            load_recipe_body("no-meta", r#"{"params":{"recipe_version":3}}"#)
                .unwrap()
                .meta_pipeline_version,
            None
        );
        // An empty `meta` object is legal too, and records nothing.
        assert_eq!(
            load_recipe_body("empty-meta", r#"{"meta":{},"params":{"recipe_version":3}}"#)
                .unwrap()
                .meta_pipeline_version,
            None
        );
        // Unknown fields inside a well-formed `meta` stay lenient — that leniency is
        // the forward-compatibility contract, not an oversight.
        assert_eq!(
            load_recipe_body(
                "future-meta",
                r#"{"meta":{"invented":[1],"pipeline_version":7},"params":{"recipe_version":3}}"#
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
            ("obj-bare", r#"{"recipe_version": 3}"#),
            ("obj-envelope", r#"{"params": {"recipe_version": 3}}"#),
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

    /// `roll` over a `--frames` manifest at `manifest`, writing into `dir`.
    fn roll_args(manifest: &Path, dir: &Path) -> RollArgs {
        RollArgs {
            inputs: vec![],
            frames: Some(manifest.to_path_buf()),
            out_dir: dir.to_path_buf(),
            recipe_in: vec![],
            knobs: ConversionFlags::default(),
            export_film_rgb: None,
            strict: false,
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
            resolve_frames(&args, &base_recipe(), &base_recipe(), &mut Vec::new(), &log)
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
        let planned = resolve_frames(&args, &shared, &shared, &mut warnings, &log);
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
        // A frame's own lift, and whether it applies: a preview may turn one frame's off.
        "roll.frame_exposure",
        "roll.small_lift",
        "roll.thin_slope",
        "roll.thin_exposure",
        "roll.thin_lift",
        // Whether the roll's white balance and midtone line apply: a frame may turn them off.
        "roll.midtone_neutral",
        "roll.neutral_balance",
        // Resolved per frame; a manifest stating it is refused, not warned about.
        "roll.frames",
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
        let changes: [Change; 10] = [
            ("calibration.film_base", |r| {
                r.calibration.film_base = Some(FilmBaseSource::Explicit([0.8, 0.5, 0.4]))
            }),
            ("roll.white_balance", |r| {
                r.roll.white_balance = Some([1.1, 1.0, 0.9])
            }),
            ("roll.midtone_line", |r| {
                r.roll.midtone_line = Some(MidtoneLine {
                    red: [-0.06, 0.04],
                    blue: [-0.02, 0.37],
                    bands: [-2.75, 1.75],
                    fade_end_stops: 1.87,
                })
            }),
            ("roll.exposure", |r| r.roll.exposure = Some(0.5)),
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
    fn gains_and_a_line_that_do_not_apply_on_the_frame_do_not_warn() {
        let line = MidtoneLine {
            red: [-0.06, 0.04],
            blue: [-0.02, 0.37],
            bands: [-2.75, 1.75],
            fade_end_stops: 1.87,
        };
        let mut shared = base_recipe();
        shared.calibration.film_base = Some(FilmBaseSource::Explicit([0.9, 0.55, 0.42]));
        shared.roll.white_balance = Some([1.05, 1.0, 0.95]);
        shared.roll.midtone_line = Some(line);
        let mut frame = shared.clone();
        frame.roll.white_balance = Some([1.1, 1.0, 0.9]);
        frame.roll.midtone_line = Some(MidtoneLine {
            fade_end_stops: 1.5,
            ..line
        });
        let breaks = |f: &Recipe, s: &Recipe| roll_wide_breaks(Path::new("a.tif"), f, s).unwrap();
        assert_eq!(breaks(&frame, &shared).len(), 2, "not vacuous");
        // The frame's own off, and the roll's off the frame inherits.
        let mut own_off = frame.clone();
        own_off.roll.neutral_balance = Some(recipe::Switch::Off);
        assert!(breaks(&own_off, &shared).is_empty());
        let mut roll_off = shared.clone();
        roll_off.roll.neutral_balance = Some(recipe::Switch::Off);
        let mut inherits = frame.clone();
        inherits.roll.neutral_balance = Some(recipe::Switch::Off);
        assert!(breaks(&inherits, &roll_off).is_empty());
        // A line switched off on the frame: only the gains differ.
        let mut line_off = frame;
        line_off.roll.midtone_neutral = Some(recipe::Switch::Off);
        let w = breaks(&line_off, &shared);
        assert!(
            w.len() == 1 && w[0].contains("`roll.white_balance`"),
            "{w:?}"
        );
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
        shared.look.contrast = 1.3;
        let log = Log::new(&args.report);
        let planned = resolve_frames(&args, &shared, &shared, &mut Vec::new(), &log);
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
        // and the way out rather than just the rule — by recipe key, which the frame's
        // manifest entry can state.
        let err = resolve_frame_output(
            Some(Path::new("frame.tiff")),
            Path::new("/s/f.tif"),
            Path::new("/out"),
            OutputTarget::resolve(
                &merged(base_recipe(), &parse_convert(&["--range", "hdr"])),
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
    fn film_rgb_export_name_sits_beside_the_frames_output() {
        let input = Path::new("/scans/frame 01.tif");
        for (output, want) in [
            ("/out/frame 01_positive.tiff", "/out/frame 01_film-rgb.tiff"),
            ("out/sub/a.tiff", "out/sub/frame 01_film-rgb.tiff"),
            ("/elsewhere/a.jpg", "/elsewhere/frame 01_film-rgb.tiff"),
            ("a.tiff", "frame 01_film-rgb.tiff"),
        ] {
            assert_eq!(
                film_rgb_export_name(input, Path::new(output)),
                PathBuf::from(want),
                "{output}"
            );
        }
    }

    #[test]
    fn film_rgb_clash_remedy_follows_who_owns_the_pair() {
        use RollTarget::{FilmRgb, Other, Output};
        let rename = Some("rename this frame's output");
        assert_eq!(film_rgb_clash_remedy(Output(0), FilmRgb(0)), rename);
        assert_eq!(film_rgb_clash_remedy(FilmRgb(1), Output(1)), rename);
        for (a, b) in [(Output(1), FilmRgb(0)), (FilmRgb(0), FilmRgb(1))] {
            assert!(
                film_rgb_clash_remedy(a, b)
                    .unwrap()
                    .contains("different folders")
            );
        }
        for (a, b) in [
            (Output(0), Output(1)),
            (Other, FilmRgb(0)),
            (FilmRgb(0), Other),
        ] {
            assert_eq!(film_rgb_clash_remedy(a, b), None, "{a:?} {b:?}");
        }
    }

    #[test]
    fn ensure_roll_targets_distinct_catches_input_and_sibling_collisions() {
        // A target aimed at an input scan, and two frames colliding on one output
        // (e.g. same stem from different dirs), both fail loudly.
        let inputs = [
            (Path::new("/scans/a.tif"), "an input scan"),
            (Path::new("/scans/b.tif"), "an input scan"),
        ];
        let clobber_input = vec![(
            "output for a".to_string(),
            PathBuf::from("/scans/a.tif"),
            RollTarget::Other,
        )];
        assert!(matches!(
            ensure_roll_targets_distinct(&inputs, &clobber_input),
            Err(NcError::Usage(_))
        ));
        let sibling_collision = vec![
            (
                "output for a".to_string(),
                PathBuf::from("/out/img_positive.tiff"),
                RollTarget::Other,
            ),
            (
                "output for b".to_string(),
                PathBuf::from("/out/img_positive.tiff"),
                RollTarget::Other,
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
                RollTarget::Other,
            ),
            (
                "output for b".to_string(),
                PathBuf::from("/out/b_positive.tiff"),
                RollTarget::Other,
            ),
        ];
        assert!(ensure_roll_targets_distinct(&inputs, &ok).is_ok());
    }

    #[test]
    fn first_null_key_names_the_nested_member() {
        use serde_json::json;
        assert_eq!(
            first_null_key(
                &json!({"look": {"contrast": 1.2, "highlight_desaturation": {"band": null}}})
            ),
            Some("look.highlight_desaturation.band".into())
        );
        assert_eq!(first_null_key(&json!({"roll": null})), Some("roll".into()));
        assert_eq!(
            first_null_key(&json!({"roll": {"white_balance": [1, null, 1]}})),
            Some("roll.white_balance[1]".into())
        );
        assert_eq!(
            without_nulls(
                &json!({"roll": {"white_balance": [1, null, 1], "white_stops": 2},
                                  "look": {"contrst": null}})
            ),
            json!({"roll": {"white_stops": 2}, "look": {}})
        );
        assert_eq!(
            first_null_key(&json!({"look": {"channel_grade": [1.1, 0.9]}})),
            None
        );
    }

    #[test]
    fn ensure_roll_targets_distinct_protects_the_frames_manifest() {
        // `run_roll` adds the `--frames` manifest to the protected read set, so a
        // write target aimed at it (e.g. `--report-file` equal to the manifest
        // path) is rejected up front rather than clobbering the manifest.
        let manifest = Path::new("/rolls/frames.json");
        let inputs = [
            (Path::new("/scans/a.tif"), "an input scan"),
            (manifest, "the --frames manifest"),
        ];
        let clobber_manifest = vec![(
            "--report-file".to_string(),
            PathBuf::from("/rolls/frames.json"),
            RollTarget::Other,
        )];
        assert!(matches!(
            ensure_roll_targets_distinct(&inputs, &clobber_manifest),
            Err(NcError::Usage(m)) if m.contains("would overwrite the --frames manifest")
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
                        max: [1.0; 3],
                    }),
                    film_type: Some(FilmType::Silver),
                    identity: Some(Box::new(Identity::new())),
                    chain: None,
                    hdr_linear_tiff: None,
                    hdr_coded_tiff: None,
                    film_rgb_exported: None,
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
            input_from_cli: InputFromCli::none(),
            film_rgb: None,
        };
        let warnings = vec!["a warning raised before the failure".to_string()];
        let mem = memory::preflight(
            &crate::io::decode::ImageShape::new(1000, 1000, 3, 16, true).unwrap(),
            RunProfile::DecodeOnly,
            SamplePlan::rect(500 * 500),
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
            SamplePlan::rect(500 * 500),
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
            SamplePlan::rect(500 * 500),
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

    /// Decode an f32 RGB TIFF: its samples and whether it embeds an ICC profile.
    fn read_f32_tiff(path: &Path) -> (u32, u32, Vec<f32>, bool) {
        use tiff::decoder::{Decoder, DecodingResult};
        let mut decoder = Decoder::new(std::fs::File::open(path).unwrap()).unwrap();
        let (w, h) = decoder.dimensions().unwrap();
        let tagged = decoder.get_tag_u8_vec(tiff::tags::Tag::IccProfile).is_ok();
        match decoder.read_image().unwrap() {
            DecodingResult::F32(data) => (w, h, data, tagged),
            other => panic!("expected f32 samples, got {other:?}"),
        }
    }

    #[test]
    fn the_film_rgb_export_is_the_film_master_before_the_pinned_matrix() {
        /// Removes the directory even when an assertion fails.
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.0).ok();
            }
        }
        let dir = std::env::temp_dir().join(format!("nc-film-rgb-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _cleanup = Cleanup(dir.clone());
        let input = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hdr-48bit.tif");
        let convert = |output: &Path, export: &Path, destination: &[&str]| {
            let mut argv = vec![
                "hanten",
                "convert",
                input.to_str().unwrap(),
                "-o",
                output.to_str().unwrap(),
                "--film-base",
                "0.9,0.55,0.42",
                "--export-film-rgb",
                export.to_str().unwrap(),
                "--quiet",
                "--report-file",
            ];
            let report = output.with_extension("report.json");
            argv.push(report.to_str().unwrap());
            argv.extend_from_slice(destination);
            let Command::Convert(args) = Cli::try_parse_from(argv).unwrap().command else {
                unreachable!("expected convert")
            };
            run_convert(args).unwrap();
            let report: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
            assert_eq!(report["film_rgb_exported"], export.to_str().unwrap());
        };

        let (master, film) = (dir.join("master.tiff"), dir.join("film.tiff"));
        convert(&master, &film, &["--film-master"]);
        let (w, h, film_rgb, film_tagged) = read_f32_tiff(&film);
        let (_, _, master_rgb, master_tagged) = read_f32_tiff(&master);
        assert!(!film_tagged, "film RGB has no primaries, so no profile");
        assert!(master_tagged);

        // Through the shipped mapper, so the matrix is the pinned one, not a copy.
        let fixture =
            FilmRgbImage::fixture(LinearImage::new(w, h, film_rgb.clone(), None).unwrap());
        let mapped = working_space::map_nc_film_rgb_v1(fixture).into_linear().rgb;
        assert_eq!(mapped.len(), master_rgb.len());
        let differ = mapped
            .iter()
            .zip(&master_rgb)
            .filter(|(a, b)| a.to_bits() != b.to_bits())
            .count();
        assert_eq!(differ, 0, "the export mapped must be the master to the bit");
        // …and the export is not the master renamed.
        assert_ne!(film_rgb, master_rgb);

        // A rendered destination exports the same film RGB: the export sits before
        // every rendering stage.
        let (sdr, film_sdr) = (dir.join("sdr.tiff"), dir.join("film-sdr.tiff"));
        convert(&sdr, &film_sdr, &[]);
        assert_eq!(read_f32_tiff(&film_sdr).2, film_rgb);
    }

    /// One `--export-pre-encode` page: its description and samples.
    #[derive(Debug)]
    struct Page {
        buffer: String,
        space: String,
        width: u32,
        height: u32,
        samples: tiff::decoder::DecodingResult,
    }

    /// Every page of a pre-encode export, in order, with no ICC profile on any.
    fn read_pre_encode(path: &Path) -> Vec<Page> {
        use tiff::decoder::Decoder;
        let mut decoder = Decoder::new(std::fs::File::open(path).unwrap()).unwrap();
        let mut pages = Vec::new();
        loop {
            assert!(decoder.get_tag_u8_vec(tiff::tags::Tag::IccProfile).is_err());
            let description: serde_json::Value = serde_json::from_str(
                &decoder
                    .get_tag_ascii_string(tiff::tags::Tag::ImageDescription)
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(description["nc_pre_encode"], 1);
            let (width, height) = decoder.dimensions().unwrap();
            pages.push(Page {
                buffer: description["buffer"].as_str().unwrap().into(),
                space: description["space"].as_str().unwrap().into(),
                width,
                height,
                samples: decoder.read_image().unwrap(),
            });
            if !decoder.more_images() {
                return pages;
            }
            decoder.next_image().unwrap();
        }
    }

    /// The u16 samples of a 16-bit RGB TIFF.
    fn read_u16_tiff(path: &Path) -> Vec<u16> {
        use tiff::decoder::{Decoder, DecodingResult};
        let mut decoder = Decoder::new(std::fs::File::open(path).unwrap()).unwrap();
        match decoder.read_image().unwrap() {
            DecodingResult::U16(data) => data,
            other => panic!("expected u16 samples, got {other:?}"),
        }
    }

    #[test]
    fn the_pre_encode_export_is_what_each_encoder_receives() {
        use tiff::decoder::DecodingResult;
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.0).ok();
            }
        }
        let dir = std::env::temp_dir().join(format!("nc-pre-encode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _cleanup = Cleanup(dir.clone());
        let input = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hdr-48bit.tif");
        // Converts with the export when one is given; returns the report.
        let convert = |output: &Path, export: Option<&Path>, destination: &[&str]| {
            let report = output.with_extension("report.json");
            let mut argv = vec![
                "hanten",
                "convert",
                input.to_str().unwrap(),
                "-o",
                output.to_str().unwrap(),
                "--film-base",
                "0.9,0.55,0.42",
                "--quiet",
                "--report-file",
                report.to_str().unwrap(),
            ];
            if let Some(export) = export {
                argv.extend(["--export-pre-encode", export.to_str().unwrap()]);
            }
            argv.extend_from_slice(destination);
            let Command::Convert(args) = Cli::try_parse_from(argv).unwrap().command else {
                unreachable!("expected convert")
            };
            run_convert(args).unwrap();
            let report: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
            assert_eq!(
                report["pre_encode_exported"],
                export.map_or(serde_json::Value::Null, |e| e.to_str().unwrap().into())
            );
        };
        let f32s = |page: &Page| match &page.samples {
            DecodingResult::F32(data) => data.clone(),
            other => panic!("{}: expected f32, got {other:?}", page.buffer),
        };
        let summary = |pages: &[Page]| -> Vec<(String, String)> {
            pages
                .iter()
                .map(|p| (p.buffer.clone(), p.space.clone()))
                .collect()
        };
        let pair = |b: &str, s: &str| (b.to_string(), s.to_string());

        // Lossless destinations: the export is the file's pixels to the bit, and the
        // export leaves the output byte-identical.
        for (name, destination, buffer, space) in [
            ("master", &["--film-master"][..], "film-master", "acescg"),
            (
                "linear",
                &[
                    "--range",
                    "hdr",
                    "--transfer",
                    "linear",
                    "--gamut",
                    "bt2020",
                ][..],
                "hdr-linear",
                "bt2020",
            ),
        ] {
            let (out, plain, export) = (
                dir.join(format!("{name}.tiff")),
                dir.join(format!("{name}-plain.tiff")),
                dir.join(format!("{name}-pre.tiff")),
            );
            convert(&out, Some(&export), destination);
            convert(&plain, None, destination);
            assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(&plain).unwrap());
            let pages = read_pre_encode(&export);
            assert_eq!(summary(&pages), [pair(buffer, space)], "{name}");
            let written = read_f32_tiff(&out).2;
            let exported = f32s(&pages[0]);
            assert_eq!(written.len(), exported.len(), "{name}");
            assert!(
                written
                    .iter()
                    .zip(&exported)
                    .all(|(a, b)| a.to_bits() == b.to_bits()),
                "{name}: the export must be the written pixels"
            );
        }

        // SDR: the export is linear; through the shipped transfer and quantizer it is
        // the written codes.
        let (sdr, export) = (dir.join("sdr.tiff"), dir.join("sdr-pre.tiff"));
        convert(&sdr, Some(&export), &["--gamut", "adobe-rgb"]);
        let pages = read_pre_encode(&export);
        assert_eq!(summary(&pages), [pair("sdr-linear", "adobe-rgb")]);
        let linear =
            LinearImage::new(pages[0].width, pages[0].height, f32s(&pages[0]), None).unwrap();
        let (encoded, icc) =
            color::encode_display_linear(linear, DestinationGamut::AdobeRgb).unwrap();
        let again = dir.join("sdr-again.tiff");
        let (staged, ..) = encode::encode_u16(&encoded, &icc, &again).unwrap();
        staged::commit_all(vec![staged]).unwrap();
        assert_eq!(read_u16_tiff(&again), read_u16_tiff(&sdr));

        // PQ: the export is the linear BT.2020 hand-off, the same buffer the linear
        // TIFF writes.
        let (pq, export) = (dir.join("pq.tiff"), dir.join("pq-pre.tiff"));
        convert(
            &pq,
            Some(&export),
            &["--range", "hdr", "--transfer", "pq", "--gamut", "bt2020"],
        );
        let pages = read_pre_encode(&export);
        assert_eq!(summary(&pages), [pair("hdr-linear", "bt2020")]);
        assert_eq!(
            f32s(&pages[0]),
            f32s(&read_pre_encode(&dir.join("linear-pre.tiff"))[0])
        );

        // Gain map: both renditions, then the half-resolution map codes.
        let (jpeg, export) = (dir.join("gain.jpg"), dir.join("gain-pre.tiff"));
        convert(
            &jpeg,
            Some(&export),
            &[
                "--range",
                "hdr",
                "--transfer",
                "native",
                "--gamut",
                "srgb",
                "--container",
                "jpeg",
            ],
        );
        let pages = read_pre_encode(&export);
        assert_eq!(
            summary(&pages),
            [
                pair("sdr-linear", "srgb"),
                pair("hdr-linear", "srgb"),
                pair("gain-map-codes", "log2-gain-window"),
            ]
        );
        let (w, h) = (pages[0].width, pages[0].height);
        assert_eq!((pages[1].width, pages[1].height), (w, h));
        assert_eq!(
            (pages[2].width, pages[2].height),
            (w.div_ceil(2), h.div_ceil(2))
        );
        assert!(matches!(pages[2].samples, DecodingResult::U8(_)));
        // The HDR rendition is the one the linear sRGB TIFF writes.
        let (linear, export_linear) = (dir.join("srgb.tiff"), dir.join("srgb-pre.tiff"));
        convert(
            &linear,
            Some(&export_linear),
            &["--range", "hdr", "--transfer", "linear", "--gamut", "srgb"],
        );
        assert_eq!(f32s(&pages[1]), f32s(&read_pre_encode(&export_linear)[0]));
        assert_ne!(f32s(&pages[0]), f32s(&pages[1]));

        // On the encoders that transform after the export, the output is unchanged too.
        for (exported, destination) in [
            (&sdr, &["--gamut", "adobe-rgb"][..]),
            (
                &pq,
                &["--range", "hdr", "--transfer", "pq", "--gamut", "bt2020"][..],
            ),
            (
                &jpeg,
                &["--range", "hdr", "--gamut", "srgb", "--container", "jpeg"][..],
            ),
        ] {
            let plain = dir.join(format!(
                "plain-{}",
                exported.file_name().unwrap().to_str().unwrap()
            ));
            convert(&plain, None, destination);
            assert_eq!(
                std::fs::read(exported).unwrap(),
                std::fs::read(&plain).unwrap(),
                "{destination:?}"
            );
        }
    }
}

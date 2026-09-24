//! The new chain's recipe (`nf-core/recipe-schema`).
//!
//! One document, versioned **as a document**: a recipe for the new chain states
//! `"recipe_version": 2` at top level, and that marker is what makes the recipe
//! self-describing. `--new-flow` still selects the chain, but a recipe can no longer
//! mean one thing under the flag and another without it — each side refuses the
//! other's recipe by name ([`check_body`]). That closes the gap `deny_unknown_fields`
//! cannot: it rejects an *unknown* key, and is blind to a **known but meaningless**
//! one, such as a whole `print` section loaded under a chain that has no print stage.
//!
//! The sections follow the chain, one per stage, and an identity stage still has one:
//!
//! ```text
//! input · calibration · measure      shared with the current chain (decode, film base)
//! reconstruction                     the fixed decode — algo::fixed::DecodeParams
//! scene_correction · look ·          one per rendering stage, each empty until its
//! fit_range · fit_gamut                epic gives it a knob (scene correction and fit
//!                                      range have)
//! ```
//!
//! **No per-section `schema_version`.** The current chain's tagged `reconstruction`
//! carries one because it once had to tell several shapes apart inside a single
//! object; here the document version does that for every section at once.
//!
//! **No `output` section yet.** The new chain writes one fixed destination, so there is
//! no output policy to choose, and a section nothing reads is the defect this module
//! exists to prevent. `nf-destinations/preset-set` adds it with the destination set. Each key lands with the task that ships its knob — design-spec
//! §9 states the shape, not keys written ahead of the code.
//!
//! Named for what it will be rather than for the migration: after
//! `nf-core/default-flip` this is *the* recipe, and the current chain's
//! `cli::ResolvedConfig` is what gets deleted.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::algo::fixed::{AnchorRule, DecodeFault, DecodeParams};
use crate::cli::ResolvedConfig;
use crate::pipeline::chain::ChainParams;
use crate::pipeline::fit_gamut::{DestinationGamut, FitGamutParams};
use crate::pipeline::fit_range::{DisplayPeak, FitRangeParams};
use crate::pipeline::look::LookParams;
use crate::pipeline::scene_correction::{SceneCorrectionParams, SceneFault, WhiteBalance};
use crate::types::{
    CalibrationParams, FilmBaseSource, InputParams, MeasureParams, NcError, Result,
};

/// The only document version this build reads.
pub const RECIPE_VERSION: u32 = 2;

/// The top-level key carrying [`RECIPE_VERSION`].
///
/// Reserved beside `params` (the sidecar envelope's key): the current chain's recipe
/// must never gain a field of this name, or a v2 document would stop being
/// distinguishable from a v1 one. A test pins it absent from `ResolvedConfig`.
pub const VERSION_KEY: &str = "recipe_version";

/// A recipe for the new chain.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    /// Required on load, so a document that does not say which chain it describes
    /// is never read as this one. [`check_body`] refuses its absence with a
    /// migration message before serde would report a bare "missing field".
    pub recipe_version: RecipeVersion,
    #[serde(default)]
    pub input: InputParams,
    #[serde(default)]
    pub calibration: Calibration,
    #[serde(default)]
    pub measure: MeasureParams,
    #[serde(default)]
    pub reconstruction: DecodeParams,
    #[serde(default)]
    pub scene_correction: SceneCorrectionParams,
    #[serde(default)]
    pub look: LookParams,
    #[serde(default)]
    pub fit_range: FitRange,
    #[serde(default)]
    pub fit_gamut: FitGamut,
}

/// The document version — a type with exactly one value, so a recipe cannot
/// deserialize into a version this build does not read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecipeVersion;

impl Serialize for RecipeVersion {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_u32(RECIPE_VERSION)
    }
}

impl<'de> Deserialize<'de> for RecipeVersion {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let v = u64::deserialize(d)?;
        if v == u64::from(RECIPE_VERSION) {
            Ok(RecipeVersion)
        } else {
            Err(serde::de::Error::custom(format!(
                "`{VERSION_KEY}` is {v}; this build reads only {RECIPE_VERSION}"
            )))
        }
    }
}

/// Fit range's recipe section: how much scene range above diffuse white it
/// compresses.
///
/// Its own type rather than [`FitRangeParams`], for the reason [`FitGamut`] is: the
/// stage's other parameter, the display's peak, is the **destination's** to state, and
/// [`Recipe::chain_params`] adds it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FitRange {
    /// In stops above diffuse white: reinhard's white point is `2^headroom_stops`, and
    /// `0` is the identity.
    pub headroom_stops: f32,
}

impl Default for FitRange {
    fn default() -> Self {
        Self {
            headroom_stops: crate::types::DEFAULT_HEADROOM_STOPS,
        }
    }
}

/// Fit gamut's recipe section: empty until `nf-display-stages/fit-gamut` gives the
/// stage a mapping knob.
///
/// Its own type rather than [`FitGamutParams`], because the stage's one parameter
/// today — the target gamut — is the **destination's** to state, not the recipe's:
/// [`Recipe::chain_params`] adds it. A recipe key for it would be a second way to
/// choose primaries the destination already fixes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FitGamut {}

/// The new chain's roll calibration: the film base alone.
///
/// Its own type rather than the current chain's [`CalibrationParams`], because that
/// one also carries `dmax` — the reference density the fixed decode never reads
/// (its anchor rule is reference-free). Sharing the struct would write
/// `"dmax": "fixed"` into every new-flow recipe: a key claiming a reference the run
/// does not use. The section stays open, as design-spec §8 describes it; a later
/// measurement joins it with its own task.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Calibration {
    /// Required at `convert`/`roll` time with no default, exactly as on the current
    /// chain — `cli::validate` refuses an unstated one on the projection.
    pub film_base: Option<FilmBaseSource>,
}

/// The current chain's sections this recipe does not have, and where each one's
/// knobs go. `reconstruction` is not here: the name survives with a different
/// shape, and [`check_body`] diagnoses its old keys one by one.
const SECTIONS_WITH_NO_COUNTERPART: &[(&str, &str)] = &[
    (
        "print",
        "white balance and exposure are `scene_correction.white_balance` and \
         `scene_correction.exposure`; the display tone is fit range, whose one \
         operator is reinhard and whose headroom is `fit_range.headroom_stops`; the \
         black point splits between scene correction and fit range \
         (`nf-scene-correction/flare-removal`), and `linear_range` has no home yet \
         (`nf-scene-correction/levels-knob`) — neither of those two has a key yet",
    ),
    (
        "output",
        "the new chain writes one fixed destination, so there is no output policy to \
         choose; its output section arrives with the destination set \
         (`nf-destinations/preset-set`)",
    ),
];

/// The current chain's `reconstruction` keys, and each one's fate here.
const OLD_RECONSTRUCTION_KEYS: &[(&str, &str)] = &[
    (
        "schema_version",
        "the document's `recipe_version` versions every section at once",
    ),
    (
        "type",
        "there is one fixed decode, so no reconstruction type to choose",
    ),
    (
        "curve",
        "there is one curve: its slope is `reconstruction.contrast` and its placement \
         `reconstruction.anchor` (`{\"mid-at-base-offset\": <d>}`)",
    ),
    (
        "density",
        "`density.scale` and `density.offset` are `reconstruction.scale` and \
         `reconstruction.offset`; the regional balances have no counterpart yet \
         (`nf-look/per-channel-grade`)",
    ),
];

/// Keys neither chain reads any more — each already a migration error on the
/// current chain (`cli::reject_legacy_recipe_keys`), whose remedies point at that
/// chain's homes. These point at the new chain's, so the diagnosis survives the flag.
/// A path is top-level (`["density"]`) or one level down (`["input", "color"]`).
const RETIRED_KEYS: &[(&[&str], &str)] = &[
    (
        &["film_base"],
        "the film base is `calibration.film_base` (`\"auto\"`, `{\"region\": [x, y, w, h]}` \
         or `{\"explicit\": [r, g, b]}`)",
    ),
    (
        &["algorithm"],
        "there is one fixed decode, so no algorithm to choose",
    ),
    (
        &["density"],
        "`density.scale` and `density.offset` are `reconstruction.scale` and \
         `reconstruction.offset`",
    ),
    (
        &["sigmoid"],
        "there is one curve: its slope is `reconstruction.contrast` and its placement \
         `reconstruction.anchor`",
    ),
    (
        &["simple"],
        "there is one fixed decode, so no algorithm to choose",
    ),
    (
        &["input", "color"],
        "it conflated transfer encoding with measurement meaning; use the independent \
         keys `input.transfer` (auto|linear) and `input.meaning` \
         (auto|scanner-device|colorimetric)",
    ),
];

/// The current chain's `calibration` keys this recipe does not have.
const OLD_CALIBRATION_KEYS: &[(&str, &str)] = &[(
    "dmax",
    "the fixed decode's anchor rule never reads a reference density",
)];

/// Refuse a recipe body written for the other chain, before serde sees it.
///
/// Run on the raw JSON, because the failure is about presence: a missing marker, or
/// a key that parses on one chain and means nothing on the other. `whole` is `true`
/// for a whole recipe and `false` for a `roll` per-frame overlay, which is a partial
/// document merged onto an already-versioned one and so need not restate the
/// version (it may, and must then state the right one).
///
/// One diagnosis per call, the first found: a user fixing a recipe removes keys one
/// at a time, and a list reads as though all of them had to go before anything else
/// could be checked.
pub fn check_body(body: &serde_json::Value, whole: bool, context: &str) -> Result<()> {
    let usage = |m: String| Err(NcError::Usage(format!("{context}: {m}")));
    match body.get(VERSION_KEY) {
        None if whole => {
            return usage(format!(
                "under `--new-flow` a recipe must state `\"{VERSION_KEY}\": {RECIPE_VERSION}` — \
                 without it the document describes the current chain, whose sections this \
                 chain does not read. `hanten params --new-flow` writes the new layout; or run \
                 without `--new-flow`, where a recipe with no `{VERSION_KEY}` is read"
            ));
        }
        Some(v) if v.as_u64() != Some(u64::from(RECIPE_VERSION)) => {
            return usage(format!(
                "`{VERSION_KEY}` is {v}; this build reads only {RECIPE_VERSION}"
            ));
        }
        _ => {}
    }
    for (path, why) in RETIRED_KEYS {
        let found = match path {
            [key] => body.get(key),
            [section, key] => body.get(section).and_then(|s| s.get(key)),
            _ => unreachable!("a retired key is one or two levels deep"),
        };
        if found.is_some() {
            return usage(format!(
                "`{}` is not a key of either chain's recipe any more: {why}",
                path.join(".")
            ));
        }
    }
    for (section, why) in SECTIONS_WITH_NO_COUNTERPART {
        if body.get(section).is_some() {
            return usage(format!(
                "`{section}` is a section of the current chain's recipe, not the new one's: \
                 {why}. Drop it — the current chain reads it only in a recipe with no \
                 `{VERSION_KEY}`"
            ));
        }
    }
    let old_key = |section: &str, table: &[(&'static str, &'static str)]| {
        let fields = body.get(section)?.as_object()?;
        table
            .iter()
            .find(|(key, _)| fields.contains_key(*key))
            .map(|(key, why)| (*key, *why))
    };
    // Retired by `nf-scene-correction/roll-white-balance`. Named here rather than left
    // to serde, whose "unknown variant" says nothing about where the mode went.
    if let Some(mode) = body
        .get("scene_correction")
        .and_then(|s| s.get("white_balance"))
        .and_then(|w| w.as_str())
        .filter(|m| ["gray-world", "percentile"].contains(m))
    {
        return usage(format!(
            "`scene_correction.white_balance` \"{mode}\" was a per-frame estimate, and the \
             new chain has none: it read a sunset as the cast and removed it. Drop it, \
             then state the gains `hanten measure-roll` reports for the roll, as \
             `{{\"explicit\": [r, g, b]}}`"
        ));
    }
    for (section, table) in [
        ("reconstruction", OLD_RECONSTRUCTION_KEYS),
        ("calibration", OLD_CALIBRATION_KEYS),
    ] {
        if let Some((key, why)) = old_key(section, table) {
            return usage(format!(
                "`{section}.{key}` belongs to the current chain's recipe, not the new one's: \
                 {why}. Drop it — the current chain reads it only in a recipe with no \
                 `{VERSION_KEY}`"
            ));
        }
    }
    Ok(())
}

/// Refuse a new-chain recipe loaded **without** `--new-flow`.
///
/// The other half of the marker's contract. Without it the current chain's
/// `deny_unknown_fields` would still refuse the document, but with an opaque
/// "unknown field" message that never says the recipe was fine for the other chain.
pub fn check_body_without_flag(body: &serde_json::Value, context: &str) -> Result<()> {
    // The remedy says only what the value supports: "pass `--new-flow`" is advice
    // only for the version the new chain reads, or the user is sent to a second
    // refusal (`recipe_version` 1 is the likely case: meant as "the current schema").
    match body.get(VERSION_KEY) {
        None => Ok(()),
        Some(v) if v.as_u64() == Some(u64::from(RECIPE_VERSION)) => Err(NcError::Usage(format!(
            "{context}: states `{VERSION_KEY}`, so it describes the new rendering chain, \
                 which only `--new-flow` reads. The current chain's recipe carries no version"
        ))),
        Some(v) => Err(NcError::Usage(format!(
            "{context}: states `{VERSION_KEY}` {v}, which no chain reads — the current \
             chain's recipe carries no `{VERSION_KEY}` at all (remove it), and the new \
             chain (`--new-flow`) reads only {RECIPE_VERSION}"
        ))),
    }
}

/// Apply the command-line flags the new chain reads, flags winning over the recipe.
///
/// Every other conversion flag is either refused by presence before this runs
/// (`flow::reject_unavailable_flags`) or names what the decode already does
/// (`--density-curve exponential`), so it has nothing to set. (`--reconstruction` is
/// refused on both chains, before either runs.) `flow`'s `every_kept_flag_reaches_the_recipe` holds that each kept flag
/// has an arm here.
pub fn merge(mut r: Recipe, args: &crate::cli::ConvertArgs) -> Recipe {
    crate::cli::merge_shared_sections(
        &mut r.input,
        &mut r.calibration.film_base,
        &mut r.measure,
        args,
    );
    // The decode's own knobs. `--density-gamma` is the decode's contrast: the flag
    // keeps its current spelling, and the split into a calibrated half and a look
    // half is `nf-reconstruction/gamma-split`'s, which also owns renaming it.
    if let Some(v) = args.density.density_scale {
        r.reconstruction.scale = v;
    }
    if let Some(v) = args.density.density_offset {
        r.reconstruction.offset = v;
    }
    if let Some(v) = args.density.density_gamma {
        r.reconstruction.contrast = v;
    }
    if let Some(d) = args.anchor.anchor_mid_offset {
        r.reconstruction.anchor = AnchorRule::MidAboveBase(d);
    }
    // Scene correction. `--auto-wb` never reaches here: `flow` refuses it by
    // presence, since this chain has no per-frame estimate.
    if let Some(gains) = args.print.white_balance {
        r.scene_correction.white_balance = WhiteBalance::Explicit(gains);
    }
    if let Some(stops) = args.scene.exposure {
        r.scene_correction.exposure = stops;
    }
    if let Some(stops) = args.print.display_tone_headroom {
        r.fit_range.headroom_stops = stops;
    }
    r
}

/// How a validation message names a knob: by the flag and the recipe key on
/// `convert`, which accepts both, and by the key alone on `roll`, which accepts no
/// conversion flags — naming a flag there hands the user a remedy they cannot type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnobNames {
    FlagAndKey,
    KeyOnly,
}

/// A knob as a validation message names it — one spelling rule for every section.
fn knob_name(names: KnobNames, section: &str, flag: &str, key: &str) -> String {
    match names {
        KnobNames::FlagAndKey => format!("{flag} (recipe `{section}.{key}`)"),
        KnobNames::KeyOnly => format!("`{section}.{key}`"),
    }
}

/// The value rules this recipe's own sections carry: the decode's, which live in
/// [`DecodeParams::check`] and are rendered here as a usage error. The shared
/// sections are checked on the projection, by the same `cli::validate` the current
/// chain uses.
pub fn validate(r: &Recipe, names: KnobNames) -> Result<()> {
    let name = |flag: &str, key: &str| knob_name(names, "reconstruction", flag, key);
    let d = &r.reconstruction;
    let message = match d.check() {
        Ok(_) => {
            validate_scene_correction(&r.scene_correction, names)?;
            return validate_fit_range(&r.fit_range, names);
        }
        Err(DecodeFault::Offset { channel, value }) => format!(
            "{} must be finite on every channel, got {value} on channel {channel}",
            name("--density-offset", "offset")
        ),
        Err(DecodeFault::Scale { channel, value }) => format!(
            "{} must be finite and positive on every channel, got {value} on channel {channel}",
            name("--density-scale", "scale")
        ),
        Err(DecodeFault::Contrast(v)) => format!(
            "{} must be finite and positive, got {v}",
            name("--density-gamma", "contrast")
        ),
        Err(DecodeFault::MidAboveBase(v)) => format!(
            "{} must be finite and positive, got {v}",
            name("--anchor-mid-offset", "anchor")
        ),
        Err(DecodeFault::Anchor { anchor, contrast }) => {
            let AnchorRule::MidAboveBase(mid) = d.anchor;
            // Two routes, and opposite remedies: a tiny contrast overflows the anchor
            // itself (`0.745 / contrast`), while a huge offset overflows only the
            // exponent `contrast · anchor`, where a larger contrast makes it worse.
            let remedy = if anchor.is_finite() {
                format!("Use a smaller {}", name("--anchor-mid-offset", "anchor"))
            } else {
                format!("Use a larger {}", name("--density-gamma", "contrast"))
            };
            format!(
                "the decode's anchor is not usable at {} {contrast:e} and {} {mid:e}: it \
                 derives an anchor of {anchor:e}, whose exponent overflows f32 and would \
                 render every sample as exactly 0.0. {remedy}",
                name("--density-gamma", "contrast"),
                name("--anchor-mid-offset", "anchor"),
            )
        }
    };
    Err(NcError::Usage(message))
}

/// Scene correction's value rules ([`SceneCorrectionParams::check`]), rendered as a
/// usage error naming the knob the way `names` says the command spells it.
fn validate_scene_correction(p: &SceneCorrectionParams, names: KnobNames) -> Result<()> {
    let name = |flag: &str, key: &str| knob_name(names, "scene_correction", flag, key);
    let message = match p.check() {
        Ok(()) => return Ok(()),
        Err(SceneFault::WhiteBalance { channel, value }) => format!(
            "{} must be finite and positive on every channel, got {value} on channel \
             {channel}",
            name("--white-balance", "white_balance")
        ),
        Err(SceneFault::Exposure(stops)) => format!(
            "{} must be finite, with a gain 2^EV that is a normal f32 (roughly -126 to \
             +127 stops), got {stops}",
            name("--exposure", "exposure")
        ),
        Err(SceneFault::Combined { channel, gain }) => format!(
            "{} times the exposure gain from {} is {gain:e} on channel {channel}, which \
             is not a normal f32 — every sample of that channel would render as 0 or \
             inf. Move the white balance or the exposure toward neutral",
            name("--white-balance", "white_balance"),
            name("--exposure", "exposure"),
        ),
    };
    Err(NcError::Usage(message))
}

/// Fit range's value rule — the headroom's, [`crate::types::headroom_fault`], shared
/// with the current chain's knob — rendered as a usage error for this recipe's key.
fn validate_fit_range(p: &FitRange, names: KnobNames) -> Result<()> {
    use crate::types::HeadroomFault;
    let name = knob_name(
        names,
        "fit_range",
        "--display-tone-headroom",
        "headroom_stops",
    );
    let message = match crate::types::headroom_fault(p.headroom_stops) {
        None => return Ok(()),
        Some(HeadroomFault::Negative(stops)) => format!(
            "{name} must be finite and non-negative, got {stops}. It is the scene range \
             above diffuse white that fit range compresses, in stops; `0` is the identity"
        ),
        Some(HeadroomFault::TooLarge(stops)) => format!(
            "{name} is {stops} stops, beyond the supported maximum of {}: above ~8 stops \
             the operator converges on plain reinhard and the extra headroom buys nothing",
            crate::types::MAX_HEADROOM_STOPS
        ),
    };
    Err(NcError::Usage(message))
}

impl Recipe {
    /// The stages' parameters, in chain order, for `pipeline::chain::render` — the
    /// recipe's sections plus the destination's peak and gamut, which only the
    /// destination states.
    pub fn chain_params(&self, peak: DisplayPeak, target: DestinationGamut) -> ChainParams {
        ChainParams {
            scene_correction: self.scene_correction.clone(),
            look: self.look.clone(),
            fit_range: FitRangeParams {
                headroom_stops: self.fit_range.headroom_stops,
                peak,
            },
            fit_gamut: FitGamutParams { target },
        }
    }

    /// The current chain's config carrying this recipe's **shared** sections, for the
    /// stages both chains run: decode, the film base and the measurement region.
    ///
    /// Scaffolding, deleted with `ResolvedConfig` by `nf-core/default-flip`. Every
    /// other section is left at its default, which is safe only because nothing past
    /// the film base reads them on the new flow: the decode reads
    /// [`Recipe::reconstruction`] and the chain [`Recipe::chain_params`], never this
    /// projection, and the destination is fixed rather than resolved from `output`.
    pub fn to_config(&self) -> ResolvedConfig {
        ResolvedConfig {
            input: self.input.clone(),
            calibration: CalibrationParams {
                film_base: self.calibration.film_base.clone(),
                ..CalibrationParams::default()
            },
            measure: self.measure.clone(),
            ..ResolvedConfig::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> std::result::Result<Recipe, serde_json::Error> {
        serde_json::from_str(json)
    }

    fn check(json: &str, whole: bool) -> std::result::Result<(), String> {
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        check_body(&v, whole, "recipe r.json").map_err(|e| e.message().to_string())
    }

    /// Top-level keys of a serialized document, in the order they are written.
    /// (A `serde_json::Value` map sorts its keys, so it cannot answer this.)
    fn written_order(text: &str) -> Vec<String> {
        let mut depth = 0usize;
        let mut keys = Vec::new();
        let mut chars = text.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            match c {
                '{' | '[' => depth += 1,
                '}' | ']' => depth -= 1,
                '"' => {
                    let end = text[i + 1..].find('"').unwrap() + i + 1;
                    if depth == 1 && text[end + 1..].trim_start().starts_with(':') {
                        keys.push(text[i + 1..end].to_string());
                    }
                    while chars.peek().is_some_and(|(j, _)| *j <= end) {
                        chars.next();
                    }
                }
                _ => {}
            }
        }
        keys
    }

    #[test]
    fn the_default_document_names_every_stage_in_chain_order_and_round_trips() {
        let text = serde_json::to_string_pretty(&Recipe::default()).unwrap();
        assert_eq!(
            written_order(&text),
            [
                VERSION_KEY,
                "input",
                "calibration",
                "measure",
                "reconstruction",
                "scene_correction",
                "look",
                "fit_range",
                "fit_gamut",
            ]
        );
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        // A stage with no knob yet is present as an empty object, not absent or
        // `null`; one with knobs writes each of them at its default — the identity for
        // scene correction, and reinhard at six stops for fit range, which is the one
        // stage whose default does something.
        for stage in ["look", "fit_gamut"] {
            assert_eq!(json[stage], serde_json::json!({}), "{stage}");
        }
        assert_eq!(
            json["scene_correction"],
            serde_json::json!({"white_balance": {"explicit": [1.0, 1.0, 1.0]}, "exposure": 0.0})
        );
        assert_eq!(
            json["fit_range"],
            serde_json::json!({"headroom_stops": 6.0})
        );
        assert_eq!(json[VERSION_KEY], RECIPE_VERSION);
        let back: Recipe = serde_json::from_str(&text).unwrap();
        assert_eq!(back, Recipe::default());
    }

    #[test]
    fn the_decode_section_spells_the_decode_parameters() {
        // Compared as text: a `Value` holds the f32 defaults widened to f64
        // (`0.8399999737739563`), while the written document spells `0.84`.
        assert_eq!(
            serde_json::to_string(&Recipe::default().reconstruction).unwrap(),
            r#"{"scale":[1.0,0.84,0.73],"offset":[0.0,0.0,0.0],"contrast":2.0,"anchor":{"mid-at-base-offset":0.62}}"#
        );
        // No reference density in the new calibration section.
        let json = serde_json::to_value(Recipe::default()).unwrap();
        assert_eq!(json["calibration"], serde_json::json!({"film_base": null}));
    }

    #[test]
    fn a_partial_recipe_takes_the_defaults_it_omits() {
        let r = parse(r#"{"recipe_version": 2, "reconstruction": {"contrast": 1.8}}"#).unwrap();
        assert_eq!(r.reconstruction.contrast, 1.8);
        assert_eq!(r.reconstruction.scale, DecodeParams::default().scale);
        assert_eq!(r.look, LookParams::default());
    }

    #[test]
    fn the_version_is_required_and_exact() {
        assert!(
            parse("{}")
                .unwrap_err()
                .to_string()
                .contains("recipe_version")
        );
        let err = parse(r#"{"recipe_version": 3}"#).unwrap_err().to_string();
        assert!(err.contains("reads only 2"), "{err}");
    }

    #[test]
    fn every_section_rejects_an_unknown_key() {
        for section in [
            "input",
            "calibration",
            "measure",
            "reconstruction",
            "scene_correction",
            "look",
            "fit_range",
            "fit_gamut",
        ] {
            let json = format!(r#"{{"recipe_version": 2, "{section}": {{"nonsense": 1}}}}"#);
            let err = parse(&json).unwrap_err().to_string();
            assert!(err.contains("nonsense"), "{section}: {err}");
        }
        let err = parse(r#"{"recipe_version": 2, "nonsense": {}}"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("nonsense"), "{err}");
    }

    #[test]
    fn a_retired_per_frame_white_balance_names_the_roll_measurement() {
        for mode in ["gray-world", "percentile"] {
            let body = format!(r#"{{"scene_correction": {{"white_balance": "{mode}"}}}}"#);
            let err = check(&body, false).unwrap_err();
            assert!(
                err.contains(mode) && err.contains("measure-roll") && err.contains("explicit"),
                "{err}"
            );
        }
        // Only the two retired names: any other string is a typo for serde to report,
        // not "a per-frame estimate".
        check(
            r#"{"scene_correction": {"white_balance": "neutral"}}"#,
            false,
        )
        .unwrap();
        // Stated gains are the one form, and pass the schema check.
        check(
            r#"{"scene_correction": {"white_balance": {"explicit": [1.2, 1, 1.1]}}}"#,
            false,
        )
        .unwrap();
    }

    #[test]
    fn a_body_without_the_marker_is_refused_as_the_current_chains() {
        let err = check(r#"{"calibration": {"film_base": "auto"}}"#, true).unwrap_err();
        assert!(err.contains("\"recipe_version\": 2"), "{err}");
        assert!(err.contains("hanten params --new-flow"), "{err}");
        // A per-frame overlay is partial and may omit it…
        check(r#"{"calibration": {"film_base": "auto"}}"#, false).unwrap();
        // …but may not state a wrong one.
        let err = check(r#"{"recipe_version": 1}"#, false).unwrap_err();
        assert!(err.contains("reads only 2"), "{err}");
    }

    #[test]
    fn an_old_section_is_refused_by_name_with_where_it_went() {
        let err = check(r#"{"recipe_version": 2, "print": {}}"#, true).unwrap_err();
        assert!(
            err.contains("`print`") && err.contains("scene_correction"),
            "{err}"
        );
        let err = check(r#"{"recipe_version": 2, "output": {}}"#, true).unwrap_err();
        assert!(
            err.contains("`output`") && err.contains("preset-set"),
            "{err}"
        );
        let err = check(
            r#"{"recipe_version": 2, "reconstruction": {"density": {"scale": [1, 1, 1]}}}"#,
            true,
        )
        .unwrap_err();
        assert!(err.contains("reconstruction.density") && err.contains("reconstruction.scale"));
        let err = check(
            r#"{"recipe_version": 2, "reconstruction": {"curve": {"type": "exponential"}}}"#,
            true,
        )
        .unwrap_err();
        assert!(err.contains("reconstruction.contrast"), "{err}");
        let err = check(
            r#"{"recipe_version": 2, "calibration": {"dmax": "fixed"}}"#,
            true,
        )
        .unwrap_err();
        assert!(err.contains("calibration.dmax") && err.contains("never reads a reference"));
        // Overlays get the same diagnosis.
        let err = check(r#"{"print": {"print_exposure": 1}}"#, false).unwrap_err();
        assert!(err.contains("`print`"), "{err}");
        // The remedy must not send the user to the current chain as-is: this document
        // states `recipe_version`, which that chain refuses outright.
        assert!(!err.contains("run without `--new-flow`"), "{err}");
    }

    #[test]
    fn a_new_chain_recipe_is_refused_without_the_flag() {
        let v = serde_json::json!({"recipe_version": 2});
        let err = check_body_without_flag(&v, "recipe r.json").unwrap_err();
        assert!(err.message().contains("only `--new-flow` reads"), "{err}");
        check_body_without_flag(&serde_json::json!({}), "recipe r.json").unwrap();
        // Any other value reads on neither chain, so the flag is not the remedy: under
        // it, `1` would be refused again for the version.
        let v = serde_json::json!({"recipe_version": 1});
        let err = check_body_without_flag(&v, "recipe r.json").unwrap_err();
        assert!(!err.message().contains("pass `--new-flow`"), "{err}");
        assert!(err.message().contains("remove it"), "{err}");
    }

    /// The marker distinguishes the two documents only while the current chain's
    /// recipe cannot carry it — the same reserved-key rule as the envelope's `params`.
    #[test]
    fn the_version_key_is_reserved_and_no_section_is_named_params() {
        let old = serde_json::to_value(ResolvedConfig::default()).unwrap();
        assert!(old.get(VERSION_KEY).is_none());
        let new = serde_json::to_value(Recipe::default()).unwrap();
        assert!(new.get("params").is_none());
    }

    /// The recipe half of the availability inventory: every section and key the
    /// current chain's recipe can carry is either shared with this one or refused by
    /// name. A key added to the current chain's schema and classified nowhere would
    /// reach the new flow as an opaque "unknown field" — or, in a shared section, as
    /// a key the new flow parses and never reads.
    #[test]
    fn every_key_of_the_current_chains_recipe_is_shared_or_diagnosed() {
        let old = serde_json::to_value(ResolvedConfig::default()).unwrap();
        let new = serde_json::to_value(Recipe::default()).unwrap();
        for (section, fields) in old.as_object().unwrap() {
            if SECTIONS_WITH_NO_COUNTERPART
                .iter()
                .any(|(s, _)| s == section)
            {
                assert!(new.get(section).is_none(), "{section}");
                continue;
            }
            let shared = new
                .get(section)
                .unwrap_or_else(|| panic!("`{section}` is neither shared nor diagnosed"));
            let table: &[(&str, &str)] = match section.as_str() {
                "reconstruction" => OLD_RECONSTRUCTION_KEYS,
                "calibration" => OLD_CALIBRATION_KEYS,
                _ => &[],
            };
            for key in fields.as_object().unwrap().keys() {
                let diagnosed = table.iter().any(|(k, _)| k == key);
                assert!(
                    shared.get(key).is_some() != diagnosed,
                    "`{section}.{key}` must be exactly one of shared or diagnosed"
                );
            }
        }
    }

    #[test]
    fn validate_refuses_each_unusable_decode_value() {
        let with = |f: fn(&mut DecodeParams)| {
            let mut r = Recipe::default();
            f(&mut r.reconstruction);
            validate(&r, KnobNames::FlagAndKey).map_err(|e| e.message().to_string())
        };
        validate(&Recipe::default(), KnobNames::FlagAndKey).unwrap();
        assert!(
            with(|d| d.scale[1] = 0.0)
                .unwrap_err()
                .contains("reconstruction.scale")
        );
        assert!(
            with(|d| d.offset[2] = f32::NAN)
                .unwrap_err()
                .contains("reconstruction.offset")
        );
        assert!(
            with(|d| d.contrast = -1.0)
                .unwrap_err()
                .contains("reconstruction.contrast")
        );
        assert!(
            with(|d| d.anchor = AnchorRule::MidAboveBase(0.0))
                .unwrap_err()
                .contains("reconstruction.anchor")
        );
        // A contrast small enough that `0.745 / contrast` overflows.
        // The remedy follows the route: here the anchor itself overflows, so a
        // larger contrast is the fix…
        let err = with(|d| d.contrast = 1e-39).unwrap_err();
        assert!(err.contains("anchor is not usable"), "{err}");
        assert!(err.contains("Use a larger --density-gamma"), "{err}");
        // A finite anchor whose exponent still overflows.
        // …and here only the exponent does, where a larger contrast makes it worse.
        let err = with(|d| d.anchor = AnchorRule::MidAboveBase(2e38)).unwrap_err();
        assert!(err.contains("anchor is not usable"), "{err}");
        assert!(err.contains("Use a smaller --anchor-mid-offset"), "{err}");
        assert!(!err.contains("larger"), "{err}");
    }

    #[test]
    fn roll_names_the_key_alone() {
        // `roll` accepts no conversion flags, so naming one there is a remedy the
        // user cannot type.
        let mut r = Recipe::default();
        r.reconstruction.contrast = 0.0;
        let msg = validate(&r, KnobNames::KeyOnly).unwrap_err();
        let msg = msg.message();
        assert!(msg.contains("`reconstruction.contrast`"), "{msg}");
        assert!(!msg.contains("--density-gamma"), "{msg}");
    }

    #[test]
    fn validate_refuses_an_unusable_fit_range_headroom() {
        let with = |stops: f32, names| {
            let mut r = Recipe::default();
            r.fit_range.headroom_stops = stops;
            validate(&r, names).map_err(|e| e.message().to_string())
        };
        with(0.0, KnobNames::FlagAndKey).unwrap();
        with(crate::types::MAX_HEADROOM_STOPS, KnobNames::FlagAndKey).unwrap();
        for bad in [-1.0, f32::NAN, f32::INFINITY] {
            let err = with(bad, KnobNames::FlagAndKey).unwrap_err();
            assert!(
                err.contains("--display-tone-headroom (recipe `fit_range.headroom_stops`)")
                    && err.contains("non-negative"),
                "{bad}: {err}"
            );
        }
        let err = with(25.0, KnobNames::KeyOnly).unwrap_err();
        assert!(err.contains("`fit_range.headroom_stops` is 25"), "{err}");
        assert!(!err.contains("--display-tone-headroom"), "{err}");
    }

    #[test]
    fn the_destination_states_fit_ranges_peak() {
        // The recipe carries the headroom; the peak comes from the destination, so a
        // recipe cannot name a peak its destination does not have.
        let mut r = Recipe::default();
        r.fit_range.headroom_stops = 4.0;
        let p = r.chain_params(DisplayPeak::SDR, DestinationGamut::DisplayP3);
        assert_eq!(p.fit_range.headroom_stops, 4.0);
        assert_eq!(p.fit_range.peak, DisplayPeak::SDR);
        let err = parse(r#"{"recipe_version": 2, "fit_range": {"peak": 4.9}}"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("peak"), "{err}");
    }

    #[test]
    fn a_key_both_chains_retired_points_at_the_new_home() {
        // The current chain's migration errors point at its own homes
        // (`reconstruction.density.scale`), so the new chain needs its own wording
        // rather than an opaque "unknown field".
        for (json, needles) in [
            (
                r#"{"recipe_version": 2, "density": {"scale": [1, 1, 1]}}"#,
                &["`density`", "`reconstruction.scale`"][..],
            ),
            (
                r#"{"recipe_version": 2, "film_base": {"source": "auto"}}"#,
                &["`film_base`", "`calibration.film_base`"],
            ),
            (
                r#"{"recipe_version": 2, "algorithm": "density"}"#,
                &["`algorithm`"],
            ),
            (r#"{"recipe_version": 2, "sigmoid": {}}"#, &["`sigmoid`"]),
            (r#"{"recipe_version": 2, "simple": {}}"#, &["`simple`"]),
            (
                r#"{"recipe_version": 2, "input": {"color": "linear"}}"#,
                &["`input.color`", "`input.transfer`"],
            ),
        ] {
            let err = check(json, true).unwrap_err();
            for needle in needles {
                assert!(err.contains(needle), "{json}: {err}");
            }
            // An overlay gets the same diagnosis.
            let overlay = json.replace(r#""recipe_version": 2, "#, "");
            assert!(check(&overlay, false).unwrap_err().contains(needles[0]));
        }
    }

    #[test]
    fn the_projection_carries_the_shared_sections_and_nothing_else() {
        let mut r = Recipe::default();
        r.calibration.film_base = Some(FilmBaseSource::Auto);
        r.measure.inset = 0.1;
        let cfg = r.to_config();
        assert_eq!(cfg.calibration.film_base, Some(FilmBaseSource::Auto));
        assert_eq!(cfg.measure.inset, 0.1);
        assert_eq!(cfg.input, r.input);
        let defaults = ResolvedConfig::default();
        assert_eq!(cfg.calibration.dmax, defaults.calibration.dmax);
        assert_eq!(cfg.reconstruction, defaults.reconstruction);
        assert_eq!(cfg.print, defaults.print);
        assert_eq!(cfg.output, defaults.output);
    }
}

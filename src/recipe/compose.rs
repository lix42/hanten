//! Composing a recipe from layers: repeated `--params` files, in order, and then a
//! `roll` frame's manifest `params` over that frame's resolved recipe.
//!
//! One merge rule for every layer ([`merge_json`]): an object merges key by key,
//! anything else replaces, and a `null` states nothing (a frame's `params` refuses one
//! before merging, since there it can only be an attempt to unset). The design and its open
//! questions are `docs/design/roll-workflow.md` ("Layering and precedence").
//!
//! **`--params` layers merge onto the serialized default recipe, never onto each
//! other** ([`compose`]). Two partial layers `{"look": {"contrast": …}}` and
//! `{"look": {"channel_grade": …}}` have the shape of an enum variant switch
//! ([`is_variant_switch`]), and merged directly the second would drop the first. In the
//! full document every struct states all its keys, so the shape never arises.

use serde_json::Value;

use super::Recipe;

/// The recipe `layers` compose to, in order, later winning.
///
/// Each layer should already have loaded on its own (`cli`'s loader does), so a fault
/// is named against the file that has it; an error here is one only the combination
/// has.
pub fn compose<'a>(layers: impl IntoIterator<Item = &'a Value>) -> serde_json::Result<Recipe> {
    let mut doc = serde_json::to_value(Recipe::default())?;
    for layer in layers {
        merge_json(&mut doc, layer);
    }
    serde_json::from_value(doc)
}

/// Merge `overlay` into `base`: objects merge key by key (recursively), a `null`
/// is skipped, and any other value replaces.
///
/// **`null` states nothing**, so `hanten params`' unset keys erase no earlier layer
/// and no layer can unset one. Any stated value wins, a restated default included:
/// put a measured file last.
///
/// **An enum variant switch replaces wholesale.** An externally tagged enum (e.g.
/// [`FilmBaseSource`]) is a one-key map, and a key-by-key merge of `{"region": …}` and
/// `{"explicit": …}` would leave a two-tag object no variant deserializes
/// ([`is_variant_switch`]). The same tag on both sides deep-merges, which in recipe v2
/// matters only for `output.display`, whose axes are meant to union. A recipe map
/// ([`MAPS`]) always merges entry by entry. A malformed overlay is refused by the
/// caller's deserialize, never applied half-merged.
///
/// [`FilmBaseSource`]: crate::types::FilmBaseSource
pub fn merge_json(base: &mut Value, overlay: &Value) {
    merge_at(base, overlay, &mut Vec::new());
}

/// The recipe's maps, by path. Their keys are data (a frame's file name), not enum
/// tags, so two one-entry tables must union rather than read as a variant switch; and
/// their entries are structs of optional fields, so `{"white_stops": …}` and
/// `{"exposure": …}` for one file merge too.
const MAPS: &[&[&str]] = &[&["roll", "frames"]];

/// [`merge_json`] at `path` from the document root.
fn merge_at<'a>(base: &mut Value, overlay: &'a Value, path: &mut Vec<&'a str>) {
    if overlay.is_null() {
        return;
    }
    let is_map = MAPS.contains(&path.as_slice());
    let is_entry = path
        .split_last()
        .is_some_and(|(_, parent)| MAPS.contains(&parent));
    if !is_map && !is_entry && is_variant_switch(base, overlay) {
        *base = overlay.clone();
        return;
    }
    match (base, overlay) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o.iter().filter(|(_, v)| !v.is_null()) {
                path.push(k);
                merge_at(b.entry(k.clone()).or_insert(Value::Null), v, path);
                path.pop();
            }
        }
        (b, o) => *b = o.clone(),
    }
}

/// The externally-tagged-enum-variant-switch signature: `base` and `overlay` are
/// both single-key objects with *different* keys (e.g. `{"region":[…]}` vs
/// `{"explicit":[…]}`). A unit variant serializes as a bare string, so switching to or
/// from one takes [`merge_json`]'s plain replace arm.
///
/// **Not a struct of optional fields that happens to state one key.** The recipe's
/// `output.display` serializes only its stated axes, so a shared `{"transfer": "pq"}`
/// and a per-frame `{"container": "tiff"}` have the same shape; two keys that are both
/// destination axes ([`crate::destination::AXIS_KEYS`]) are therefore merged field by
/// field. No externally tagged enum in the recipe has a variant of those names.
pub(crate) fn is_variant_switch(base: &Value, overlay: &Value) -> bool {
    let axis =
        |k: Option<&String>| k.is_some_and(|k| crate::destination::AXIS_KEYS.contains(&k.as_str()));
    match (base, overlay) {
        (Value::Object(b), Value::Object(o)) => {
            b.len() == 1
                && o.len() == 1
                && b.keys().next() != o.keys().next()
                && !(axis(b.keys().next()) && axis(o.keys().next()))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_json_deep_merges_objects_and_replaces_other_values() {
        // Objects merge key-by-key (recursively); scalars/arrays replace wholesale.
        let mut base = json!({"a": {"x": 1, "y": 2}, "b": 3});
        let overlay = json!({"a": {"y": 20, "z": 30}, "b": [1, 2]});
        merge_json(&mut base, &overlay);
        assert_eq!(base, json!({"a": {"x": 1, "y": 20, "z": 30}, "b": [1, 2]}));
    }

    #[test]
    fn a_null_states_nothing() {
        let mut base = json!({"calibration": {"film_base": {"explicit": [0.6, 0.3, 0.2]}},
                              "look": {"contrast": 1.2}});
        let overlay = json!({"calibration": {"film_base": null},
                             "look": {"contrast": null, "channel_grade": [1.1, 1.0]},
                             "roll": null});
        merge_json(&mut base, &overlay);
        assert_eq!(
            base,
            json!({"calibration": {"film_base": {"explicit": [0.6, 0.3, 0.2]}},
                   "look": {"contrast": 1.2, "channel_grade": [1.1, 1.0]}})
        );
    }

    #[test]
    fn merge_json_merges_destination_axes_but_switches_the_output_variant() {
        // One stated axis each, different keys: the shape of a variant switch, but a
        // struct of optional fields — the frame's container joins the roll's transfer.
        let mut base = json!({"output": {"display": {"transfer": "pq"}}});
        let overlay = json!({"output": {"display": {"container": "tiff"}}});
        merge_json(&mut base, &overlay);
        assert_eq!(
            base,
            json!({"output": {"display": {"transfer": "pq", "container": "tiff"}}})
        );
        // The genuine enum level still switches: the film master replaces the display
        // arm, and a display arm replaces the film master.
        let mut base = json!({"output": {"display": {"transfer": "pq"}}});
        merge_json(&mut base, &json!({"output": "film-master"}));
        assert_eq!(base, json!({"output": "film-master"}));
        let overlay = json!({"output": {"display": {"gamut": "adobe-rgb"}}});
        merge_json(&mut base, &overlay);
        assert_eq!(base, overlay);
    }

    #[test]
    fn merge_json_replaces_enum_variant_switch_but_deep_merges_same_tag() {
        // A variant switch (`region` → `explicit`) must REPLACE the one-key map, not
        // union the tags — `{"region":…, "explicit":…}` deserializes as no variant.
        let mut base = json!({"film_base": {"region": [1, 2, 3, 4]}});
        merge_json(
            &mut base,
            &json!({"film_base": {"explicit": [0.9, 0.5, 0.4]}}),
        );
        assert_eq!(base, json!({"film_base": {"explicit": [0.9, 0.5, 0.4]}}));
        // The SAME tag on both sides is not a variant switch: recurse into it so a
        // partial override of one sub-field keeps its siblings.
        let mut base = json!({"curve": {"dmax": {"auto": {"p": 0.5, "q": 1}}}});
        merge_json(&mut base, &json!({"curve": {"dmax": {"auto": {"p": 0.9}}}}));
        assert_eq!(
            base,
            json!({"curve": {"dmax": {"auto": {"p": 0.9, "q": 1}}}})
        );
    }

    /// A bare tag over a newtype variant **replaces** it, so serde rejects the incomplete
    /// override. Every externally-tagged recipe variant is a newtype carrying a positional
    /// payload, where a bare tag states nothing *and there is nothing it could state*. A
    /// guard that once kept the base on a matching tag silently turned a malformed
    /// per-frame `{"film_base": "explicit"}` into an inherit at exit 0.
    #[test]
    fn a_bare_tag_overlay_replaces_a_newtype_variant() {
        for base in [
            json!({"explicit": [0.9, 0.55, 0.42]}), // FilmBaseSource / WhiteBalance
            json!({"region": [1, 2, 3, 4]}),        // FilmBaseSource::Region
        ] {
            let tag = base.as_object().unwrap().keys().next().unwrap().clone();
            let mut merged = base.clone();
            merge_json(&mut merged, &json!(tag.clone()));
            assert_eq!(merged, json!(tag), "{base}");
        }
    }

    #[test]
    fn a_frames_own_roll_white_keeps_the_rolls_gains() {
        // `roll --frames` merges over the *serialized* shared recipe, where an unset roll
        // value is a `null` key, so a one-key overlay is never read as an enum switch.
        let mut shared = Recipe::default();
        shared.roll.white_balance = Some([0.8, 1.0, 1.25]);
        let mut v = serde_json::to_value(&shared).unwrap();
        merge_json(&mut v, &json!({"roll": {"white_stops": 2.0}}));
        let frame: Recipe = serde_json::from_value(v).unwrap();
        assert_eq!(frame.roll.white_balance, Some([0.8, 1.0, 1.25]));
        assert_eq!(frame.roll.white_stops, Some(2.0));
    }

    #[test]
    fn two_partial_layers_of_one_section_both_survive() {
        // Merged onto each other, the second one-key `look` would read as a variant
        // switch and drop the first; `compose` merges both onto the full default.
        let a = json!({"recipe_version": 3, "look": {"contrast": 1.3}});
        let b = json!({"recipe_version": 3, "look": {"channel_grade": [1.1, 0.9]}});
        let r = compose([&a, &b]).unwrap();
        assert_eq!(r.look.contrast, 1.3);
        assert_eq!(r.look.channel_grade, [1.1, 0.9]);
    }

    #[test]
    fn a_later_layer_wins_and_a_null_keeps_the_measured_base() {
        let measured = json!({"recipe_version": 3,
            "calibration": {"film_base": {"explicit": [0.6, 0.3, 0.2]}},
            "roll": {"white_balance": [0.8, 1.0, 1.2], "white_stops": 2.5},
            "scene_correction": {"exposure": 0.5}});
        // `hanten params`: every key, the unset base and roll ones `null`.
        let mut look = serde_json::to_value(Recipe::default()).unwrap();
        look["scene_correction"]["exposure"] = json!(-0.25);
        let r = compose([&measured, &look]).unwrap();
        assert_eq!(
            r.calibration.film_base,
            Some(crate::types::FilmBaseSource::Explicit([0.6, 0.3, 0.2]))
        );
        assert_eq!(r.roll.white_balance, Some([0.8, 1.0, 1.2]));
        assert_eq!(r.roll.white_stops, Some(2.5));
        assert_eq!(r.scene_correction.exposure, -0.25, "the later layer wins");
    }

    #[test]
    fn two_layers_frame_tables_union() {
        // Two one-entry tables have the variant-switch shape; `roll.frames` is a map, so
        // they union, and a later layer's entry for the same file wins.
        let a = json!({"recipe_version": 3, "roll": {"frames": {"a.tif": {"white_stops": 2.0}}}});
        let b = json!({"recipe_version": 3, "roll": {"frames": {"b.tif": {"white_stops": 1.8}}}});
        let c = json!({"recipe_version": 3, "roll": {"frames": {"a.tif": {"white_stops": 1.6}}}});
        let r = compose([&a, &b, &c]).unwrap();
        let stops: Vec<(&str, f32)> = r
            .roll
            .frames
            .iter()
            .map(|(k, v)| (k.as_str(), v.white_stops.unwrap()))
            .collect();
        assert_eq!(stops, [("a.tif", 1.6), ("b.tif", 1.8)]);
    }

    #[test]
    fn one_files_clamp_and_lift_from_two_layers_merge() {
        // Each entry has one key, and the keys differ: the variant-switch shape, which
        // would drop the clamp. An entry is a struct, so its fields merge.
        let clamp =
            json!({"recipe_version": 3, "roll": {"frames": {"a.tif": {"white_stops": 2.0}}}});
        let lift = json!({"recipe_version": 3, "roll": {"frames": {"a.tif": {"exposure": 0.2}}}});
        let r = compose([&clamp, &lift]).unwrap();
        let entry = r.roll.frames["a.tif"];
        assert_eq!((entry.white_stops, entry.exposure), (Some(2.0), Some(0.2)));
    }

    #[test]
    fn a_later_layer_switches_the_film_base_variant() {
        let a =
            json!({"recipe_version": 3, "calibration": {"film_base": {"region": [1, 2, 3, 4]}}});
        let b = json!({"recipe_version": 3, "calibration": {"film_base": {"explicit": [0.6, 0.3, 0.2]}}});
        let r = compose([&a, &b]).unwrap();
        assert_eq!(
            r.calibration.film_base,
            Some(crate::types::FilmBaseSource::Explicit([0.6, 0.3, 0.2]))
        );
    }

    /// One layer resolves exactly as the file parsed alone: composition must not move
    /// a single-`--params` run. Every committed recipe is checked, plus the defaults.
    #[test]
    fn one_layer_composes_to_its_own_parse() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("scripts/real-scan-verify/recipes");
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if !name.ends_with(".json") || name.ends_with(".provenance.json") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let alone: Recipe = serde_json::from_str(&text).unwrap();
            let value: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(compose([&value]).unwrap(), alone, "{name}");
            checked += 1;
        }
        assert!(checked > 0, "no recipe found under {}", dir.display());
        let defaults = serde_json::to_value(Recipe::default()).unwrap();
        assert_eq!(compose([&defaults]).unwrap(), Recipe::default());
        assert_eq!(compose([]).unwrap(), Recipe::default());
    }
}

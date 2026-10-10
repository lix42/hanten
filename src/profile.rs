//! `hanten profile`: a look written as an annotated JSONC recipe, with no scan
//! (`docs/design/roll-workflow.md`, "Authored files").
//!
//! A profile writes every recipe key but [`OMITTED_SECTIONS`], a `null` where the
//! rendering decides, and is stamped like every recipe document Hanten writes. Its
//! comments are generated here from [`NOTES`], one per key, and are never read back.

use crate::cli::ConversionFlags;
use crate::recipe::Recipe;

/// The recipe sections a profile leaves out, since each belongs to one roll: its
/// measurements (`calibration`, `roll`), and the adjustment made on top of them
/// (`scene_correction`), which a recipe file would otherwise state as a value the roll's
/// warnings read as stale.
pub const OMITTED_SECTIONS: [&str; 3] = ["calibration", "roll", "scene_correction"];

/// How a key is written: a section opens an object whose keys are annotated in turn;
/// a value is written on one line, a tagged value (`{"explicit": […]}`) included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Section,
    Value,
}

/// One annotated recipe key: its dotted path, its flag (`None` for a section), and the
/// note written beside it. A flag's possible values are appended from clap.
#[derive(Clone, Copy, Debug)]
pub struct Note {
    pub path: &'static str,
    pub kind: Kind,
    pub flag: Option<&'static str>,
    pub note: &'static str,
}

const fn section(path: &'static str, note: &'static str) -> Note {
    Note {
        path,
        kind: Kind::Section,
        flag: None,
        note,
    }
}

const fn value(path: &'static str, flag: &'static str, note: &'static str) -> Note {
    Note {
        path,
        kind: Kind::Value,
        flag: Some(flag),
        note,
    }
}

/// Every key a profile writes. `every_written_key_has_a_note` holds it to the document.
pub const NOTES: &[Note] = &[
    Note {
        path: "recipe_version",
        kind: Kind::Value,
        flag: None,
        note: "the recipe layout this file is written in",
    },
    section("input", "how the scan's samples are read"),
    value(
        "input.transfer",
        "--input-transfer",
        "how samples are encoded",
    ),
    value("input.meaning", "--input-meaning", "what samples measure"),
    value(
        "input.film_type",
        "--film-type",
        "declared film chemistry; provenance only",
    ),
    section("measure", "where measurements are taken"),
    value(
        "measure.inset",
        "--measure-inset",
        "border inset, a fraction of the shorter edge",
    ),
    section("reconstruction", "the fixed decode: density to film RGB"),
    value(
        "reconstruction.scale",
        "--density-scale",
        "per-channel density gain R, G, B",
    ),
    value(
        "reconstruction.offset",
        "--density-offset",
        "per-channel density offset R, G, B",
    ),
    value(
        "reconstruction.linearization",
        "--density-gamma",
        "the decode's slope, > 0",
    ),
    value(
        "reconstruction.anchor",
        "--anchor-mid-offset",
        "mid-grey's density above the film base",
    ),
    value(
        "rendering",
        "--rendering",
        "the base every setting below builds on",
    ),
    section("look", "contrast and colour"),
    value(
        "look.contrast",
        "--contrast",
        "multiplier on the base slope, > 0; 1 keeps it",
    ),
    value(
        "look.saturation",
        "--saturation",
        "multiplier on the default colour, > 0; 1 keeps it",
    ),
    value(
        "look.channel_grade",
        "--channel-grade",
        "red and blue exponents R, B; 1, 1 is none",
    ),
    section(
        "look.highlight_desaturation",
        "pulls bright near-neutral pixels to neutral; null: the rendering's",
    ),
    value(
        "look.highlight_desaturation.strength",
        "--highlight-desaturation",
        "0 to 1; 0 is off",
    ),
    value(
        "look.highlight_desaturation.start_stops",
        "--highlight-desaturation-start",
        "stops below diffuse white, negative",
    ),
    value(
        "look.highlight_desaturation.band",
        "--highlight-desaturation-band",
        "saturation band S0, S1",
    ),
    section("fit_range", "the display tone; null: the rendering's"),
    value(
        "fit_range.headroom_stops",
        "--display-tone-headroom",
        "stops above reference white; 0 is the identity",
    ),
    value(
        "fit_range.display_black",
        "--display-black",
        "where the film base renders, in display stops below mid-grey, or \"off\"",
    ),
    section("fit_gamut", "no settings yet"),
    section(
        "output",
        "the destination, {\"display\": axes}, or \"film-master\" (--film-master)",
    ),
    section(
        "output.display",
        "a rendered destination; an unstated axis is derived from the stated ones",
    ),
    value("output.display.range", "--range", "dynamic range"),
    value(
        "output.display.transfer",
        "--transfer",
        "how samples are stored",
    ),
    value("output.display.gamut", "--gamut", "primaries"),
    value("output.display.container", "--container", "file container"),
];

fn note_for(path: &str) -> Option<&'static Note> {
    NOTES.iter().find(|n| n.path == path)
}

/// The flags that set an [`OMITTED_SECTIONS`] section, by the section they set: what
/// `profile` refuses and hides from its help. A flag missing here is still refused
/// ([`refusal`] compares the sections), only named less well.
pub const REFUSED_FLAGS: &[(&str, &str)] = &[
    ("calibration", "--film-base"),
    ("calibration", "--base-region"),
    ("roll", "--roll-white-balance"),
    ("roll", "--roll-white"),
    ("roll", "--roll-dark"),
    ("roll", "--roll-exposure"),
    ("roll", "--roll-frame-exposure"),
    ("roll", "--small-lift"),
    ("roll", "--roll-thin-slope"),
    ("roll", "--roll-thin-exposure"),
    ("roll", "--thin-lift"),
    ("roll", "--roll-midtone-line"),
    ("roll", "--midtone-neutral"),
    ("roll", "--neutral-balance"),
    ("scene_correction", "--white-balance"),
    ("scene_correction", "--exposure"),
];

/// Why `profile` refuses `knobs`, whose merge over the defaults is `merged`: a flag set
/// a section the profile leaves out. A listed flag is refused by presence, even at its
/// default (no recipe could have set it); the section comparison catches a flag the list
/// lacks.
pub fn refusal(knobs: &ConversionFlags, merged: &Recipe) -> Option<String> {
    let defaults = Recipe::default();
    let (section, flag) = match REFUSED_FLAGS.iter().find(|(_, flag)| stated(knobs, flag)) {
        Some(&(section, flag)) => (section, flag),
        None if merged.calibration != defaults.calibration => ("calibration", "a flag here"),
        None if merged.roll != defaults.roll => ("roll", "a flag here"),
        None if merged.scene_correction != defaults.scene_correction => {
            ("scene_correction", "a flag here")
        }
        None => return None,
    };
    let switch = ROLL_SWITCHES.iter().find(|(f, _)| *f == flag);
    let (what, remedy) = match (section, switch) {
        ("calibration", _) => (
            "the film base (`calibration`), which belongs to one roll".to_string(),
            "or write the roll's file with `hanten measure-base <unexposed-frame> --out \
             base.json` and layer it after the look (--params look.jsonc --params base.json)"
                .to_string(),
        ),
        ("roll", Some((_, key))) => (
            "a switch in the roll's section (`roll`)".to_string(),
            format!("or state `roll.{key}` in the roll's recipe file"),
        ),
        ("roll", None) => (
            "a roll value (`roll`), which belongs to one roll".to_string(),
            "or write the roll's file with `hanten measure-roll <frames> --out roll.json` and \
             layer it after the look (--params look.jsonc --params roll.json)"
                .to_string(),
        ),
        _ => (
            "an adjustment to one roll's measured white balance and exposure \
             (`scene_correction`)"
                .to_string(),
            "where it adjusts that roll's measurement".to_string(),
        ),
    };
    Some(format!(
        "hanten profile writes a look, and {flag} sets {what}: pass it to `convert` or \
         `roll`, {remedy}"
    ))
}

/// The film base `profile` probes a look's render at: thinner on every channel than any
/// base measured. The base's own rendered luminance does not depend on it, and the
/// densest sample decodes densest under the densest base, so a look that fails here
/// fails under every real one.
pub const PROBE_BASE: [f32; 3] = [0.02; 3];

/// What `profile` adds to a render probe's refusal.
pub const PROBE_NOTE: &str = "Probed at a film base thinner than any measured, so no \
     film base renders this look";

/// The `roll` switches among [`REFUSED_FLAGS`], with their keys: chosen rather than
/// measured, so `measure-roll` never writes them.
const ROLL_SWITCHES: &[(&str, &str)] = &[
    ("--small-lift", "small_lift"),
    ("--thin-lift", "thin_lift"),
    ("--midtone-neutral", "midtone_neutral"),
    ("--neutral-balance", "neutral_balance"),
];

/// Whether `flag`, one of [`REFUSED_FLAGS`], is on the command line.
fn stated(knobs: &ConversionFlags, flag: &str) -> bool {
    let (base, roll, scene) = (&knobs.film_base, &knobs.roll, &knobs.scene);
    match flag {
        "--film-base" => base.film_base.is_some(),
        "--base-region" => base.base_region.is_some(),
        "--roll-white-balance" => roll.roll_white_balance.is_some(),
        "--roll-white" => roll.roll_white.is_some(),
        "--roll-dark" => roll.roll_dark.is_some(),
        "--roll-exposure" => roll.roll_exposure.is_some(),
        "--roll-frame-exposure" => roll.roll_frame_exposure.is_some(),
        "--small-lift" => roll.small_lift.is_some(),
        "--roll-thin-slope" => roll.roll_thin_slope.is_some(),
        "--roll-thin-exposure" => roll.roll_thin_exposure.is_some(),
        "--thin-lift" => roll.thin_lift.is_some(),
        "--roll-midtone-line" => roll.roll_midtone_line.is_some(),
        "--midtone-neutral" => roll.midtone_neutral.is_some(),
        "--neutral-balance" => roll.neutral_balance.is_some(),
        "--white-balance" => scene.white_balance.is_some(),
        "--exposure" => scene.exposure.is_some(),
        _ => false,
    }
}

/// A removed flag's message under `profile`: one whose remedy names a flag `profile`
/// refuses gets where that flag goes.
pub fn removed_flag_message(message: String) -> String {
    if REFUSED_FLAGS.iter().any(|(_, flag)| {
        message.contains(&format!("{flag} ")) || message.contains(&format!("{flag}`"))
    }) {
        format!(
            "{}. Those flags go on `convert` or `roll`: a profile holds no film base, \
             roll values or white balance and exposure",
            message.trim_end_matches('.')
        )
    } else {
        message
    }
}

/// The comment heading a profile, one line per element.
const HEADER: &[&str] = &[
    "A Hanten look profile, written by `hanten profile`. It writes every setting but what",
    "belongs to one roll (`calibration`, `roll`, `scene_correction`); a null takes the",
    "rendering's value, which a later build may move. `meta` names the build that wrote it.",
    "Checked without a scan: what depends on your base or pixels (clipping, a render your",
    "base cannot hold) is checked when it converts. It states the decode, so layer it",
    "before a roll's measured recipe: --params look.jsonc --params roll.json",
    "`hanten profile` never overwrites it, and its comments are not read back: edit freely.",
];

/// `json` — a `{meta, params}` envelope as serde writes it — as annotated JSONC. Read
/// from serde's text rather than a `Value`, so key order and each number's spelling (an
/// `f32` as `1.2`, not `1.2000000476837158`) are serde's. `possible_values` gives a
/// flag's accepted values, appended to its note.
pub fn render(json: &str, possible_values: &dyn Fn(&str) -> Vec<String>) -> String {
    let doc = Parser {
        text: json.as_bytes(),
        at: 0,
    }
    .document();
    let mut out = String::new();
    for line in HEADER {
        out.push_str("// ");
        out.push_str(line);
        out.push('\n');
    }
    let writer = Writer { possible_values };
    match &doc {
        Node::Object(entries) => writer.object(&mut out, entries, None, 0, true, None),
        other => {
            out.push_str(&inline(other));
            out.push('\n');
        }
    }
    out
}

/// A JSON value with its scalars kept as serde spelled them and its keys in order.
#[derive(Debug)]
enum Node {
    Scalar(String),
    Array(Vec<Node>),
    Object(Vec<(String, Node)>),
}

/// A parser for the text serde just wrote, which is valid JSON by construction; the
/// round-trip test is what holds that.
struct Parser<'a> {
    text: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn document(mut self) -> Node {
        self.value()
    }

    fn skip_space(&mut self) {
        while self.text.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn value(&mut self) -> Node {
        self.skip_space();
        match self.text.get(self.at) {
            Some(b'{') => {
                self.at += 1;
                let mut entries = Vec::new();
                loop {
                    self.skip_space();
                    if self.eat(b'}') {
                        return Node::Object(entries);
                    }
                    let key = self.string();
                    self.skip_space();
                    self.eat(b':');
                    let value = self.value();
                    entries.push((key, value));
                    self.skip_space();
                    self.eat(b',');
                }
            }
            Some(b'[') => {
                self.at += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_space();
                    if self.eat(b']') {
                        return Node::Array(items);
                    }
                    items.push(self.value());
                    self.skip_space();
                    self.eat(b',');
                }
            }
            Some(b'"') => Node::Scalar(self.string()),
            _ => {
                let start = self.at;
                while self
                    .text
                    .get(self.at)
                    .is_some_and(|c| !matches!(c, b',' | b'}' | b']') && !c.is_ascii_whitespace())
                {
                    self.at += 1;
                }
                Node::Scalar(String::from_utf8_lossy(&self.text[start..self.at]).into_owned())
            }
        }
    }

    /// A string token, quotes and escapes kept.
    fn string(&mut self) -> String {
        let start = self.at;
        self.at += 1;
        while let Some(&c) = self.text.get(self.at) {
            self.at += 1;
            match c {
                b'\\' => self.at += 1,
                b'"' => break,
                _ => {}
            }
        }
        String::from_utf8_lossy(&self.text[start..self.at.min(self.text.len())]).into_owned()
    }

    fn eat(&mut self, c: u8) -> bool {
        let found = self.text.get(self.at) == Some(&c);
        if found {
            self.at += 1;
        }
        found
    }
}

struct Writer<'a> {
    possible_values: &'a dyn Fn(&str) -> Vec<String>,
}

impl Writer<'_> {
    /// An object over several lines. `path` is its recipe path: `None` outside `params`,
    /// where nothing is annotated, and `Some("")` for `params` itself.
    fn object(
        &self,
        out: &mut String,
        entries: &[(String, Node)],
        path: Option<&str>,
        depth: usize,
        last: bool,
        note: Option<String>,
    ) {
        if entries.is_empty() {
            out.push_str("{}");
            end_line(out, last, note);
            return;
        }
        out.push('{');
        if let Some(note) = note {
            out.push_str("  // ");
            out.push_str(&note);
        }
        out.push('\n');
        let indent = "  ".repeat(depth + 1);
        let entries: Vec<_> = entries
            .iter()
            .filter(|(key, _)| {
                path != Some("") || !OMITTED_SECTIONS.contains(&key.trim_matches('"'))
            })
            .collect();
        for (i, (key, child)) in entries.iter().enumerate() {
            let child_last = i + 1 == entries.len();
            let name = key.trim_matches('"');
            let child_path = match path {
                None if depth == 0 && name == "params" => Some(String::new()),
                None => None,
                Some("") => Some(name.to_string()),
                Some(parent) => Some(format!("{parent}.{name}")),
            };
            out.push_str(&indent);
            out.push_str(key);
            out.push_str(": ");
            let found = child_path.as_deref().and_then(note_for);
            let text = found.map(|n| self.comment(n));
            match child {
                Node::Object(inner) if found.is_none_or(|n| n.kind == Kind::Section) => self
                    .object(
                        out,
                        inner,
                        child_path.as_deref(),
                        depth + 1,
                        child_last,
                        text,
                    ),
                _ => {
                    out.push_str(&inline(child));
                    end_line(out, child_last, text);
                }
            }
        }
        out.push_str(&"  ".repeat(depth));
        out.push('}');
        // The note went on the opening line.
        end_line(out, last, None);
    }

    fn comment(&self, n: &Note) -> String {
        let mut text = n.note.to_string();
        if let Some(flag) = n.flag {
            text.push_str("; ");
            text.push_str(flag);
            let values = (self.possible_values)(flag);
            if !values.is_empty() {
                text.push_str(&format!(" ({})", values.join(" | ")));
            }
        }
        text
    }
}

fn end_line(out: &mut String, last: bool, note: Option<String>) {
    if !last {
        out.push(',');
    }
    if let Some(note) = note {
        out.push_str("  // ");
        out.push_str(&note);
    }
    out.push('\n');
}

/// `node` on one line, with a space after each `,` and `:`.
fn inline(node: &Node) -> String {
    match node {
        Node::Scalar(s) => s.clone(),
        Node::Array(items) => format!(
            "[{}]",
            items.iter().map(inline).collect::<Vec<_>>().join(", ")
        ),
        Node::Object(entries) => format!(
            "{{{}}}",
            entries
                .iter()
                .map(|(k, v)| format!("{k}: {}", inline(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::Recipe;
    use serde_json::{Value, json};

    fn none(_: &str) -> Vec<String> {
        Vec::new()
    }

    fn reparsed(text: &str) -> Value {
        serde_json::from_str(&crate::jsonc::strip_comments(text).unwrap()).unwrap()
    }

    #[test]
    fn the_rendered_file_parses_back_to_the_document() {
        let json = r#"{"meta":{"pipeline_version":10},"params":{"recipe_version":3,
            "look":{"contrast":1.2,"channel_grade":[1.0,1.0],
            "highlight_desaturation":{"strength":null}},
            "reconstruction":{"anchor":{"mid-at-base-offset":0.62}},
            "fit_gamut":{},"output":{"display":{"range":"hdr"}},
            "odd":"a \"quoted\", {tricky} // string"}}"#;
        let text = render(json, &none);
        assert_eq!(
            reparsed(&text),
            serde_json::from_str::<Value>(json).unwrap()
        );
        assert!(
            text.contains("\"contrast\": 1.2,  // multiplier on the base slope"),
            "{text}"
        );
        assert!(
            text.contains("\"look\": {  // contrast and colour"),
            "{text}"
        );
        assert!(text.starts_with("// A Hanten look profile"), "{text}");
        // Order is serde's, not sorted.
        assert!(text.find("\"look\"") < text.find("\"fit_gamut\""), "{text}");
        // `meta` is not annotated.
        assert!(text.contains("\"pipeline_version\": 10\n"), "{text}");
    }

    #[test]
    fn a_film_master_output_is_written_as_its_string() {
        let json = json!({"params": {"recipe_version": 3, "output": "film-master"}}).to_string();
        let text = render(&json, &none);
        assert!(
            text.contains("\"output\": \"film-master\",  // the destination"),
            "{text}"
        );
    }

    #[test]
    fn possible_values_follow_the_flag() {
        let json = json!({"params": {"output": {"display": {"range": "sdr"}}}}).to_string();
        let text = render(&json, &|flag| {
            if flag == "--range" {
                vec!["sdr".into(), "hdr".into()]
            } else {
                Vec::new()
            }
        });
        assert!(
            text.contains("dynamic range; --range (sdr | hdr)"),
            "{text}"
        );
    }

    /// The paths `params` holds that a profile writes, descending through sections.
    fn written_paths(v: &Value, path: &str, out: &mut Vec<String>) {
        for (key, child) in v.as_object().unwrap() {
            if path.is_empty() && OMITTED_SECTIONS.contains(&key.as_str()) {
                continue;
            }
            let p = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            if note_for(&p).is_some_and(|n| n.kind == Kind::Section) && child.is_object() {
                written_paths(child, &p, out);
            }
            out.push(p);
        }
    }

    /// Recipes that between them write every key: the default, every destination axis
    /// stated, and the film master.
    fn covering_recipes() -> Vec<Value> {
        let axes = json!({"recipe_version": 3, "output": {"display": {
            "range": "hdr", "transfer": "linear", "gamut": "bt2020", "container": "tiff"}}});
        let master = json!({"recipe_version": 3, "output": "film-master"});
        [
            Recipe::default(),
            serde_json::from_value(axes).unwrap(),
            serde_json::from_value(master).unwrap(),
        ]
        .iter()
        .map(|r| serde_json::to_value(r).unwrap())
        .collect()
    }

    #[test]
    fn every_written_key_has_a_note_and_every_note_a_key() {
        let mut written = Vec::new();
        for recipe in covering_recipes() {
            written_paths(&recipe, "", &mut written);
        }
        for path in &written {
            assert!(note_for(path).is_some(), "`{path}` is written with no note");
        }
        for n in NOTES {
            assert!(
                written.iter().any(|p| p == n.path),
                "note `{}` matches no key",
                n.path
            );
        }
    }

    /// Every visible flag in the groups that set an omitted section is listed, so a new
    /// one is named in its refusal and hidden from `profile --help`. Clap names each
    /// flattened group after its struct.
    #[test]
    fn every_flag_that_sets_an_omitted_section_is_listed() {
        use clap::CommandFactory;
        let cli = crate::cli::Cli::command();
        let convert = cli.find_subcommand("convert").unwrap();
        let mut seen = 0;
        for group in [
            "FilmBaseOverrides",
            "RollOverrides",
            "SceneCorrectionOverrides",
        ] {
            let group = convert
                .get_groups()
                .find(|g| g.get_id() == group)
                .unwrap_or_else(|| panic!("no group {group}"));
            for id in group.get_args() {
                let arg = convert.get_arguments().find(|a| a.get_id() == id).unwrap();
                if arg.is_hide_set() {
                    continue; // a removed flag, refused as removed
                }
                let flag = format!("--{}", arg.get_long().unwrap());
                assert!(
                    REFUSED_FLAGS.iter().any(|(_, f)| *f == flag),
                    "{flag} is missing from REFUSED_FLAGS"
                );
                seen += 1;
            }
        }
        assert_eq!(
            seen,
            REFUSED_FLAGS.len(),
            "REFUSED_FLAGS lists a flag no group has"
        );
    }

    #[test]
    fn every_refused_flag_is_hidden_from_profiles_help() {
        use clap::CommandFactory;
        let cli = crate::cli::Cli::command();
        let profile = cli.find_subcommand("profile").unwrap();
        for (_, flag) in REFUSED_FLAGS {
            let long = flag.trim_start_matches("--");
            let arg = profile.get_arguments().find(|a| a.get_long() == Some(long));
            assert!(arg.is_some_and(|a| a.is_hide_set()), "{flag}");
        }
    }

    #[test]
    fn every_noted_flag_is_a_profile_flag() {
        use clap::CommandFactory;
        let cli = crate::cli::Cli::command();
        let profile = cli.find_subcommand("profile").unwrap();
        for flag in NOTES.iter().filter_map(|n| n.flag) {
            let long = flag.trim_start_matches("--");
            assert!(
                profile
                    .get_arguments()
                    .any(|a| a.get_long() == Some(long) && !a.is_hide_set()),
                "{flag}"
            );
        }
    }

    #[test]
    fn the_omitted_sections_are_left_out() {
        let json = json!({"params": {"recipe_version": 3,
            "calibration": {"film_base": null}, "roll": {"exposure": null},
            "scene_correction": {"exposure": 0.0}}})
        .to_string();
        let back = reparsed(&render(&json, &none));
        assert_eq!(back, json!({"params": {"recipe_version": 3}}));
    }

    fn knobs(flags: &[&str]) -> ConversionFlags {
        use clap::Parser;
        let argv = ["hanten", "profile"].iter().chain(flags).copied();
        match crate::cli::Cli::try_parse_from(argv).unwrap().command {
            crate::cli::Command::Profile(a) => a.knobs,
            _ => unreachable!(),
        }
    }

    /// The value each refused flag is given here, a legal one.
    fn sample(flag: &str) -> &'static str {
        match flag {
            "--base-region" => "0,0,10,10",
            "--roll-midtone-line" => "1,0,1,0,-1,1,2",
            "--small-lift" | "--thin-lift" | "--midtone-neutral" | "--neutral-balance" => "off",
            "--film-base" | "--roll-white-balance" | "--white-balance" => "0.9,0.5,0.4",
            _ => "0.5",
        }
    }

    #[test]
    fn every_refused_flag_is_refused_by_name() {
        for (section, flag) in REFUSED_FLAGS {
            let k = knobs(&[flag, sample(flag)]);
            let merged = crate::recipe::merge(Recipe::default(), &k);
            let message = refusal(&k, &merged).unwrap_or_else(|| panic!("{flag} accepted"));
            assert!(message.contains(&format!("{flag} sets")), "{message}");
            assert!(message.contains(&format!("(`{section}`)")), "{message}");
        }
        // Even at the default value: no recipe could have set it.
        let k = knobs(&["--exposure", "0"]);
        let message = refusal(&k, &crate::recipe::merge(Recipe::default(), &k)).unwrap();
        assert!(message.contains("--exposure sets"), "{message}");
        // A switch's remedy is its key, which `measure-roll` never writes.
        let k = knobs(&["--neutral-balance", "off"]);
        let message = refusal(&k, &crate::recipe::merge(Recipe::default(), &k)).unwrap();
        assert!(message.contains("`roll.neutral_balance`"), "{message}");
        assert!(!message.contains("measure-roll"), "{message}");
        // A look flag is not refused.
        let k = knobs(&["--contrast", "1.2", "--rendering", "direct"]);
        assert!(refusal(&k, &crate::recipe::merge(Recipe::default(), &k)).is_none());
    }

    #[test]
    fn a_flag_missing_from_the_list_is_still_refused() {
        // The sections decide; the list only names the flag.
        let k = ConversionFlags::default();
        let mut merged = Recipe::default();
        merged.roll.exposure = Some(0.5);
        let message = refusal(&k, &merged).unwrap();
        assert!(
            message.contains("a flag here sets a roll value"),
            "{message}"
        );
    }

    #[test]
    fn a_removed_flags_remedy_naming_a_refused_flag_is_placed() {
        let placed = removed_flag_message("pass `--roll-white-balance R,G,B`.".into());
        assert!(placed.ends_with("white balance and exposure"), "{placed}");
        assert!(!placed.contains(".."), "{placed}");
        let untouched = removed_flag_message("use --display-black instead".into());
        assert_eq!(untouched, "use --display-black instead");
    }

    #[test]
    fn every_note_is_unique() {
        for (i, n) in NOTES.iter().enumerate() {
            assert!(
                NOTES[i + 1..].iter().all(|m| m.path != n.path),
                "{} twice",
                n.path
            );
        }
    }
}

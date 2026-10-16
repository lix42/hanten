//! How `roll.frames` keys name their frames.
//!
//! A recipe file keys each frame by its path **relative to the recipe's own folder**
//! (absolute only where no relative path exists, as across Windows drives), so a roll
//! folder holding its scans and recipe still converts after it moves. A bare file name
//! is the frame beside the recipe; a recipe on stdin or a pipe is read from the working
//! directory. A written relative key separates components with `/` on every platform;
//! an absolute one keeps the platform's form.
//!
//! In memory every key is the frame's resolved path: `cli` rewrites a layer's keys when
//! it loads ([`rebase`]), matches a frame by its own resolved path ([`super::for_frame`]),
//! and writes keys relative to the file it writes ([`relative`]). So the report's
//! `recipe` and its `params_hash` carry absolute paths: they change when the roll moves,
//! though the image does not. Keys are written by real path: a symlink is followed.
//!
//! Where `cli` resolves the table, a key naming no file warns, rather than silently
//! reaching no frame; a key whose file exists but is not among the run's inputs stays
//! silent, since rendering a subset with the whole roll's numbers is a designed use.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

/// Rewrite each `roll.frames` key in a recipe body to `resolve(key)`. Two keys resolving
/// to one frame are an error naming both; so is any error `resolve` returns.
pub fn rebase(
    body: &mut Value,
    mut resolve: impl FnMut(&str) -> Result<String, String>,
) -> Result<(), String> {
    let Some(Value::Object(frames)) = body.pointer_mut("/roll/frames") else {
        return Ok(());
    };
    let mut rebased = serde_json::Map::new();
    let mut stated: BTreeMap<String, String> = BTreeMap::new();
    for (key, entry) in std::mem::take(frames) {
        let path = resolve(&key)?;
        if let Some(first) = stated.get(&path) {
            return Err(format!(
                "`roll.frames` keys {first:?} and {key:?} both name {path}: merge them into \
                 one entry"
            ));
        }
        stated.insert(path.clone(), key);
        rebased.insert(path, entry);
    }
    *frames = rebased;
    Ok(())
}

/// `frames` with each key, a resolved path, made relative to `dir` (itself resolved) —
/// the keys a recipe written into `dir` states, `/`-separated, and `.` for `dir` itself
/// (an empty key is refused). A key with no relative path stays as it is, in the
/// platform's form.
pub fn relative<T: Clone>(frames: &BTreeMap<String, T>, dir: &Path) -> BTreeMap<String, T> {
    frames
        .iter()
        .map(|(key, entry)| {
            let key = relative_path(dir, Path::new(key))
                .and_then(|p| {
                    let parts: Option<Vec<&str>> =
                        p.components().map(|c| c.as_os_str().to_str()).collect();
                    parts.map(|parts| parts.join("/"))
                })
                .map(|k| if k.is_empty() { ".".to_owned() } else { k })
                .unwrap_or_else(|| key.clone());
            (key, entry.clone())
        })
        .collect()
}

/// The path from directory `dir` to `to`, both absolute and free of `.` and `..`; `None`
/// when either is relative or they share no root.
fn relative_path(dir: &Path, to: &Path) -> Option<PathBuf> {
    if !dir.is_absolute() || !to.is_absolute() {
        return None;
    }
    let dir: Vec<Component> = dir.components().collect();
    let to: Vec<Component> = to.components().collect();
    if dir.first() != to.first() {
        return None;
    }
    let common = dir.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut out = PathBuf::new();
    for _ in common..dir.len() {
        out.push("..");
    }
    for c in &to[common..] {
        out.push(c.as_os_str());
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_path_relative_to_a_folder_climbs_to_the_common_parent() {
        let rel = |d: &str, t: &str| relative_path(Path::new(d), Path::new(t));
        assert_eq!(rel("/r", "/r/a.tif"), Some("a.tif".into()));
        assert_eq!(rel("/r", "/r/sub/a.tif"), Some("sub/a.tif".into()));
        assert_eq!(
            rel("/r/recipes", "/r/scans/a.tif"),
            Some("../scans/a.tif".into())
        );
        assert_eq!(rel("/", "/a.tif"), Some("a.tif".into()));
        assert_eq!(rel("r", "/r/a.tif"), None);
    }

    #[test]
    fn written_keys_are_relative_to_the_folder_written_into() {
        let frames = BTreeMap::from([
            ("/roll/a/01.tif".to_string(), 1),
            ("/roll/b/01.tif".to_string(), 2),
        ]);
        let written = relative(&frames, Path::new("/roll"));
        assert_eq!(
            written,
            BTreeMap::from([("a/01.tif".to_string(), 1), ("b/01.tif".to_string(), 2)])
        );
        // `/` between components, climbing included.
        let written = relative(&frames, Path::new("/roll/c/d"));
        assert_eq!(
            written.keys().collect::<Vec<_>>(),
            ["../../a/01.tif", "../../b/01.tif"]
        );
        #[cfg(windows)]
        assert_eq!(
            relative(
                &BTreeMap::from([(r"C:\roll\a\01.tif".to_string(), 1)]),
                Path::new(r"C:\roll")
            ),
            BTreeMap::from([("a/01.tif".to_string(), 1)])
        );
    }

    #[test]
    fn a_key_naming_the_folder_written_into_is_written_as_a_dot() {
        let frames = BTreeMap::from([("/roll".to_string(), 1)]);
        assert_eq!(
            relative(&frames, Path::new("/roll")),
            BTreeMap::from([(".".to_string(), 1)])
        );
    }

    #[test]
    fn rebase_rewrites_every_key_and_refuses_two_naming_one_frame() {
        let mut body = json!({"roll": {"frames": {"a.tif": {"frame_exposure": 1.0}}}});
        rebase(&mut body, |k| Ok(format!("/r/{k}"))).unwrap();
        assert_eq!(
            body,
            json!({"roll": {"frames": {"/r/a.tif": {"frame_exposure": 1.0}}}})
        );

        let mut body = json!({"roll": {"frames": {"a.tif": {}, "./a.tif": {}}}});
        let err = rebase(&mut body, |k| {
            Ok(format!("/r/{}", k.trim_start_matches("./")))
        })
        .unwrap_err();
        assert!(
            err.contains("\"./a.tif\" and \"a.tif\" both name /r/a.tif"),
            "{err}"
        );

        let mut none = json!({"look": {}});
        rebase(&mut none, |_| Err("unreached".into())).unwrap();
    }
}

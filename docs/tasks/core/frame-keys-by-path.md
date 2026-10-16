---
title: Key roll frames by relative path
status: done
deps: []
summary: roll.frames is keyed by file name; a recipe applied to a same-named file elsewhere shares its entry
done: 2026-10-16
---
# Key roll frames by relative path

## Goal

`roll.frames` entries are keyed by file name (prototype spike unknown 9). Key them by
path relative to the roll's project or recipe file, absolute when no relative path exists
([desktop app design](../../design/desktop-app.md) §3.4).

## What is known

- Keys are paths relative to the recipe file's folder (a recipe on stdin or a pipe:
  the working directory), absolute where no relative path exists; written keys are
  relative to the file written, `/`-separated, by real path (`recipe::frame_keys`).
- Bare-name keys still work: a bare name is the frame beside the recipe. A key naming
  no file warns (`--strict` promotes it); two keys naming one file exit 2.
- Same-named inputs in different folders are accepted by `measure-roll`; `roll`
  still refuses pictures that would share an output file.
- The report's `recipe` and `params_hash` carry absolute keys, so they change when a
  roll moves; the image does not.

## Open questions

1. How a split file's two pictures are keyed — left to `core/half-frame-split`.
2. Whether the app needs this at all (design §3.2, §3.5) — the CLI fix stands either way.

## How to Verify

- Two frames with one file name in different folders get separate entries and render
  with their own values.
- A roll moved to another folder converts unchanged.

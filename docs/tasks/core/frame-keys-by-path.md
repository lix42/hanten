---
title: Key roll frames by relative path
status: todo
deps: []
summary: roll.frames is keyed by file name; a recipe applied to a same-named file elsewhere shares its entry
---
# Key roll frames by relative path

## Goal

`roll.frames` entries are keyed by file name (prototype spike unknown 9). Key them by
path relative to the roll's project or recipe file, absolute when no relative path exists
([desktop app design](../../design/desktop-app.md) §3.4).

## What is known

- `Recipe::for_frame` looks entries up by file name; a path key is refused today
  ("Keys are file names, not paths", exit 2).
- `measure-roll --out` and `roll --measure-roll` refuse two inputs with one file name
  (exit 2, `refuse_shared_file_names`). An entry is shared silently only when a recipe
  from one folder is applied to a same-named file in another.
- The app moves a roll's folder around, so relative paths keep working after a move.

## Open questions

1. Relative to what for the CLI: the recipe file, the working directory, or a stated root?
2. Whether bare-name keys stay accepted (versions are reset before release, so old
   recipes need not be).
3. How a split file's two pictures are keyed (`core/half-frame-split`).
4. Whether the app needs this at all: the core measures in-process and resolves each
   frame's recipe itself (design §3.2, §3.5), so it may be a CLI usability fix rather
   than an app prerequisite.

## How to Verify

- Two frames with one file name in different folders get separate entries and render
  with their own values.
- A roll moved to another folder converts unchanged.

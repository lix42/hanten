---
title: Key roll frames by relative path
status: todo
deps: []
summary: roll.frames is keyed by file name, so two frames with one name share an entry
---
# Key roll frames by relative path

## Goal

`roll.frames` entries are keyed by file name, so two frames with the same name from
different folders share one entry (prototype spike unknown 9). Key them by path relative
to the roll's project or recipe file, absolute when no relative path exists
([desktop app design](../../design/desktop-app.md) §3.4).

## What is known

- `Recipe::for_frame` looks entries up by file name; a path key is refused today
  ("Keys are file names, not paths", exit 2).
- The app moves a roll's folder around, so relative paths keep working after a move.

## Open questions

1. Relative to what for the CLI: the recipe file, the working directory, or a stated root?
2. Whether bare-name keys stay accepted (versions are reset before release, so old
   recipes need not be).
3. How a split file's two pictures are keyed (`core/half-frame-split`).

## How to Verify

- Two frames with one file name in different folders get separate entries and render
  with their own values.
- A roll moved to another folder converts unchanged.

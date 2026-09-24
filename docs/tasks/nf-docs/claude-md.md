# Update CLAUDE.md for the new architecture

## Goal

Rewrite the parts of CLAUDE.md that describe the old chain — the architecture
map and the HDR framing — and retire the migration rule itself once the migration
is done.

## Design

- **The architecture map is a preset-dispatch diagram.** The new one is a stage
  sequence with destinations at the end. That is the largest single edit, and it
  is also the section most often read first by someone picking up a task.
- **The migration rule retires with the migration**, but not everything in it
  does: the tag and the recipe for building it outlive the migration and move
  into the verification notes rather than being deleted.
- **Keep the layout.** Since 2026-09-24 CLAUDE.md holds only cross-cutting rules;
  subsystem traps live in the `//!` docs of their modules, indexed by its "Where
  the detail lives" table. A retired module's traps retire with it, and a new
  stage's go in its own module docs, not here — this task edits the Architecture
  section, the table, and the migration rule.
- **Update this file before the agent primers.** `.claude/agents/nc-reviewer` and
  `nc-fixer` carry summaries of these rules and are told CLAUDE.md wins on
  conflict; they are a lossy cache, not a second source.
- Can run at any point, but each pass should follow the code rather than lead
  it — a CLAUDE.md describing a chain that does not exist yet is worse than one
  describing the old.

## How to Verify

- No paragraph describes a preset, knob or module the binary no longer has, and
  every backticked path and task id still resolves.
- The claims changed were checked by grepping for their *negation*, since a stale
  sentence is never in the diff.
- The agent primers agree with the rewritten sections, and every "Where the detail
  lives" row names a module that exists.

## Dependencies

None — can run at any point.

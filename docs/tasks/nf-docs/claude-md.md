# Update CLAUDE.md for the new architecture

## Goal

Rewrite the parts of CLAUDE.md that describe the old chain — the architecture
map and the HDR framing — and retire the migration rule itself once the migration
is done.

## Design

- **The architecture map is a preset-dispatch diagram.** The new one is a stage
  sequence with destinations at the end. That is the largest single edit, and it
  is also the section most often read first by someone picking up a task.
- **The HDR framing is not merely dated, it becomes wrong.** CLAUDE.md calls the
  inert default gain map "a rendering-intent option, not a correctness gap"; the
  design calls its cause — a reconstruction whose shoulder pins diffuse white at
  reference white — a stage-boundary violation. When the shoulder goes, the
  paragraph is asserting something the code no longer does.
- **The migration rule retires with the migration**, but not everything in it
  does: the tag and the recipe for building it outlive the migration and move
  into the verification notes rather than being deleted.
- **Keep the traps.** Most of the file is hard-won gotchas that survive untouched
  — colorimetry, the memory model, the gates, the editing traps. Only the
  paragraphs describing the old chain go; resist the urge to tidy the rest.
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
- The agent primers agree with the rewritten sections.

## Dependencies

None — can run at any point.

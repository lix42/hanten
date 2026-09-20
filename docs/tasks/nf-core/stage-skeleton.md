# The new stage module tree

## Goal

Create the modules and typed boundaries for the rendering chain — scene correction →
look → fit range → fit gamut — with **no behaviour in them yet**. The point is that
every later epic has a named place to land in, and that the stage boundaries are
decided once, deliberately, rather than falling out of whichever stage ships first.

## Design

- **Written fresh.** Per CLAUDE.md's migration rule: do not shape these modules around
  the old code's seams, types or fusions, and do not "extract" a stage out of a
  per-pixel body because that is where the arithmetic lives today. The existing seam
  is worth *reading* — `render_split::display_source` (`src/pipeline/render_split.rs`)
  is today's single producer of the shared display source, so it enumerates what the
  stages must eventually do — but it is context, not a source.
- **One pure function per stage**, `(input, params) -> output`, with the orchestrator
  composing them. Each stage gets its own module; nothing reaches across.
- **The boundary is carried by a type, not by a comment.** `AcesCgImage` is the
  pattern that already works here: private fields plus a module-private constructor,
  so exactly one function can mint one and the wrong buffer cannot enter the chain.
  Decide how far to take that — a distinct type per stage output is the strong form
  and costs conversions; one working-space type with a phase marker is the weak one.
- **Stages are identity until their epic fills them.** An identity stage must be a
  real stage (it appears in the chain, in the report, and in the recipe) rather than
  an absent one, or the later epic re-litigates where it goes.

## Open questions

- **Where the tree lives.** A sibling `src/pipeline/nf/` keeps the two flows visibly
  separate and makes retirement a directory delete; putting the new stages beside the
  old ones avoids a rename later. `nf-retire` runs early in this plan, which argues
  for the second.
- **Recipe sections now or per stage?** Creating each stage's recipe section here
  fixes the §9 shape before any stage knows its knobs; deferring means four small
  schema changes. The `print.*` prefix rename is `nf-retire`'s, not this task's.
- **What an identity stage's params are** — an empty struct, or `Option<T>`? This
  decides whether "the look stage is off" is expressible before the look stage exists.

## How to Verify

- The chain compiles and composes under `--new-flow` with every stage an identity
  pass; `cargo clippy --all-targets -- -D warnings` is clean.
- A test pins the stage order and the fact that each boundary type can be minted only
  by its own stage (a compile-fail or a visibility assertion, whichever is cheaper).
- No new crate-level or broad `#[allow(dead_code)]`; any item-level allow names the
  epic that will consume it, per the house rule.

## Dependencies

- [The `--new-flow` selector](new-flow-flag.md)
- [Spike: can a look-stage operator reproduce the knee'd sigmoid's whites?](../nf-look/path-to-white-spike.md)
  — the spike runs first, so no structural work starts before its answer

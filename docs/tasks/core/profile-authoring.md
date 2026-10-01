# Author a reusable pipeline profile without an image

**Design:** [the roll workflow](../../design/roll-workflow.md) — the single source for this task's CLI shape; where they disagree, the doc wins.

## Goal

Rename `hanten params` to **`hanten profile`** and make it author a reusable look: accept
the same override flags `convert` takes, validate them, and write an annotated,
hand-editable file — with no scan involved. (`--preset` and `--film-stock` retired
with the `characteristic` curve, `nf-retire/characteristic`: a named look is now a
profile file, which is what this task authors.)

Whether `--dump-params` goes is the design doc's open question 5: no sidecar has been
written since `nf-core/default-flip`, so it is the only recipe a `convert` leaves.

## Why `--dump-params` is not the answer

Measured 2026-08-11 (the sidecar point no longer holds — none is written since
`nf-core/default-flip`):

- Its output was **byte-identical to the sidecar** every conversion then wrote.
- It carries **nothing the image produced**: the same flags over two *different*
  scans emit identical files. It records the *modes* (`"auto"`, `"percentile"`,
  `{"region": …}`), never the resolved measurements the report holds alongside it.
- So "freeze a recipe" ran a full decode → render → encode to emit an echo of
  flags the user had just typed, and the result still re-measures per frame.

Meanwhile `hanten params` prints only defaults and accepts **no flags at all**, so
composing a real recipe means splicing the measured fragments in by hand.

## What is known

- `validate` is already a pure function of the resolved config, shared verbatim by
  `convert` and `roll`. It can run here unchanged.
- It cannot cover everything: clipping, region uniformity and grid disagreement
  need pixels. Config-only validation is the honest scope, and the output should
  not imply more.
- `validate_convert` adds a flag-*presence* rule on top; whether a profile should
  be held to it depends on whether a profile is expected to be complete.

## Open questions

1. **Annotated output means JSONC** (the design doc's "Authored files"). Comments are *generated
   from the schema*, never preserved — serde round-trips discard them. So what
   generates them, and how much do they say? Allowed values and units are clearly
   worth it; restating the whole parameter reference is not.
2. **`--out` and an existing file.** Since a rewrite would destroy a user's
   comments and edits, refusing unless forced is the safe default. Confirm, and
   decide what "forced" looks like.
3. **Should a profile be complete or partial?** A complete document is durable
   against default changes — defaults moved three times already
   (`pipeline_version` 1 → 3). A partial one is readable and composes better.
   Possibly both, behind a flag.
4. **Does a profile validate as a profile, or as a whole recipe?** A profile
   legitimately has no `calibration` section, so whole-recipe validation would
   reject every one of them.
5. **Is a profile stamped?** Design-spec §8's replay contract makes every recipe
   document hanten writes a stamped `{meta, params}` envelope, so a stamped
   hand-edited look would warn after every `pipeline_version` bump.

## How to Verify

- `hanten profile <overrides> --out look.jsonc` writes a file with no scan present,
  and that file is accepted by `--params` unchanged.
- `hanten profile <look flags> --out look.jsonc` writes the same values
  `convert --dump-params` writes today in its `params`.
- The emitted comments survive a round trip *as comments in the file*, and the
  file still parses — the JSONC-is-a-superset claim, tested rather than assumed.
- A contradictory override set (e.g. a sigmoid flag with an exponential curve) is
  reported here, not deferred to apply time.
- `--out` refuses to clobber an existing file.
- If the design doc's open question 5 retires `--dump-params`, it is gone from
  `convert` and no path silently accepts it.
- `hanten params` is gone; nothing in docs, tests or scripts still invokes it.

## Dependencies

- [Layered recipe composition](recipe-composition.md) — shares the merge semantics
  this command must reproduce exactly
- [CLI framework](cli-framework.md)
- [The `calibration` recipe section](calibration-recipe-section.md) — a profile is
  "a recipe with no `calibration` section", which needs the section to exist

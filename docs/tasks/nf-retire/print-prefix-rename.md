# Rename the `print.*` prefix

## Goal

Retire the `print.*` recipe prefix, a leftover from the legacy print path, and
move each knob under the stage that now owns it.

## Design

- **Its members belong to different stages now.** White balance, exposure and the
  flare half of the black point are scene correction; `linear_range` is an affine
  levels remap that needs a name and a home; the display-tone knobs are fit
  range. One prefix cannot describe that, and `print` names a stage that no
  longer exists.
- **Wait until there is no second implementation.** While legacy and the new
  chain both read `print.*`, a rename either renames twice or renames one
  meaning — and the two *are* different meanings, since legacy runs the controls
  on film RGB before the 3×3.
- **Recipe shape mirrors design-spec §9, and every recipe struct uses
  `deny_unknown_fields`**, so a key that moves in the code but not in §9 silently
  rejects docs-shaped recipes. The structs, §9 and the guide move together.
- **`--auto-wb` has no destination.** The new chain retired the per-frame estimate
  (`nf-scene-correction/roll-white-balance`) and measures white balance once per roll
  with `hanten measure-roll`, so `print.white_balance`'s `"gray-world"` /
  `"percentile"` retire here rather than move; only the explicit gains carry over.
- **Old spellings get a migration error, no aliases** — nc is unreleased, so this
  is cheap, and an alias would keep the legacy meaning readable forever.
- One thing to carry into the new name: `print_exposure` is a scalar gain applied
  *after* the curve, which is why it is incompatible with an unbounded operator
  today. The renamed knob should sit where that is no longer a surprise, or the
  incompatibility should follow it in prose.

## How to Verify

- A recipe using the old prefix fails with the new path named in the message.
- `--dump-params` writes the new shape, and a round-trip of its output is
  accepted.
- §9, the structs and `docs/using-nc.md` agree — verified by running the binary,
  not by reading the diff.

## Dependencies

- [Retire `legacy` and `custom`](legacy-custom.md)
- [The scene correction stage](../nf-scene-correction/stage.md)

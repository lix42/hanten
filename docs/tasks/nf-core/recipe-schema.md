# The recipe schema across the flow boundary

## Goal

Settle how a recipe describes the new chain — which sections exist, how a version is
declared, and what happens to a recipe written for the old flow — before four stage
epics each invent their own answer.

## Design

- **The sharp risk is accepted-and-ignored.** Every recipe struct is
  `deny_unknown_fields`, which catches an *unknown* key and is blind to a **known but
  meaningless** one: a recipe carrying `print.*` loaded under the new flow parses,
  and its knobs do nothing. That is the bug class the project forbids, and the flow
  boundary creates it wholesale rather than one knob at a time. The *refusal* belongs
  with the generic new-flow rejection ([the audit](knob-availability-audit.md)
  classifies presence versus resolved value); the *schema* decides whether such a key
  can be present at all.
- **Versioning.** The one precedent is per-object: the tagged `reconstruction` object
  carries `schema_version` 1 and rejects anything else loudly. Decide whether each
  new stage section carries its own, whether there is a document-level version, and
  what a recipe declaring neither means.
- **`params` is reserved at top level.** `split_envelope` tells a `{meta, params}`
  sidecar from a bare recipe by that key alone, so no stage section may be named
  `params`; a test asserts it stays absent.
- **The round-trip is the cheapest gate on the whole schema.** `--dump-params` and
  `nc params` write every key expanded, and `--preset` has no recipe key. A new-flow
  dump must reload to the same resolved config and the same `params_hash`.
- **§9 is the mapping, and it is not written ahead.** design-spec §9 assigns each
  flag's key to its stage section, and the stage names change here — but
  [the spec task](../nf-docs/design-spec.md) warns that a §9 written before the code
  is a second source of truth. State the shape; let each knob's key land with the
  task that ships it.

## Open questions

- **Migration error or translation.** nc is unreleased and the `algorithm` precedent
  is an error with no aliases. Does `print.*` → its new name get the same treatment,
  or a rename map (the audit owns the table either way)?
- **Does a recipe declare the flow at all?** It cannot name `--new-flow`, which is
  CLI-only, so a recipe's meaning depends on a flag outside it — which is exactly
  what makes the meaningless-key case silent.

## How to Verify

- A recipe carrying an old-flow section fails under the new flow, naming the section
  and its replacement; the same recipe without the flag still loads.
- `--dump-params` output reloads to an identical resolved config and `params_hash`,
  and a docs-shaped recipe copied out of §9 loads rather than being rejected.
- No stage section is named `params`; the existing assertion still holds.
- The four CI gates pass.

## Dependencies

- [The new stage module tree](stage-skeleton.md)

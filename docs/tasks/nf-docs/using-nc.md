# Bring the guide up to the new flow

## Goal

Re-verify `docs/using-nc.md` against the binary once the default is the new
chain. The guide's contract is that it describes what `nc` currently accepts, and
the default flip invalidates most of it at once.

## Design

- **Verified by running the binary, never against a diff.** That is the guide's
  whole contract, and every time it has gone stale, re-verification found two or
  three of its *own* examples broken in ways the changelog did not mention. The
  `update-usingnc-doc` skill carries the procedure and the traps.
- **This is a rewrite, not an edit.** The order the guide teaches — estimate a
  base, pick a curve, then render — follows the old chain's shape. The new chain
  is a stage sequence ending in a destination, and most of the flags the current
  walkthroughs use are renamed or gone.
- **Run it after the flip and before `nf-retire` finishes**, so the removed
  flags still error helpfully while the guide is being written: the migration
  messages are themselves user-visible surface and should be quoted from the
  binary, not invented.
- **This task is the shape change, not a parking spot.** The standing convention
  still holds — a user-visible change updates the guide in the same PR — so
  per-knob updates belong to the tasks that ship them.

## How to Verify

- Every command in the guide was actually run, on the fixtures where possible so
  a reader can follow without assets.
- Every claimed report field, warning and exit code was observed, not recalled.
- A removed flag's error message in the guide matches the binary's, character for
  character.

## Dependencies

- [Flip the default to the new flow](../nf-core/default-flip.md)

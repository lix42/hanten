# What a user would actually run

## Goal

Define the procedure a user follows when the shipped default is not good enough for
their chain: shoot a target, fit, freeze it into a roll recipe. The shipped values
are a prior; this is how someone improves on them.

## Design

- **We can fit our own chain and never a user's.** One global `scale` cannot
  neutralize every stock, developer, scanner and light — design-update Part 2 says
  so of the direct destination, and Part 3 says the loop settles common-ground
  values with the residual left to rendering. So the shipped number is a default
  prior, and a user who wants better needs a *procedure*, not a better constant.
- **A procedure is a product surface.** It needs a target the user can buy, a step
  they can run, and an artifact they can keep. The artifact is the roll recipe: a
  per-roll calibration frozen into one honours nc's roll-consistency promise rather
  than contradicting it, unlike a per-frame fit, which nc rejects by principle.
- **[`io/scanner-density-calibration`](../io/scanner-density-calibration.md) owns
  the instrument and the model** — which target, and what is fitted. This task owns
  the workflow around it: what the user shoots, what they run, what it writes, and
  what nc does when the recipe carries one.

Open:

- **Where the fit runs.** An `nc` subcommand, an `nctool` command, or a documented
  manual read of a report. The first puts a fitting procedure inside the
  deterministic binary; the last is cheapest and hardest to get right.
- **What the recipe carries.** The fitted values themselves, or a named calibration
  the recipe references. Values are self-contained; a reference is reusable across
  rolls from one scanner session.
- **How a user knows they need it** — without a criterion the procedure is advice
  nobody acts on; the neutrality gate's is a candidate. And whether a calibration
  is per scanner, per roll or per session, which decides how often they shoot it.

## How to Verify

- The procedure is written in `docs/using-nc.md`, verified by running the binary,
  and followed end to end by someone who did not write it.
- A calibrated roll renders more neutral than the same roll on the default prior,
  measured rather than asserted.
- A recipe carrying a calibration is deterministic and reproduces across builds.

## Dependencies

- [Scanner Density Calibration](../io/scanner-density-calibration.md) — the
  instrument and the model the fit uses
- [Tune `scale` and `gamma` by review](scale-gamma-loop.md) — the default prior the
  procedure improves on

# services/telemetry-ingest — agent notes

Read `README.md` first. This file holds only what it does not.

- **Gates here are `pnpm verify`** (prettier, `tsc`, `scripts/check-config.mjs`,
  vitest), never the Rust ones. `pnpm` is pinned by `packageManager`; use
  `corepack pnpm` if the global one differs.
- **The contract directory is shared with Rust.** A change to
  `contracts/telemetry/upload-v1/` must pass both `cargo test --bin hanten upload`
  and this suite.
- **Tests run in `workerd`**, so `compatibility_date` cannot pass the date the locked
  `workerd` supports; the runtime refuses to start and every test "fails".
- **Never log a request, header or body**, and never add a column holding one: the
  privacy test dumps every table and fails on a canary. A new table must be added to
  its expected list.
- **Every index costs a written row per insert and per delete**; update the README's
  cost model when adding one.

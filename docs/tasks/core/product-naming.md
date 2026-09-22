# Name the product Hanten, and fix the boundary

## Goal

Give the product its own name — **Hanten** — while `nc` stays the internal name of the
crate, the binary and the identifiers, and **write the boundary down** so it is not
re-litigated every time someone notices the two names disagree.

## Design

**Almost every `nc` in the tree is an identifier, not branding.** That is the whole
reason this is a small change, and the reason the boundary needs recording: without it,
a later pass "tidies up" a version string.

| Stays `nc` | Why |
|---|---|
| `nc-film-rgb-v1` (`working_mapping` in every report) | a **versioned** colour-space identifier. Renaming it means a v2, with pixel-identity questions attached — see CLAUDE.md's colorimetry notes |
| `nc_version`, telemetry `schema_version` 4 | snapshot tests assert the exact JSON; a rename costs a schema bump for nothing |
| `NC_*` environment variables (~16) | diagnostic and test surface |
| recipe keys, report fields, exit codes | the scripting contract |
| the crate, the binary, `nc <subcommand>` | see the open question below |
| `nctool`, `../nc-assets` | tooling and the machine-local asset symlink |

| Becomes Hanten | |
|---|---|
| the GitHub repository | GitHub redirects the old URL, but every worktree's remote needs updating by hand |
| `README.md` title and opening | |
| CLAUDE.md's "What this project is" | |
| `docs/design-spec.md` title and intro | |

**The boundary's home is CLAUDE.md, not this file.** Executing this task moves the two
tables into the "What this project is" section, beside the existing "AI-friendly means
every knob is a flag, not ML" note — which exists for exactly this reason and records
that it "has been explicitly corrected once already". A naming rule with no written home
is a rule that gets undone.

**Not a dependency, but a scheduling constraint.** This touches `README.md`, `CLAUDE.md`
and `docs/design-spec.md` — files nearly every other branch also edits. It has no
dependencies and could run at any time, but it should run **alone**, between merges,
rather than beside the `nf-*` migration, where 50-odd open tasks would each conflict.

## Open questions

- **Should the binary become `hanten`?** Today's answer is no — it is short, typed
  constantly, and an internal command name is ordinary. But the window matters: nc is
  unreleased, so there are no external users of the CLI surface and this is the cheapest
  it will ever be. After a release it is a breaking change forever. The cost if taken:
  every example in `using-nc.md` (which is *verified against the binary*, so it needs
  re-running rather than editing), every skill, `scripts/real-scan-verify/harness.sh`,
  `nctool --nc <binary>`, and every review-set matrix. **Decide it deliberately rather
  than inherit it.**
- Whether `--help`'s preamble and `--version` should name the product. Nothing asserts
  either string today, so it is free; the question is only whether it reads better.
- Whether the repository going private happens with this or separately. It is
  independent — nothing in the tree depends on public access, no badges, no Pages, no
  raw URLs — so it need not wait.

## How to Verify

- `grep -rn "Hanten" --include=* .` finds it only in the four branding locations above
  and in CLAUDE.md's boundary section; **no identifier acquired it**.
- `cargo test --all-features` and the `nctool` suite pass untouched — if either moved,
  something crossed the line.
- The report's `working_mapping` still reads `nc-film-rgb-v1` and the telemetry snapshot
  tests are unmodified.
- Every worktree's `origin` points at the renamed repository.
- CLAUDE.md carries the boundary, and this task file stops being its home.

## Dependencies

None.

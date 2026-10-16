---
title: Stable codes for warnings and errors
status: todo
deps: []
related: [io/truncated-file-fault]
summary: every warning and error carries a code and facts; the CLI formats its prose from them, the app maps codes to its own handling
---
# Stable codes for warnings and errors

## Goal

Every warning and error carries a **stable code**, a severity, the frame it concerns and
its facts (numbers, paths). The CLI formats its prose from the code, so the CLI and the
desktop app never drift; the app's message handler decides from the code whether to
retry, prompt the user or show an error, without parsing wording
([desktop app design](../../design/desktop-app.md) §2.7, §3.3).

## What is known

- Today warnings are prose written for the command line. Some are wrong or noise in an
  app (prototype spike unknown 4): "drop it" on every exposure tweak, the unused IR plane
  on every HDRi export.
- Exit codes are already a contract (design-spec §11); report fields and messages a
  user acts on are documented in `docs/using-nc.md`.
- The preview path produces no report today; the app needs one from previews too.

## Open questions

1. The code scheme and where codes live; whether report JSON gains a code field beside
   the message.
2. Which existing warnings are CLI-only advice the app should ignore.
3. How a preview run reports.

## How to Verify

- Every warning and error the binary emits carries a code; a test enumerates them.
- CLI wording is unchanged, or changed deliberately with `docs/using-nc.md` updated.

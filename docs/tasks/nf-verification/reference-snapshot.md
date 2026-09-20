# The frozen reference build

## Goal

Freeze the pre-migration binary behind a git tag and establish the recipe for
producing a reference rendition from it. This is what lets `nf-retire` run early
instead of last: the old behaviour is preserved by a tag, not by code.

## Design

- **The tag is the preservation mechanism.** Once it exists, nothing in the tree
  has to stay alive to remain comparable — which is the premise the whole
  strangler strategy rests on (CLAUDE.md's migration rule).
- **The rendition comes from building the tag in a worktree** and pointing the
  review harness at that binary: `nctool review generate --nc <binary>` already
  accepts a path, and `analysis/review-build-axis` is the harness half — it
  labels each cell from its own sidecar's `identity` block rather than from a
  typed-in name, so a cell that claims the wrong build is caught.
- **The snapshot is more than a commit.** It must also pin *what to run*: the
  reference config (`--preset sigmoid-knees` is the best result reviewed so far,
  and Part 1 keeps it as the migration's visual reference), and the frames. A tag
  with no recorded invocation is a build nobody can reproduce a comparison from.
- The tag is a named point, not a release — name it so it reads as one.

## Implementation Suggestion

The deliverable is mostly a written procedure plus the tag: where the worktree
goes, the build command, where the built binary is cached (outside the repo — it
is a large artifact), and the reference invocation. Put it where someone looking
for it will find it, beside the verification harness rather than in a progress
log.

## How to Verify

- Building the tag in a fresh worktree and rendering one committed fixture frame
  reproduces byte-identical output across two builds on one machine.
- A review set generated from that binary labels its cells with the tagged
  commit, and a mislabelled cell is flagged.
- Someone who has not done it before can follow the written procedure end to end.

## Dependencies

- [A build axis in the review set](../analysis/review-build-axis.md)

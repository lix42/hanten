# Spike reports

One file per spike: the question it was asked, what was measured, what it concluded, and
what it deliberately did not settle. A spike's **task** lives in `docs/tasks/<epic>/` and
says why the work exists; its **execution trail** lives in `docs/progress/<epic>.md`;
the **result** lives here, because more than one task usually cites it and a closed task
is a bad home for reference material.

The distinction from `docs/reports/`: a report verifies, validates or measures something
that exists. A spike answers an open question before the thing exists.

Sets, scripts and raw measurements stay **outside the repo** (`../temp/<set>/`) — they
are the user's own photographs. A spike report cites them by path and carries only
derived numbers.

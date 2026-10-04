# viewer-interop

The procedure for [`analysis/viewer-interoperability`](../../docs/tasks/analysis/viewer-interoperability.md):
do real viewers open and display each destination as intended? `nctool acceptance`
proves every file decodes correctly from the standards; this checks the apps people
actually use. Android is [`analysis/android-gain-map-check`](../../docs/tasks/analysis/android-gain-map-check.md),
which reuses this set and rubric.

Two halves: `nctool viewer set` builds a fixed file set and its checklist, and
`nctool viewer check` runs the decoders that need no person. The rest is by hand.

## 1. Build the set

```sh
cargo build --release
PYTHONPATH=scripts/analysis python3 -m nctool viewer set \
  --out ../temp/viewer-set --nc target/release/hanten
```

Stdlib only; it needs `../nc-assets` for the two real frames. It renders every
destination the benchmark's `fixtures` set names (each ready `destination::ROWS` row
plus the film master: 12) from each input of
[`scripts/analysis/viewer.json`](../analysis/viewer.json):

| input | why |
|---|---|
| `chart` | the synthetic acceptance chart: known patches, no personal content |
| `ektar-1627` | three clearly different channel gains, so a reader using one channel shows it |
| `gold-1144` | highlights reach the HDR peak on every channel |

A real frame renders under the recipe `hanten measure-roll` writes for its roll (its
real frames, unexposed frame and leader), re-measured on every run. Beside the files:
`viewer-set.json` (each file's destination, encoding, size, sha256, stated gain-map
gains and expectations, and the build) and `rubric.md` (the checklist). Two runs of
one build are byte-identical. A directory that already holds a set is refused, since its
`rubric.md` may hold answers: copy it out, then rebuild with `--force`.

**The set is the user's photographs**: `--out` inside the repository is refused.
Never commit or publish it.

## 2. Decoder pre-checks

```sh
(cd scripts/iso-decoder-oracle && swiftc -O oracle.swift -o oracle)
brew install libultrahdr
PYTHONPATH=scripts/analysis python3 -m nctool viewer check ../temp/viewer-set \
  --oracle scripts/iso-decoder-oracle/oracle
```

For every gain-map JPEG, both **Apple ImageIO** (the oracle) and **libultrahdr**
(`ultrahdr_app`, the reference decoder behind Android's) must find the ISO gain map,
read three channel gains equal to the ones nc's report states, and decode the file at
its own size. The result is `checks.json` in the set, with the decoder versions. A file
changed since the set was built fails. This proves parsing, not that a viewer selects
the HDR rendition; that is the rubric.

## 3. The rubric, by hand

Open `rubric.md`. One table per reader and display setting; per file, answer:

1. it opens without repair or error;
2. it shows the intended rendition (the table's column);
3. with HDR off, or on an SDR-only reader, it falls back as expected;
4. orientation, dimensions and crop are right (the table gives the size);
5. no channel swap, inversion, all-black or all-white render, or edge artifact.

Item 3 is pass/fail only for the gain-map JPEG, whose SDR base is its specified
fallback. No fallback is specified for the HDR TIFFs or the film master, so there it
records what the reader shows. "Plausible" and "looks good" are not answers; say what
was seen.

Who opens what (`viewer.json`'s `expect.*.readers`):

| reader | files | settings |
|---|---|---|
| macOS Preview, macOS Photos | all | HDR display with HDR on; HDR off |
| iPhone Photos | all | HDR on |
| Chrome on macOS | gain-map JPEGs | HDR on; HDR off |
| an SDR-only reader | gain-map JPEGs | as installed |

Record the app, OS and display, and how HDR was turned off. To get files onto the
iPhone, AirDrop them; note whether a file landed in Photos or Files, since that is
itself part of item 1.

## 4. Record

Per reader and setting, log in `docs/progress/analysis.md`: the versions, the count of
yes answers, and every other answer with what was seen. File each failure as its own
task; the rubric fixes nothing inline. Keep the filled `rubric.md` with the set, not in
the repository. Results so far are in that log's `## viewer-interoperability` section.

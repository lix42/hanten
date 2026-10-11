# float-tiff-probe

Does an Apple reader see HDR in a file? A small Swift program that reports the content
headroom ImageIO and Core Image give a file, and that has ImageIO's own encoder write a
float image tagged with a headroom, to see whether any container keeps it. Written for
[`output/float-tiff-in-readers`](../../docs/tasks/output/float-tiff-in-readers.md);
results in [`docs/spike/float-tiff-in-readers.md`](../../docs/spike/float-tiff-in-readers.md).

**macOS-only, manual, not in CI.** Re-run it on a new macOS before believing the HDR
linear TIFF still cannot carry a headroom: no gate would notice Apple starting to read
one, since a TIFF tag changes no pixel.

## Usage

From the repo root, with macOS 26 or newer and the Xcode command-line tools:

```sh
(cd scripts/float-tiff-probe && swiftc -O probe.swift -o probe)
cargo build --release
P=scripts/float-tiff-probe/probe
OUT=../temp/float-tiff-probe   # outside the repo
mkdir -p $OUT

# Does ImageIO's own TIFF encoder keep a headroom?
$P write $OUT/h0.tif 0
$P write $OUT/h493.tif 4.926108
cmp $OUT/h0.tif $OUT/h493.tif && echo "headroom not written"
$P read $OUT/h493.tif

# nc's files, from the committed fixture
base=(--film-base 0.9,0.55,0.42 --report none)
target/release/hanten convert tests/fixtures/hdri-64bit.tif $base \
  --range hdr --transfer linear --gamut display-p3 --container tiff -o $OUT/linear.tiff
target/release/hanten convert tests/fixtures/hdri-64bit.tif $base --film-master -o $OUT/master.tiff
target/release/hanten convert tests/fixtures/hdri-64bit.tif $base \
  --range hdr --transfer pq --gamut bt2020 --container tiff -o $OUT/pq.tiff
$P read $OUT/linear.tiff $OUT/master.tiff $OUT/pq.tiff
```

`write` picks the container from the extension (`exr`, `heic`, `png`, `avif` also
work); `thumb` as a last argument asks ImageIO to embed a thumbnail.

## Reading the output

- `headroom` is what a reader acts on. `0` means unknown, and such an image is
  excluded from tone mapping (`CGImage.h`). An HDR file reads above `1`.
- `calculated` is measured from the pixels (macOS 26). Readers do not use it in place
  of `headroom`: the HDR linear TIFF's is above 1 and it still shows as SDR.
- A PQ or HLG file reads `4.9261084` whatever it carries: that is
  `kCGDefaultHDRImageContentHeadroom` (1000/203), Apple's default for those colour
  spaces, not something nc wrote.
- The probe does not cover thumbnails. Preview's sidebar is the check, by eye; how it
  was done is in the spike.

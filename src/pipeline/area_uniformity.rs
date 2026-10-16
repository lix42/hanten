//! Tiled uniformity of an effective area (`film-base/tiling-uniformity-validator`).
//!
//! The pooled spread [`super::film_base::measure_area`] gates on mixes grain with
//! spatial structure. Splitting the area into [`TILES`]×[`TILES`] tiles separates them:
//! each tile's p05–p95 is grain (*within*), and the tile medians carry the structure,
//! split again into **outlier tiles** (a local defect: dust, a leak, a holder sliver)
//! and the **gradient**, a least-squares plane through the rest.
//!
//! Everything is in **density** (`-log10` of the transmission), so a figure reads the
//! same on a thin scan and a dense one. Diagnostic only: nothing here moves the base.

use std::sync::LazyLock;

use serde::Serialize;

use super::film_base::{CODES, quantile_rank};

/// Tiles per side of the area.
pub const TILES: u32 = 4;

const TILE_COUNT: usize = (TILES * TILES) as usize;

/// The tile histograms' bin width, in density: an order below the smallest figure the
/// thresholds separate.
const BIN_DENSITY: f32 = 0.0005;

/// The top bin's lower edge; above `-log10(1/65535) = 4.82`, so only code 0 lands in it.
const MAX_DENSITY: f32 = 4.9;

const BINS: usize = (MAX_DENSITY / BIN_DENSITY) as usize + 1;

/// Each 16-bit code's density bin.
static BIN_OF: LazyLock<Vec<u16>> = LazyLock::new(|| {
    (0..CODES)
        .map(|code| {
            let d = -((code as f32 / (CODES - 1) as f32).log10());
            (d / BIN_DENSITY).min((BINS - 1) as f32) as u16
        })
        .collect()
});

/// The tile percentiles bounding *within*.
const WITHIN_PERCENTILES: [f32; 2] = [0.05, 0.95];

/// Largest gradient, `|left → right| + |top → bottom|` on any channel, of a uniform area.
///
/// Measured 2026-10-13 over `../nc-assets` (the `probe` below): the 12 unexposed frames
/// have a gradient of at most 0.030 (0.10 stops); leaders 0.015-0.152 — Portra 160's
/// 0.114, blue left to right as `docs/reports/sigmoid-reference-baseline.md` found, Gold
/// 200's 0.027.
/// Set above the unexposed worst with headroom, and below Portra 160's leader.
pub const MAX_GRADIENT: f32 = 0.05;

/// Largest `|offset|`, on any channel, of a tile from the plane through the others.
///
/// Measured with [`MAX_GRADIENT`]: no unexposed frame has a tile more than 0.012 off the
/// plane through its other 15, so 0.03 (0.1 stops) leaves 2.5× headroom. On a leader,
/// edge falloff that is not planar can set aside a whole edge column.
pub const MAX_TILE_OFFSET: f32 = 0.03;

/// Most tiles set aside as outliers. More than this is not a local defect, and is
/// left in the plane to show as gradient.
pub const MAX_OUTLIER_TILES: usize = 4;

/// The tiled decomposition of one area, per channel `[r, g, b]`, in density.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AreaUniformity {
    /// Tiles across and down.
    pub tiles: [u32; 2],
    /// Grain: the median over tiles of each tile's p05–p95.
    pub within: [f32; 3],
    /// The tile medians' range — all the between-tile structure, before it is split.
    pub between: [f32; 3],
    /// The plane through the tile medians, outlier tiles left out: its change
    /// `[left → right, top → bottom]` from edge to edge of the area. Positive is
    /// denser toward the right or bottom.
    pub gradient: [[f32; 2]; 3],
    /// Tiles set aside, in that order, each more than [`MAX_TILE_OFFSET`] off the plane
    /// through the tiles still kept when it was.
    pub outlier_tiles: Vec<OutlierTile>,
    pub verdict: Verdict,
}

/// A tile set aside from the plane.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct OutlierTile {
    /// Zero-based, from the area's left.
    pub column: u32,
    /// Zero-based, from the area's top.
    pub row: u32,
    /// Its median minus the plane's value there, as it was set aside; positive is
    /// denser.
    pub offset: [f32; 3],
}

impl OutlierTile {
    fn on_edge(&self) -> bool {
        [self.column, self.row]
            .iter()
            .any(|&i| i == 0 || i == TILES - 1)
    }
}

/// What the decomposition says about the area.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Uniform,
    /// The gradient exceeds [`MAX_GRADIENT`] on some channel.
    Sloped,
    /// Some tile is more than [`MAX_TILE_OFFSET`] off the plane.
    OutlierTiles,
    SlopedWithOutlierTiles,
}

/// The warnings an area's verdict raises: one for a gradient, one for the outlier tiles.
pub fn warnings(u: &AreaUniformity) -> Vec<String> {
    let mut out = Vec::new();
    if matches!(u.verdict, Verdict::Sloped | Verdict::SlopedWithOutlierTiles) {
        let c = worst_channel(u.gradient.map(|[x, y]| x.abs() + y.abs()));
        let [x, y] = u.gradient[c];
        out.push(format!(
            "the effective area is sloped: its {} density changes by {x:+.3} from the left \
             edge to the right and {y:+.3} from the top to the bottom (more than {MAX_GRADIENT} \
             in all; grain within a tile is {:.3}), so the median is a base for its middle, \
             not its edges. Uneven light on the scan, or film that is not unexposed: measure \
             another unexposed frame, or a flat region of this one (`hanten measure-base \
             FRAME --base-region X,Y,W,H`)",
            CHANNELS[c], u.within[c]
        ));
    }
    if !u.outlier_tiles.is_empty() {
        let tiles: Vec<String> = u
            .outlier_tiles
            .iter()
            .map(|t| {
                let c = worst_channel(t.offset.map(f32::abs));
                format!(
                    "column {}, row {} ({:+.3} on {})",
                    t.column, t.row, t.offset[c], CHANNELS[c]
                )
            })
            .collect();
        let inset = if u.outlier_tiles.iter().any(OutlierTile::on_edge) {
            "; raise --measure-inset if one at the area's edge is the holder"
        } else {
            ""
        };
        out.push(format!(
            "part of the effective area stands out, more than {MAX_TILE_OFFSET} density off \
             the plane through the rest of its {}x{} tiles (counted from the top left): {} \
             — dust, a light leak or a holder sliver. The median resists a few tiles; look \
             at those parts of the frame{inset}",
            u.tiles[0],
            u.tiles[1],
            tiles.join("; ")
        ));
    }
    out
}

const CHANNELS: [&str; 3] = ["red", "green", "blue"];

/// The channel with the largest value; the first on a tie.
fn worst_channel(v: [f32; 3]) -> usize {
    (1..3).fold(0, |w, c| if v[c] > v[w] { c } else { w })
}

/// Per-tile, per-channel counts of density bins, fed one sample at a time by the scan
/// that also builds `film_base`'s code histogram. Fixed ~1.9 MB whatever the frame size,
/// so `pipeline::memory` owes it no term.
pub struct Tiling {
    /// Each column of the area's tile, and each row's.
    col_of: Vec<u8>,
    row_of: Vec<u8>,
    counts: Vec<u32>,
    n: [[u64; 3]; TILE_COUNT],
}

impl Tiling {
    /// A tiling of a `w`×`h` area; `None` when it is narrower or shorter than [`TILES`].
    pub fn new(w: u32, h: u32) -> Option<Self> {
        let along = |len: u32| (0..len).map(|i| (i * TILES / len) as u8).collect();
        (w >= TILES && h >= TILES).then(|| Self {
            col_of: along(w),
            row_of: along(h),
            counts: vec![0; TILE_COUNT * 3 * BINS],
            n: [[0; 3]; TILE_COUNT],
        })
    }

    /// Count channel `c`'s 16-bit `code` at `(dx, dy)` within the area.
    #[inline]
    pub fn add(&mut self, dx: u32, dy: u32, c: usize, code: usize) {
        let t =
            self.row_of[dy as usize] as usize * TILES as usize + self.col_of[dx as usize] as usize;
        self.counts[(t * 3 + c) * BINS + BIN_OF[code] as usize] += 1;
        self.n[t][c] += 1;
    }

    /// Decompose the counts; `None` when some tile has no finite sample on a channel,
    /// since it has no median to compare.
    pub fn finish(self) -> Option<AreaUniformity> {
        if self.n.iter().flatten().any(|&n| n == 0) {
            return None;
        }
        let [lo, hi] = WITHIN_PERCENTILES;
        let mut medians = [[0.0f32; 3]; TILE_COUNT];
        let mut within = [0.0f32; 3];
        for c in 0..3 {
            let mut spans = [0.0f32; TILE_COUNT];
            for (t, (m, s)) in medians.iter_mut().zip(&mut spans).enumerate() {
                m[c] = self.percentile(t, c, 0.5);
                *s = self.percentile(t, c, hi) - self.percentile(t, c, lo);
            }
            spans.sort_by(f32::total_cmp);
            within[c] = spans[TILE_COUNT / 2];
        }
        Some(decompose(&medians, within))
    }

    /// Tile `t`'s `p`-quantile on channel `c`, in density at its bin's centre.
    fn percentile(&self, t: usize, c: usize, p: f32) -> f32 {
        let k = quantile_rank(self.n[t][c], p);
        let bins = &self.counts[(t * 3 + c) * BINS..][..BINS];
        let mut seen = 0u64;
        for (b, &count) in bins.iter().enumerate() {
            seen += u64::from(count);
            if seen > k {
                return (b as f32 + 0.5) * BIN_DENSITY;
            }
        }
        unreachable!("rank {k} is below the tile's count")
    }
}

/// A tile's position relative to the grid's centre, in tiles.
fn at(t: usize) -> (f64, f64) {
    let centre = (TILES - 1) as f64 / 2.0;
    (
        (t as u32 % TILES) as f64 - centre,
        (t as u32 / TILES) as f64 - centre,
    )
}

/// Split the tile medians (row-major) into outlier tiles and a gradient.
///
/// Each round sets aside the tile furthest, on any channel, from the plane through the
/// other kept tiles, while that distance exceeds [`MAX_TILE_OFFSET`] — judged against a
/// plane it is not in, so it cannot drag the plane toward itself, and up to
/// [`MAX_OUTLIER_TILES`]. The gradient is then the plane through the tiles kept.
fn decompose(medians: &[[f32; 3]; TILE_COUNT], within: [f32; 3]) -> AreaUniformity {
    let value = |c: usize| move |t: usize| medians[t][c] as f64;
    let mut kept: Vec<usize> = (0..TILE_COUNT).collect();
    let mut outlier_tiles = Vec::new();
    while outlier_tiles.len() < MAX_OUTLIER_TILES {
        // Each kept tile's offset from the plane through the other kept tiles.
        let offsets = kept.iter().map(|&t| {
            let others: Vec<usize> = kept.iter().copied().filter(|&o| o != t).collect();
            let offset: [f64; 3] =
                std::array::from_fn(|c| value(c)(t) - plane_at(&others, value(c), at(t)));
            (t, offset)
        });
        let mut worst: Option<(usize, [f64; 3], f64)> = None;
        for (t, offset) in offsets {
            let r = offset.iter().fold(0.0f64, |m, o| m.max(o.abs()));
            if worst.is_none_or(|(_, _, w)| r > w) {
                worst = Some((t, offset, r));
            }
        }
        match worst {
            Some((t, offset, r)) if r > MAX_TILE_OFFSET as f64 => {
                kept.retain(|&k| k != t);
                outlier_tiles.push(OutlierTile {
                    column: t as u32 % TILES,
                    row: t as u32 / TILES,
                    offset: offset.map(|o| o as f32),
                });
            }
            _ => break,
        }
    }

    let span = TILES as f64;
    let mut gradient = [[0.0f32; 2]; 3];
    let mut between = [0.0f32; 3];
    for c in 0..3 {
        let [_, bx, by] = coefficients(&kept, value(c));
        gradient[c] = [(bx * span) as f32, (by * span) as f32];
        let (lo, hi) = medians
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), m| {
                (lo.min(m[c]), hi.max(m[c]))
            });
        between[c] = hi - lo;
    }
    let sloped = gradient
        .iter()
        .any(|[x, y]| x.abs() + y.abs() > MAX_GRADIENT);
    let verdict = match (sloped, !outlier_tiles.is_empty()) {
        (false, false) => Verdict::Uniform,
        (true, false) => Verdict::Sloped,
        (false, true) => Verdict::OutlierTiles,
        (true, true) => Verdict::SlopedWithOutlierTiles,
    };
    AreaUniformity {
        tiles: [TILES, TILES],
        within,
        between,
        gradient,
        outlier_tiles,
        verdict,
    }
}

/// The least-squares plane through `tiles`, evaluated at `(x, y)`.
fn plane_at(tiles: &[usize], value: impl Fn(usize) -> f64, (x, y): (f64, f64)) -> f64 {
    let [a, bx, by] = coefficients(tiles, value);
    a + bx * x + by * y
}

/// `[a, bx, by]` of the least-squares plane `a + bx·x + by·y` through `tiles` at their
/// [`at`] positions, by Cramer's rule on the normal equations; sums run in `tiles`'
/// order. At least `TILE_COUNT - MAX_OUTLIER_TILES - 1` tiles remain, never collinear.
fn coefficients(tiles: &[usize], value: impl Fn(usize) -> f64) -> [f64; 3] {
    let (mut n, mut sx, mut sy, mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let (mut sv, mut sxv, mut syv) = (0.0, 0.0, 0.0);
    for &t in tiles {
        let (x, y) = at(t);
        let v = value(t);
        n += 1.0;
        sx += x;
        sy += y;
        sxx += x * x;
        syy += y * y;
        sxy += x * y;
        sv += v;
        sxv += x * v;
        syv += y * v;
    }
    let m = [[n, sx, sy], [sx, sxx, sxy], [sy, sxy, syy]];
    let rhs = [sv, sxv, syv];
    let det = det3(m);
    std::array::from_fn(|k| {
        let mut mk = m;
        for (row, r) in mk.iter_mut().zip(rhs) {
            row[k] = r;
        }
        det3(mk) / det
    })
}

fn det3(m: [[f64; 3]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

/// Tile `region` of `image` alone, outside `film_base::measure_area`'s pass.
#[cfg(test)]
pub(crate) fn measure(
    image: &crate::types::LinearImage,
    region: [u32; 4],
) -> crate::types::Result<Option<AreaUniformity>> {
    let Some(mut tiling) = Tiling::new(region[2], region[3]) else {
        super::film_base::check_rect(image, region)?;
        return Ok(None);
    };
    super::film_base::for_each_code(image, region, |dx, dy, c, code| tiling.add(dx, dy, c, code))?;
    Ok(tiling.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LinearImage;

    /// A `w`x`h` frame whose density at each pixel is `density(x, y)` over a 0.3 base.
    fn frame(w: u32, h: u32, density: impl Fn(u32, u32) -> f32) -> LinearImage {
        let mut buf = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                let t = 0.3 * 10f32.powf(-density(x, y));
                buf.extend_from_slice(&[t, t, t]);
            }
        }
        LinearImage::new(w, h, buf).unwrap()
    }

    /// Deterministic per-pixel noise, uniform in `[-a, a]`.
    fn noise(x: u32, y: u32, a: f32) -> f32 {
        // SplitMix64 over the pixel's index.
        let mut v = (u64::from(y) << 32 | u64::from(x)).wrapping_add(0x9e37_79b9_7f4a_7c15);
        v = (v ^ (v >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        v = (v ^ (v >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        v ^= v >> 31;
        a * (2.0 * (v >> 11) as f32 / (1u64 << 53) as f32 - 1.0)
    }

    /// `delta` density inside the tile at `(column, row)` of a 200x200 frame.
    fn in_tile(x: u32, y: u32, (column, row): (u32, u32), delta: f32) -> f32 {
        if x / 50 == column && y / 50 == row {
            delta
        } else {
            0.0
        }
    }

    fn measure_all(img: &LinearImage) -> AreaUniformity {
        measure(img, [0, 0, img.width, img.height])
            .unwrap()
            .unwrap()
    }

    fn located(u: &AreaUniformity) -> Vec<(u32, u32)> {
        u.outlier_tiles.iter().map(|t| (t.column, t.row)).collect()
    }

    #[test]
    fn grain_alone_is_within_tile_and_uniform() {
        let u = measure_all(&frame(200, 200, |x, y| noise(x, y, 0.1)));
        for c in 0..3 {
            assert!(u.within[c] > 0.15, "{u:?}");
            assert!(u.between[c] < 0.01, "{u:?}");
        }
        assert_eq!(u.verdict, Verdict::Uniform);
        assert!(warnings(&u).is_empty());
    }

    #[test]
    fn a_smooth_ramp_is_a_gradient_not_grain() {
        let u = measure_all(&frame(200, 200, |x, _| 0.2 * x as f32 / 200.0));
        for c in 0..3 {
            let [lr, tb] = u.gradient[c];
            assert!((lr - 0.2).abs() < 0.003, "edge to edge: {u:?}");
            assert!(tb.abs() < 0.002, "{u:?}");
            assert!(u.within[c] < 0.05, "{u:?}");
        }
        assert_eq!(u.verdict, Verdict::Sloped);
        let w = warnings(&u);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("changes by +0.20"), "{w:?}");
        assert!(
            w[0].contains("hanten measure-base FRAME --base-region"),
            "{w:?}"
        );
    }

    #[test]
    fn one_bad_tile_is_located_and_does_not_skew_the_gradient() {
        let u = measure_all(&frame(200, 200, |x, y| {
            noise(x, y, 0.05) + in_tile(x, y, (2, 1), 0.1)
        }));
        assert_eq!(located(&u), [(2, 1)], "{u:?}");
        for c in 0..3 {
            assert!((u.outlier_tiles[0].offset[c] - 0.1).abs() < 0.005, "{u:?}");
            assert!(u.gradient[c].iter().all(|t| t.abs() < 0.005), "{u:?}");
        }
        assert_eq!(u.verdict, Verdict::OutlierTiles);
        let w = warnings(&u);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("column 2, row 1 (+0.09"), "{w:?}");
        // An interior tile: a larger inset cannot reach it.
        assert!(!w[0].contains("--measure-inset"), "{w:?}");
    }

    #[test]
    fn two_bad_tiles_are_both_set_aside_and_do_not_read_as_a_gradient() {
        let u = measure_all(&frame(200, 200, |x, y| {
            in_tile(x, y, (0, 0), 0.1) + in_tile(x, y, (1, 0), 0.1)
        }));
        let mut tiles = located(&u);
        tiles.sort();
        assert_eq!(tiles, [(0, 0), (1, 0)], "{u:?}");
        assert!(u.gradient.iter().flatten().all(|t| t.abs() < 1e-3), "{u:?}");
        assert_eq!(u.verdict, Verdict::OutlierTiles);
        assert!(warnings(&u)[0].contains("--measure-inset"), "an edge tile");
    }

    #[test]
    fn an_outlier_on_one_channel_is_caught_beside_a_worse_one_on_another() {
        // Red alone is off at (0, 0); blue alone at (3, 2), by less.
        let mut img = frame(200, 200, |_, _| 0.0);
        for ((column, row), c, d) in [((0, 0), 0, 0.08f32), ((3, 2), 2, 0.05)] {
            for y in row * 50..(row + 1) * 50 {
                for x in column * 50..(column + 1) * 50 {
                    img.rgb[((y * 200 + x) * 3) as usize + c] = 0.3 * 10f32.powf(-d);
                }
            }
        }
        let u = measure_all(&img);
        assert_eq!(located(&u), [(0, 0), (3, 2)], "{u:?}");
        assert!((u.outlier_tiles[1].offset[2] - 0.05).abs() < 0.003, "{u:?}");
        assert!(u.gradient.iter().flatten().all(|t| t.abs() < 1e-3), "{u:?}");
    }

    #[test]
    fn a_ramp_and_a_bad_tile_are_both_reported() {
        let u = measure_all(&frame(200, 200, |x, y| {
            0.2 * y as f32 / 200.0 + in_tile(x, y, (0, 0), -0.1)
        }));
        assert_eq!(located(&u), [(0, 0)], "{u:?}");
        assert!((u.gradient[0][1] - 0.2).abs() < 0.003, "{u:?}");
        assert_eq!(u.verdict, Verdict::SlopedWithOutlierTiles);
        assert_eq!(warnings(&u).len(), 2);
    }

    #[test]
    fn more_defects_than_the_cap_are_not_all_set_aside() {
        // Half the area dense: not a local defect.
        let u = measure_all(&frame(200, 200, |x, _| if x < 100 { 0.1 } else { 0.0 }));
        assert!(u.outlier_tiles.len() <= MAX_OUTLIER_TILES, "{u:?}");
        assert_ne!(u.verdict, Verdict::Uniform, "{u:?}");
    }

    #[test]
    fn a_tile_with_no_finite_sample_leaves_the_area_untiled() {
        let mut img = frame(200, 200, |_, _| 0.0);
        for y in 0..50 {
            for x in 0..50 {
                img.rgb[((y * 200 + x) * 3) as usize] = f32::NAN;
            }
        }
        assert_eq!(measure(&img, [0, 0, 200, 200]).unwrap(), None);
    }

    #[test]
    fn an_area_smaller_than_the_tiling_is_not_tiled() {
        let img = frame(10, 10, |_, _| 0.0);
        assert_eq!(measure(&img, [0, 0, TILES - 1, 10]).unwrap(), None);
        assert!(measure(&img, [0, 0, TILES, TILES]).unwrap().is_some());
        assert!(measure(&img, [8, 8, 4, 4]).is_err(), "out of bounds");
        assert!(
            measure(&img, [8, 8, 3, 3]).is_err(),
            "out of bounds, untileable"
        );
    }

    #[test]
    fn the_plane_fit_recovers_an_exact_plane_from_any_fifteen_tiles() {
        let rest: Vec<usize> = (0..TILE_COUNT).filter(|&t| t != 5).collect();
        let [a, bx, by] = coefficients(&rest, |t| {
            let (x, y) = at(t);
            0.5 + 0.02 * x - 0.03 * y
        });
        assert!((a - 0.5).abs() < 1e-12, "{a}");
        assert!((bx - 0.02).abs() < 1e-12, "{bx}");
        assert!((by + 0.03).abs() < 1e-12, "{by}");
    }
}

/// Probe: the decomposition over every manifest roll's unexposed and leader frames, on
/// the effective area `measure-base` reads. Prints derived numbers only.
#[cfg(test)]
mod probe {
    use super::*;
    use crate::pipeline::film_base::effective_area;
    use std::path::PathBuf;

    #[test]
    #[ignore = "requires ../nc-assets; run with --ignored --nocapture"]
    fn decomposition_on_reference_frames() {
        let root = std::env::var("NC_ASSETS")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../nc-assets"));
        let Ok(text) = std::fs::read_to_string(root.join("manifest.json")) else {
            eprintln!("SKIP: no manifest");
            return;
        };
        let m: serde_json::Value = serde_json::from_str(&text).unwrap();
        let f3 = |v: [f32; 3]| format!("{:.4}/{:.4}/{:.4}", v[0], v[1], v[2]);
        for (roll, entry) in m["rolls"].as_object().unwrap() {
            for f in entry["frames"].as_array().unwrap() {
                let role = f["role"].as_str().unwrap();
                if !matches!(role, "unexposed" | "leader") {
                    continue;
                }
                let p = root.join(f["file"].as_str().unwrap());
                let (scan, _) = crate::io::decode::decode_within(&p, u64::MAX).unwrap();
                let area = effective_area(&scan, crate::types::DEFAULT_MEASURE_INSET).unwrap();
                let u = measure(&scan.image, area.region).unwrap().unwrap();
                let t = u.gradient;
                let outliers: Vec<String> = u
                    .outlier_tiles
                    .iter()
                    .map(|o| format!("({},{}) {}", o.column, o.row, f3(o.offset)))
                    .collect();
                println!(
                    "{roll} {role:9} within {}  between {}  gradient {} (lr {:+.4}/{:+.4}/{:+.4} \
                     tb {:+.4}/{:+.4}/{:+.4})  outliers [{}]",
                    f3(u.within),
                    f3(u.between),
                    f3(t.map(|[x, y]| x.abs() + y.abs())),
                    t[0][0],
                    t[1][0],
                    t[2][0],
                    t[0][1],
                    t[1][1],
                    t[2][1],
                    outliers.join(", "),
                );
            }
        }
    }
}

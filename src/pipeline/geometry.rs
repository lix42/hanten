//! Crop and orientation (`core/crop-rotate`): which part of the scan is the picture, and
//! which way up it is written.
//!
//! **A crop is in the scan's own pixels** — `[x, y, w, h]`, the convention of
//! `--base-region` and `effective_area.region`, before any rotation — so a stated crop
//! keeps its meaning whatever the orientation. It is applied twice, to two different
//! things:
//!
//! - **The measurement region.** [`intersect`] makes it a third cut of the effective area
//!   (`film_base::effective_area`), so every statistic read over the area — the film base,
//!   the roll's white and white balance, the polarity check — reads only the picture.
//!   The holder march still runs on the whole scan: it marches in from the scan's edges.
//! - **The pixels.** [`crop_scan`] cuts the scan before the fixed decode, so every stage
//!   after it sees only the picture.
//!
//! **Orientation is a flip, then a clockwise quarter turn**, applied by [`orient`] to the
//! rendered image just before the destination's encode. It only moves pixels, so an
//! oriented output is the unoriented one turned, sample for sample, and no statistic
//! the chain reads sees a different order.
//!
//! Both work in place (`pipeline::memory`): the crop compacts rows to the front of the
//! scan's own buffers, and the orientation follows the permutation's cycles with one bit
//! per pixel to mark what has moved.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::types::{LinearImage, NcError, Result, Scan};

/// A clockwise quarter turn, written as its degrees.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub enum Rotation {
    #[default]
    R0,
    R90,
    R180,
    R270,
}

impl Rotation {
    pub fn degrees(self) -> u16 {
        match self {
            Self::R0 => 0,
            Self::R90 => 90,
            Self::R180 => 180,
            Self::R270 => 270,
        }
    }

    /// Whether the turn swaps width and height.
    fn swaps(self) -> bool {
        matches!(self, Self::R90 | Self::R270)
    }
}

impl TryFrom<u16> for Rotation {
    type Error = String;

    fn try_from(degrees: u16) -> std::result::Result<Self, String> {
        match degrees {
            0 => Ok(Self::R0),
            90 => Ok(Self::R90),
            180 => Ok(Self::R180),
            270 => Ok(Self::R270),
            other => Err(format!(
                "a rotation is a clockwise quarter turn, 0, 90, 180 or 270 (got {other})"
            )),
        }
    }
}

impl From<Rotation> for u16 {
    fn from(r: Rotation) -> u16 {
        r.degrees()
    }
}

/// `--rotate`'s value parser.
pub fn parse_rotation(s: &str) -> std::result::Result<Rotation, String> {
    let degrees: u16 = s
        .trim()
        .parse()
        .map_err(|_| format!("expected 0, 90, 180 or 270, got {s:?}"))?;
    Rotation::try_from(degrees)
}

/// A mirror of the scan, applied before the turn. `none` is stated, so a frame's own
/// entry can undo the roll's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Flip {
    #[default]
    None,
    /// Left and right swap.
    Horizontal,
    /// Top and bottom swap.
    Vertical,
}

/// One frame's crop and orientation. Each unset value is the identity, or the roll's
/// for a [`GeometrySection::frames`] entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Geometry {
    /// `[x, y, w, h]` in the scan's own pixels, before the orientation.
    pub crop: Option<[u32; 4]>,
    pub rotate: Option<Rotation>,
    pub flip: Option<Flip>,
}

impl Geometry {
    /// `over`'s stated values, the rest from `self`.
    fn under(self, over: Geometry) -> Geometry {
        Geometry {
            crop: over.crop.or(self.crop),
            rotate: over.rotate.or(self.rotate),
            flip: over.flip.or(self.flip),
        }
    }

    pub fn orientation(&self) -> Orientation {
        Orientation {
            flip: self.flip.unwrap_or_default(),
            rotate: self.rotate.unwrap_or_default(),
        }
    }

    /// Whether this writes the scan's pixels as decoded.
    pub fn is_identity(&self) -> bool {
        self.crop.is_none() && self.orientation().is_identity()
    }

    /// The written image's `(width, height)` for a `width`×`height` scan.
    pub fn output_size(&self, width: u32, height: u32) -> (u32, u32) {
        let (w, h) = self.crop.map_or((width, height), |[_, _, w, h]| (w, h));
        if self.orientation().rotate.swaps() {
            (h, w)
        } else {
            (w, h)
        }
    }
}

/// What a conversion did to the scan's pixels, for the report: present only when it
/// cropped, flipped or turned them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct GeometryReport {
    /// `[x, y, w, h]` in the scan's own pixels; absent when the whole scan is kept.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crop: Option<[u32; 4]>,
    pub flip: Flip,
    pub rotate: Rotation,
    /// The written image's size.
    pub width: u32,
    pub height: u32,
}

impl GeometryReport {
    /// The report for `g` over a `width`×`height` scan; `None` when `g` is the identity.
    pub fn of(g: &Geometry, width: u32, height: u32) -> Option<Self> {
        let (w, h) = g.output_size(width, height);
        let o = g.orientation();
        (!g.is_identity()).then_some(Self {
            crop: g.crop,
            flip: o.flip,
            rotate: o.rotate,
            width: w,
            height: h,
        })
    }
}

/// The recipe's `geometry` section: the crop and orientation for every frame, and each
/// frame's own, keyed by file name like `roll.frames`. Written with its nulls.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GeometrySection {
    pub crop: Option<[u32; 4]>,
    pub rotate: Option<Rotation>,
    pub flip: Option<Flip>,
    /// A frame's own values, over the section's.
    pub frames: BTreeMap<String, Geometry>,
}

impl GeometrySection {
    /// The values every frame shares.
    pub fn shared(&self) -> Geometry {
        Geometry {
            crop: self.crop,
            rotate: self.rotate,
            flip: self.flip,
        }
    }

    /// The geometry `input` is converted and measured with: its entry over the shared.
    pub fn of(&self, input: &Path) -> Geometry {
        let entry = input
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| self.frames.get(n));
        self.shared().under(entry.copied().unwrap_or_default())
    }

    /// Set the typed flags' values. A flag beats every frame's entry, so the value it
    /// beat leaves the table: what measures a frame is what renders it.
    pub fn apply_flags(&mut self, crop: Option<[u32; 4]>, o: Option<Rotation>, f: Option<Flip>) {
        self.frames.retain(|_, entry| {
            if crop.is_some() {
                entry.crop = None;
            }
            if o.is_some() {
                entry.rotate = None;
            }
            if f.is_some() {
                entry.flip = None;
            }
            *entry != Geometry::default()
        });
        self.crop = crop.or(self.crop);
        self.rotate = o.or(self.rotate);
        self.flip = f.or(self.flip);
    }

    /// Move `input`'s entry, if any, into the shared values, removing it — as
    /// `Recipe::for_frame` does with `roll.frames`.
    pub fn take_frame(&mut self, input: &Path) {
        let name = input.file_name().and_then(|n| n.to_str());
        if let Some(entry) = name.and_then(|n| self.frames.remove(n)) {
            let g = self.shared().under(entry);
            self.crop = g.crop;
            self.rotate = g.rotate;
            self.flip = g.flip;
        }
    }
}

/// Check the section as stated: a crop of positive size whose far edge a `u32` holds,
/// and an entry per file name that states something.
pub fn validate(section: &GeometrySection, crop_name: &str) -> Result<()> {
    check_crop_shape(section.crop, crop_name)?;
    validate_frames(section)
}

/// Check the `geometry.frames` table, each entry by its own name.
pub fn validate_frames(section: &GeometrySection) -> Result<()> {
    for (name, entry) in &section.frames {
        if name.is_empty() || Path::new(name).file_name() != Some(name.as_ref()) {
            return Err(NcError::Usage(format!(
                "recipe `geometry.frames` keys are file names, not paths: got {name:?}"
            )));
        }
        if *entry == Geometry::default() {
            return Err(NcError::Usage(format!(
                "recipe `geometry.frames.\"{name}\"` states nothing: give it a `crop`, a \
                 `rotate` or a `flip`, or drop the entry"
            )));
        }
        check_crop_shape(
            entry.crop,
            &format!("recipe `geometry.frames.\"{name}\".crop`"),
        )?;
    }
    Ok(())
}

/// Refuse a crop of zero size or one whose far edge a `u32` cannot hold.
pub fn check_crop_shape(crop: Option<[u32; 4]>, name: &str) -> Result<()> {
    let Some([x, y, w, h]) = crop else {
        return Ok(());
    };
    if w == 0 || h == 0 {
        return Err(NcError::Usage(format!(
            "{name} width and height must be > 0 (got {w}x{h})"
        )));
    }
    if x.checked_add(w).is_none() || y.checked_add(h).is_none() {
        return Err(NcError::Usage(format!(
            "{name} [{x}, {y}, {w}, {h}] reaches past any frame"
        )));
    }
    Ok(())
}

/// Whether `[x, y, w, h]` lies inside a `width`×`height` image; `u64` edges, so a
/// rectangle near `u32::MAX` cannot wrap.
pub fn rect_within([x, y, w, h]: [u32; 4], width: u32, height: u32) -> bool {
    u64::from(x) + u64::from(w) <= u64::from(width)
        && u64::from(y) + u64::from(h) <= u64::from(height)
}

/// Refuse a crop reaching past a `width`×`height` scan. Run on the probed size before
/// the decode, and again wherever a crop meets a scan.
pub fn check_crop_fits(crop: Option<[u32; 4]>, width: u32, height: u32) -> Result<()> {
    let Some([x, y, w, h]) = crop else {
        return Ok(());
    };
    if w == 0 || h == 0 || !rect_within([x, y, w, h], width, height) {
        return Err(NcError::Usage(format!(
            "the crop [{x}, {y}, {w}, {h}] (--crop, recipe `geometry.crop` or the frame's \
             `geometry.frames` entry) reaches past the {width}x{height} scan: it is \
             [x, y, w, h] in the scan's own pixels, before any rotation"
        )));
    }
    Ok(())
}

/// The rectangle two `[x, y, w, h]` rectangles share, or `None` when they share no pixel.
pub fn intersect(a: [u32; 4], b: [u32; 4]) -> Option<[u32; 4]> {
    let far = |o: u32, len: u32| u64::from(o) + u64::from(len);
    let x0 = a[0].max(b[0]);
    let y0 = a[1].max(b[1]);
    let x1 = far(a[0], a[2]).min(far(b[0], b[2]));
    let y1 = far(a[1], a[3]).min(far(b[1], b[3]));
    (x1 > u64::from(x0) && y1 > u64::from(y0)).then(|| {
        [
            x0,
            y0,
            (x1 - u64::from(x0)) as u32,
            (y1 - u64::from(y0)) as u32,
        ]
    })
}

/// Cut `scan` to `crop`, its RGB and IR plane together, in place. `None` returns it as is.
pub fn crop_scan(mut scan: Scan, crop: Option<[u32; 4]>) -> Result<Scan> {
    let Some(rect) = crop else {
        return Ok(scan);
    };
    let (width, height) = (scan.image.width, scan.image.height);
    check_crop_fits(crop, width, height)?;
    crop_rows(&mut scan.image.rgb, width, 3, rect);
    if let Some(ir) = scan.ir.as_mut() {
        crop_rows(&mut ir.samples, width, 1, rect);
    }
    scan.image.width = rect[2];
    scan.image.height = rect[3];
    Ok(scan)
}

/// Compact `rect`'s rows of a `width`-wide interleaved buffer to its front, then drop
/// the rest. Each row moves to an index no later than its own, so copying in row order
/// never overwrites a row still to move.
fn crop_rows<T: Copy>(buf: &mut Vec<T>, width: u32, channels: usize, [x, y, w, h]: [u32; 4]) {
    let (width, x, y, w) = (width as usize, x as usize, y as usize, w as usize);
    let row = w * channels;
    for r in 0..h as usize {
        let src = ((y + r) * width + x) * channels;
        buf.copy_within(src..src + row, r * row);
    }
    buf.truncate(row * h as usize);
    // Release the tail: the decode reuses this buffer through the whole chain. A mapped
    // block shrinks in place (`allocator::resize`).
    buf.shrink_to_fit();
}

/// A flip, then a clockwise quarter turn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Orientation {
    pub flip: Flip,
    pub rotate: Rotation,
}

impl Orientation {
    pub fn is_identity(self) -> bool {
        self == Self::default()
    }

    /// The source pixel `(x, y)` of a `w`×`h` image that lands at output `(ox, oy)`.
    fn source(self, w: usize, h: usize, ox: usize, oy: usize) -> (usize, usize) {
        // Undo the turn into the flipped image's coordinates, then undo the flip.
        let (fx, fy) = match self.rotate {
            Rotation::R0 => (ox, oy),
            Rotation::R90 => (oy, h - 1 - ox),
            Rotation::R180 => (w - 1 - ox, h - 1 - oy),
            Rotation::R270 => (w - 1 - oy, ox),
        };
        match self.flip {
            Flip::None => (fx, fy),
            Flip::Horizontal => (w - 1 - fx, fy),
            Flip::Vertical => (fx, h - 1 - fy),
        }
    }
}

/// Orient `image` in place: its pixels move, its width and height swap on a quarter
/// turn, and no sample changes.
pub fn orient(image: &mut LinearImage, o: Orientation) {
    let (w, h) = (image.width, image.height);
    permute(&mut image.rgb, 3, w as usize, h as usize, o);
    if o.rotate.swaps() {
        (image.width, image.height) = (h, w);
    }
}

/// Apply `o` to a `w`×`h` buffer of `channels`-sample pixels. A quarter turn follows the
/// permutation's cycles: each output slot takes its source's pixel, and the slot freed
/// is filled next, until the cycle closes on the pixel held aside.
fn permute<T: Copy + Default>(buf: &mut [T], channels: usize, w: usize, h: usize, o: Orientation) {
    if o.is_identity() || w == 0 || h == 0 {
        return;
    }
    // A half turn and the flips are involutions: one pass of swaps, in order.
    let row = w * channels;
    let swap_pixels = |buf: &mut [T], a: usize, b: usize| {
        for c in 0..channels {
            buf.swap(a * channels + c, b * channels + c);
        }
    };
    match (o.rotate, o.flip) {
        (Rotation::R0, Flip::Horizontal) | (Rotation::R180, Flip::Vertical) => {
            for line in buf.chunks_exact_mut(row) {
                for x in 0..w / 2 {
                    swap_pixels(line, x, w - 1 - x);
                }
            }
            return;
        }
        (Rotation::R0, Flip::Vertical) | (Rotation::R180, Flip::Horizontal) => {
            for y in 0..h / 2 {
                let (top, bottom) = buf.split_at_mut((h - 1 - y) * row);
                top[y * row..(y + 1) * row].swap_with_slice(&mut bottom[..row]);
            }
            return;
        }
        (Rotation::R180, Flip::None) => {
            let n = w * h;
            for i in 0..n / 2 {
                swap_pixels(buf, i, n - 1 - i);
            }
            return;
        }
        _ => {}
    }
    let out_w = if o.rotate.swaps() { h } else { w };
    // Output index → source index, both in pixels.
    let source_of = |out: usize| {
        let (sx, sy) = o.source(w, h, out % out_w, out / out_w);
        sy * w + sx
    };
    let n = w * h;
    let mut moved = vec![0u64; n.div_ceil(64)];
    let mut held = [T::default(); 4];
    for start in 0..n {
        if moved[start / 64] >> (start % 64) & 1 == 1 {
            continue;
        }
        held[..channels].copy_from_slice(&buf[start * channels..(start + 1) * channels]);
        let mut slot = start;
        loop {
            moved[slot / 64] |= 1 << (slot % 64);
            let from = source_of(slot);
            if from == start {
                buf[slot * channels..(slot + 1) * channels].copy_from_slice(&held[..channels]);
                break;
            }
            buf.copy_within(from * channels..(from + 1) * channels, slot * channels);
            slot = from;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::IrPlane;

    /// A `w`×`h` image whose pixel at `(x, y)` is `[x, y, x * 100 + y]`.
    fn labelled(w: u32, h: u32) -> LinearImage {
        let rgb = (0..h)
            .flat_map(|y| (0..w).flat_map(move |x| [x as f32, y as f32, (x * 100 + y) as f32]))
            .collect();
        LinearImage::new(w, h, rgb).unwrap()
    }

    fn at(img: &LinearImage, x: u32, y: u32) -> [f32; 3] {
        let i = ((y * img.width + x) * 3) as usize;
        [img.rgb[i], img.rgb[i + 1], img.rgb[i + 2]]
    }

    fn label(x: u32, y: u32) -> [f32; 3] {
        [x as f32, y as f32, (x * 100 + y) as f32]
    }

    fn oriented(w: u32, h: u32, flip: Flip, rotate: Rotation) -> LinearImage {
        let mut img = labelled(w, h);
        orient(&mut img, Orientation { flip, rotate });
        img
    }

    #[test]
    fn a_quarter_turn_is_clockwise_and_swaps_the_size() {
        let img = oriented(3, 2, Flip::None, Rotation::R90);
        assert_eq!((img.width, img.height), (2, 3));
        // The scan's top-left lands top-right; its bottom-left, top-left.
        assert_eq!(at(&img, 1, 0), label(0, 0));
        assert_eq!(at(&img, 0, 0), label(0, 1));
        assert_eq!(at(&img, 0, 2), label(2, 1));
    }

    #[test]
    fn each_turn_lands_the_top_left_corner_where_it_should() {
        let cases = [
            (Rotation::R0, (0, 0)),
            (Rotation::R90, (1, 0)),
            (Rotation::R180, (2, 1)),
            (Rotation::R270, (0, 2)),
        ];
        for (rotate, (x, y)) in cases {
            let img = oriented(3, 2, Flip::None, rotate);
            assert_eq!(at(&img, x, y), label(0, 0), "{rotate:?}");
        }
    }

    #[test]
    fn the_flip_comes_before_the_turn() {
        // Mirrored left to right, then turned: the scan's top-right lands top-right.
        let img = oriented(3, 2, Flip::Horizontal, Rotation::R90);
        assert_eq!(at(&img, 1, 0), label(2, 0));
        let img = oriented(3, 2, Flip::Vertical, Rotation::R0);
        assert_eq!(at(&img, 0, 0), label(0, 1));
    }

    #[test]
    fn every_orientation_moves_pixels_and_changes_none() {
        for flip in [Flip::None, Flip::Horizontal, Flip::Vertical] {
            for rotate in [Rotation::R0, Rotation::R90, Rotation::R180, Rotation::R270] {
                for (w, h) in [(1, 1), (1, 7), (5, 3), (4, 4), (64, 3), (65, 2)] {
                    let o = Orientation { flip, rotate };
                    let img = oriented(w, h, flip, rotate);
                    let (ow, _) = (img.width, img.height);
                    for oy in 0..img.height {
                        for ox in 0..ow {
                            let (sx, sy) =
                                o.source(w as usize, h as usize, ox as usize, oy as usize);
                            assert_eq!(
                                at(&img, ox, oy),
                                label(sx as u32, sy as u32),
                                "{o:?} {w}x{h}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn four_quarter_turns_are_the_identity() {
        let mut img = labelled(5, 3);
        let turn = Orientation {
            flip: Flip::None,
            rotate: Rotation::R90,
        };
        for _ in 0..4 {
            orient(&mut img, turn);
        }
        assert_eq!((img.width, img.height), (5, 3));
        assert_eq!(img.rgb, labelled(5, 3).rgb);
    }

    #[test]
    fn a_crop_keeps_its_rectangle_in_rgb_and_ir() {
        let image = labelled(5, 4);
        let ir = IrPlane {
            samples: (0..20).map(|i| i as f32).collect(),
            verified: true,
        };
        let scan = crop_scan(Scan::new(image, Some(ir)).unwrap(), Some([1, 2, 3, 2])).unwrap();
        assert_eq!((scan.image.width, scan.image.height), (3, 2));
        assert_eq!(scan.image.rgb.len(), 3 * 2 * 3);
        assert_eq!(at(&scan.image, 0, 0), label(1, 2));
        assert_eq!(at(&scan.image, 2, 1), label(3, 3));
        assert_eq!(
            scan.ir.unwrap().samples,
            vec![11.0, 12.0, 13.0, 16.0, 17.0, 18.0]
        );
    }

    #[test]
    fn no_crop_returns_the_scan_untouched() {
        let scan = crop_scan(Scan::new(labelled(4, 3), None).unwrap(), None).unwrap();
        assert_eq!(scan.image.rgb, labelled(4, 3).rgb);
    }

    #[test]
    fn a_crop_past_the_scan_is_refused() {
        let scan = Scan::new(labelled(4, 3), None).unwrap();
        let err = crop_scan(scan, Some([2, 0, 3, 3])).unwrap_err();
        assert!(matches!(err, NcError::Usage(_)), "{err}");
        assert!(err.to_string().contains("4x3"), "{err}");
        assert!(check_crop_fits(Some([1, 0, 3, 3]), 4, 3).is_ok());
    }

    #[test]
    fn rectangles_intersect_or_share_nothing() {
        assert_eq!(intersect([0, 0, 10, 10], [5, 2, 10, 3]), Some([5, 2, 5, 3]));
        assert_eq!(intersect([0, 0, 10, 10], [10, 0, 5, 5]), None);
        assert_eq!(
            intersect([2, 2, 4, 4], [0, 0, 100, 100]),
            Some([2, 2, 4, 4])
        );
    }

    #[test]
    fn the_output_size_follows_the_crop_then_the_turn() {
        let g = Geometry {
            crop: Some([0, 0, 30, 20]),
            rotate: Some(Rotation::R270),
            flip: None,
        };
        assert_eq!(g.output_size(100, 80), (20, 30));
        assert_eq!(Geometry::default().output_size(100, 80), (100, 80));
    }

    #[test]
    fn a_frames_entry_overrides_the_shared_values_one_by_one() {
        let mut s = GeometrySection {
            crop: Some([0, 0, 10, 10]),
            rotate: Some(Rotation::R90),
            ..Default::default()
        };
        s.frames.insert(
            "a.tif".into(),
            Geometry {
                rotate: Some(Rotation::R180),
                ..Default::default()
            },
        );
        let g = s.of(Path::new("/scans/a.tif"));
        assert_eq!(g.crop, Some([0, 0, 10, 10]));
        assert_eq!(g.rotate, Some(Rotation::R180));
        assert_eq!(s.of(Path::new("b.tif")).rotate, Some(Rotation::R90));
        s.take_frame(Path::new("/scans/a.tif"));
        assert!(s.frames.is_empty());
        assert_eq!(s.shared(), g);
    }

    #[test]
    fn rotation_reads_and_writes_degrees() {
        let r: Rotation = serde_json::from_str("270").unwrap();
        assert_eq!(r, Rotation::R270);
        assert_eq!(serde_json::to_string(&Rotation::R90).unwrap(), "90");
        let err = serde_json::from_str::<Rotation>("45").unwrap_err();
        assert!(err.to_string().contains("quarter turn"), "{err}");
        assert!(parse_rotation("91").is_err());
    }

    #[test]
    fn validation_names_the_bad_entry() {
        let mut s = GeometrySection::default();
        s.frames.insert("dir/a.tif".into(), Geometry::default());
        assert!(
            validate(&s, "--crop")
                .unwrap_err()
                .to_string()
                .contains("file names")
        );
        let mut s = GeometrySection::default();
        s.frames.insert("a.tif".into(), Geometry::default());
        assert!(
            validate(&s, "--crop")
                .unwrap_err()
                .to_string()
                .contains("states nothing")
        );
        let s = GeometrySection {
            crop: Some([0, 0, 0, 5]),
            ..Default::default()
        };
        assert!(
            validate(&s, "--crop")
                .unwrap_err()
                .to_string()
                .contains("--crop")
        );
    }
}

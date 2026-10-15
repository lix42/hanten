/**
 * Chart labelling's arithmetic: a target's patches projected from four corners.
 *
 * The same projection as `nctool.charts` (`scripts/analysis/nctool/charts.py`), which
 * checks a label against the scan; the overlay only has to show where that check
 * will sample. Both tests pin one shared case, so the two cannot drift apart.
 *
 * Corners are fractions of the frame, the target's own top left first, then
 * clockwise in the target's orientation.
 */

export type Point = readonly [number, number];
export type Quad = readonly [Point, Point, Point, Point];

export interface TargetPatch {
  readonly id: string;
  readonly name: string;
  /** `[x, y, w, h]` in target units. */
  readonly rect: readonly [number, number, number, number];
}

export interface Target {
  readonly id: string;
  readonly version: number;
  readonly name: string;
  /** Prose: which way up the target is, and where its corners sit. */
  readonly corners: string;
  readonly size: readonly [number, number];
  readonly patches: readonly TargetPatch[];
  readonly neutrals: readonly string[];
  readonly saturated?: readonly string[];
  /** The patch the user clicks when several ways up pass the check. */
  readonly anchor: { readonly patch: string; readonly prompt: string };
}

export interface ChartLabel {
  readonly target: string;
  readonly card: string;
  readonly corners: Quad;
  /** The user chose the way up (the anchor click): the check verifies only this order. */
  readonly oriented?: true;
}

/**
 * Every way `label` breaks `nctool.charts.check_label`'s shape rules, which every check
 * and import applies to the whole labels file: one bad label saved would break them
 * for every frame, so the server refuses it first.
 */
export function labelProblems(label: ChartLabel): string[] {
  const c = label.corners as readonly (readonly number[])[];
  if (c.length !== 4 || !c.every((p) => p.length === 2 && p.every(Number.isFinite))) {
    return ["corners must be four [x, y] points"];
  }
  if (!c.every((p) => p.every((v) => v >= 0 && v <= 1))) {
    return ["corners must lie inside the frame"];
  }
  const turn = (i: number) => {
    const [a, b, d] = [c[i]!, c[(i + 1) % 4]!, c[(i + 2) % 4]!];
    return (b[0]! - a[0]!) * (d[1]! - a[1]!) - (b[1]! - a[1]!) * (d[0]! - a[0]!);
  };
  const turns = [0, 1, 2, 3].map(turn);
  if (!(turns.every((t) => t > 0) || turns.every((t) => t < 0))) {
    return ["corners must go round a convex quadrilateral (a corner was dragged across)"];
  }
  return label.card.trim() === "" ? ["card must not be empty"] : [];
}

const sameLabels = (a: readonly ChartLabel[], b: readonly ChartLabel[]) =>
  JSON.stringify(a) === JSON.stringify(b);

/**
 * A frame's labels once a save's check returns: the corners the check settled on,
 * marked clean — but only if the frame still holds what was sent. A corner dragged
 * while the check ran (seconds) is kept, and the frame stays unsaved.
 */
export function mergeChecked(
  sent: readonly ChartLabel[],
  current: readonly ChartLabel[],
  results: readonly CheckResult[],
): { readonly labels: readonly ChartLabel[]; readonly clean: boolean } {
  if (!sameLabels(sent, current)) return { labels: current, clean: false };
  return {
    labels: sent.map((l, i) => ({ ...l, corners: results[i]?.corners ?? l.corners })),
    clean: true,
  };
}

/**
 * What an unlabelled frame starts from: the previous frame's labels, copied, the first
 * time the frame is opened only — a frame cleared on purpose stays clear.
 */
export function seedLabels(
  firstVisit: boolean,
  current: readonly ChartLabel[],
  previous: readonly ChartLabel[] | undefined,
): readonly ChartLabel[] | undefined {
  if (!firstVisit || current.length > 0 || !previous || previous.length === 0) return undefined;
  return previous.map((l) => ({
    target: l.target,
    card: l.card,
    corners: l.corners.map((p) => [p[0], p[1]] as const) as unknown as Quad,
  }));
}

/** One frame's check, as `nctool manifest charts check` prints it per label. */
export interface CheckResult {
  readonly target: string;
  readonly card: string;
  readonly pass: boolean;
  readonly failures: readonly string[];
  /** Gaps read bright, but too few to say the grid is off (blur, flare). */
  readonly warnings: readonly string[];
  readonly worst_spread: number;
  readonly worst_gap: number | null;
  readonly neutral_steps: readonly number[];
  /** The corners in the target's own order — the turn that passed, else as given. */
  readonly corners: Quad;
  readonly reoriented: boolean;
  /** Every passing orientation, when more than one passed (and so `pass` is false). */
  readonly candidates?: readonly Quad[];
}

export type Matrix = readonly [
  readonly [number, number, number],
  readonly [number, number, number],
  readonly [number, number, number],
];

/** Gaussian elimination with partial pivoting; throws on degenerate corners. */
function solve(a: number[][], b: number[]): number[] {
  const n = b.length;
  const m = a.map((row, i) => [...row, b[i] ?? 0]);
  for (let c = 0; c < n; c++) {
    let p = c;
    for (let r = c + 1; r < n; r++) if (Math.abs(m[r]![c]!) > Math.abs(m[p]![c]!)) p = r;
    if (Math.abs(m[p]![c]!) < 1e-12) throw new Error("degenerate corners");
    [m[c], m[p]] = [m[p]!, m[c]!];
    for (let r = 0; r < n; r++) {
      if (r === c) continue;
      const f = m[r]![c]! / m[c]![c]!;
      m[r] = m[r]!.map((x, k) => x - f * m[c]![k]!);
    }
  }
  return m.map((row, i) => row[n]! / row[i]!);
}

/** The projective map taking the four `src` points to the four `dst`. */
export function homography(src: readonly Point[], dst: readonly Point[]): Matrix {
  const a: number[][] = [];
  const b: number[] = [];
  src.forEach(([x, y], i) => {
    const [u, v] = dst[i]!;
    a.push([x, y, 1, 0, 0, 0, -u * x, -u * y], [0, 0, 0, x, y, 1, -v * x, -v * y]);
    b.push(u, v);
  });
  const h = solve(a, b);
  return [
    [h[0]!, h[1]!, h[2]!],
    [h[3]!, h[4]!, h[5]!],
    [h[6]!, h[7]!, 1],
  ];
}

export function project(h: Matrix, [x, y]: Point): Point {
  const w = h[2][0] * x + h[2][1] * y + h[2][2];
  return [(h[0][0] * x + h[0][1] * y + h[0][2]) / w, (h[1][0] * x + h[1][1] * y + h[1][2]) / w];
}

export function targetToFrame(target: Target, corners: Quad): Matrix {
  const [w, h] = target.size;
  return homography(
    [
      [0, 0],
      [w, 0],
      [w, h],
      [0, h],
    ],
    corners,
  );
}

function rectQuad([x, y, w, h]: readonly [number, number, number, number]): Quad {
  return [
    [x, y],
    [x + w, y],
    [x + w, y + h],
    [x, y + h],
  ];
}

export interface ProjectedPatch {
  readonly id: string;
  readonly quad: Quad;
  /** The middle the check samples (`charts.INNER`, half of each side). */
  readonly sample: Quad;
  readonly neutral: boolean;
}

/** Every patch of `target` in frame fractions, or `undefined` for degenerate corners. */
export function projectPatches(target: Target, corners: Quad): ProjectedPatch[] | undefined {
  let h: Matrix;
  try {
    h = targetToFrame(target, corners);
  } catch {
    return undefined;
  }
  const map = (q: Quad): Quad => [
    project(h, q[0]),
    project(h, q[1]),
    project(h, q[2]),
    project(h, q[3]),
  ];
  return target.patches.map((p) => {
    const [x, y, w, hh] = p.rect;
    return {
      id: p.id,
      quad: map(rectQuad(p.rect)),
      sample: map(rectQuad([x + w / 4, y + hh / 4, w / 2, hh / 2])),
      neutral: target.neutrals.includes(p.id),
    };
  });
}

/**
 * Four clicks, in whatever order they came, as a quad running clockwise on screen,
 * starting from the click nearest the frame's top left. `width` and `height` are the
 * frame's pixels, since a fraction of a non-square frame is not a distance.
 */
export function orderClockwise(points: readonly Point[], width: number, height: number): Quad {
  const px = points.map(([x, y]) => [x * width, y * height] as const);
  const cx = px.reduce((s, p) => s + p[0], 0) / px.length;
  const cy = px.reduce((s, p) => s + p[1], 0) / px.length;
  // With y pointing down, increasing angle runs clockwise on screen.
  const byAngle = points
    .map((p, i) => ({
      p,
      a: Math.atan2(px[i]![1] - cy, px[i]![0] - cx),
      d: px[i]![0] + px[i]![1],
    }))
    .sort((a, b) => a.a - b.a);
  const start = byAngle.reduce((best, q, i) => (q.d < byAngle[best]!.d ? i : best), 0);
  const ring = [...byAngle.slice(start), ...byAngle.slice(0, start)].map((q) => q.p);
  return [ring[0]!, ring[1]!, ring[2]!, ring[3]!];
}

/**
 * `corners` turned so the target's long side lies along the quad's long side.
 * Which end is the top is left to the check, which reads the scan
 * (`nctool.charts.find_placement`): from the outline alone, a target and the same
 * target upside down look alike.
 */
export function fitLongSide(target: Target, corners: Quad, width: number, height: number): Quad {
  const len = (a: Point, b: Point) => Math.hypot((a[0] - b[0]) * width, (a[1] - b[1]) * height);
  const across = len(corners[0], corners[1]) + len(corners[2], corners[3]);
  const down = len(corners[1], corners[2]) + len(corners[3], corners[0]);
  const wide = target.size[0] >= target.size[1];
  return wide === across >= down ? corners : rotate(corners);
}

/** Whether `p` lies inside the convex quad `q` (either winding). */
export function insideQuad(q: Quad, p: Point): boolean {
  const cross = (a: Point, b: Point) =>
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
  const s = [0, 1, 2, 3].map((i) =>
    cross(q[i as 0 | 1 | 2 | 3], q[((i + 1) % 4) as 0 | 1 | 2 | 3]),
  );
  return s.every((v) => v >= 0) || s.every((v) => v <= 0);
}

/**
 * Of several orientations that all pass the check, the one that puts the target's
 * anchor patch (its `anchor`, e.g. the Classic's white) under the user's click; or
 * `undefined` when none does.
 */
export function chooseByAnchor(
  target: Target,
  candidates: readonly Quad[],
  click: Point,
): Quad | undefined {
  return candidates.find((corners) => {
    const patch = projectPatches(target, corners)?.find((p) => p.id === target.anchor.patch);
    return patch !== undefined && insideQuad(patch.quad, click);
  });
}

/**
 * The same corners for the target turned a quarter turn clockwise in the frame:
 * what was its bottom-left corner becomes its top left. For a label placed with the
 * target the wrong way up — four presses come back to the start.
 */
export function rotate(corners: Quad): Quad {
  return [corners[3], corners[0], corners[1], corners[2]];
}

/** Patch ids a failure line names (`"A1 not uniform…"`, `"gap A1|right…"`, `"A4->B4"`). */
export function failingIds(failures: readonly string[]): Set<string> {
  const ids = new Set<string>();
  for (const f of failures) {
    for (const m of f.matchAll(/\b([A-Z][0-9]+[a-z]?)(?=[ |\-,)]|$)/g)) ids.add(m[1]!);
  }
  return ids;
}

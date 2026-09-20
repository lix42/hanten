/**
 * Patches: a labelled rectangle on a frame.
 *
 * Select an area, say what it is — "cloud", "white shirt", "shadow under the
 * bridge" — so a later comparison can talk about a region rather than about the
 * picture as a whole. The labels are the point: an agent reading the copied
 * block learns *where to look*, which is what it cannot see for itself.
 *
 * **A patch belongs to the frame, not to a config.** Every rendition of a frame
 * is the same subject rendered differently, so one rectangle has to sit on the
 * same cloud in all of them — that is what makes a patch usable for comparing
 * variants, and it is the same in-place principle the stage is built on.
 * Coordinates are normalised to the image (see `geometry.ts`), so they survive a
 * fit/fullsize toggle, a resize, and a config that renders at another size.
 *
 * They live only in the page, exactly like notes: a review set is a throwaway
 * directory, nothing here writes to the user's photographs, and the way a patch
 * leaves the page is the `n` dialog's Copy all.
 *
 * There is no edit. A patch is a rectangle plus a handful of words; correcting
 * one is deleting it and drawing it again, which is a shorter gesture than any
 * editor would be.
 */

import type { NormalRect } from "./geometry";

export interface Patch extends NormalRect {
  readonly id: string;
  readonly label: string;
}

/** Image id -> its patches, in the order they were drawn. */
export type Patches = Readonly<Record<string, readonly Patch[]>>;

export function patchesFor(patches: Patches, imageId: string): readonly Patch[] {
  return patches[imageId] ?? [];
}

export function addPatch(patches: Patches, imageId: string, patch: Patch): Patches {
  return { ...patches, [imageId]: [...patchesFor(patches, imageId), patch] };
}

/**
 * Drop one patch, and the frame's key with it when it was the last.
 *
 * So "how many frames have patches" stays a key count, the same rule
 * `setNote` follows for a blanked note.
 */
export function removePatch(patches: Patches, imageId: string, patchId: string): Patches {
  const kept = patchesFor(patches, imageId).filter((patch) => patch.id !== patchId);
  if (kept.length === patchesFor(patches, imageId).length) return patches;
  if (kept.length === 0) {
    const { [imageId]: _dropped, ...rest } = patches;
    return rest;
  }
  return { ...patches, [imageId]: kept };
}

export function patchCount(patches: Patches): number {
  return Object.values(patches).reduce((total, list) => total + list.length, 0);
}

export function patchedFrameCount(patches: Patches): number {
  return Object.values(patches).filter((list) => list.length > 0).length;
}

/**
 * One patch as a line of the copied block.
 *
 * Percentages of the frame rather than pixels: a patch is stored against the
 * image, and the renditions of one frame need not share a pixel size — a
 * percentage is the one spelling that is true of all of them. One decimal is
 * about a pixel on a 10,000px scan, which is far finer than a hand-drawn
 * rectangle is meant to be read.
 */
export function formatPatch(patch: Patch): string {
  const pct = (value: number) => `${(value * 100).toFixed(1)}%`;
  return (
    `- ${JSON.stringify(patch.label)} — x ${pct(patch.x)} y ${pct(patch.y)} ` +
    `w ${pct(patch.width)} h ${pct(patch.height)}`
  );
}

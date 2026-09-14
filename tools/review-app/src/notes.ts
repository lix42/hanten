/**
 * Review notes: what the viewer wrote about each frame.
 *
 * They live only in the page — no server, no storage — because a review set is a
 * throwaway directory and the notes are meant to be collected in one go and
 * pasted somewhere that keeps them. That also means a reload loses them, which is
 * why `n` offers the whole set in one copyable block.
 *
 * Keyed by image id rather than by index: the set is watched, and an edit to
 * `review.json` can reorder frames under a note that was written about one of
 * them.
 */

/** Frame id -> note. A frame with no note simply has no key. */
export type Notes = Readonly<Record<string, string>>;

/** What a note is attached to, for formatting. */
export interface NotedFrame {
  readonly id: string;
  readonly label: string;
}

/**
 * Set a frame's note, or drop it when the text is blank.
 *
 * Blank removes rather than storing `""` so "how many notes are there" stays a
 * key count, and clearing a textarea is the same gesture as never writing one.
 * Returns the same object when nothing changed, so a signal set from it does not
 * re-render every dialog on a keystroke that altered nothing.
 */
export function setNote(notes: Notes, id: string, text: string): Notes {
  const trimmed = text.trim();
  const has = Object.hasOwn(notes, id);
  if (trimmed === "") {
    if (!has) return notes;
    const { [id]: _dropped, ...rest } = notes;
    return rest;
  }
  if (has && notes[id] === text) return notes;
  // The raw text is kept, not the trimmed one: a note may be several paragraphs
  // and trimming as you type would fight the writer. Only blankness is judged.
  return { ...notes, [id]: text };
}

export function noteCount(notes: Notes): number {
  return Object.keys(notes).length;
}

/**
 * The identity of what is being reviewed, for deciding when notes are stale.
 *
 * Deliberately the set's path and its **frame list**, not its renditions: the
 * workflow this app is built for is re-running `nc` over the same frames and
 * watching them update in place, and wiping the notes on every such refresh
 * would make them useless. A different set, or a set whose frames changed, is
 * what makes an existing note describe something that is no longer there.
 */
export function notesScope(path: string, imageIds: readonly string[]): string {
  return `${path}\n${imageIds.join("\n")}`;
}

/**
 * Every note as one block, in set order.
 *
 * Set order rather than the order they were written: the result is read beside
 * the review, and a reader scanning for a frame expects it where the set puts
 * it. Frames with no note are left out entirely rather than listed as empty —
 * the block is a summary of what was found, not a form.
 */
export function formatNotes(frames: readonly NotedFrame[], notes: Notes, heading?: string): string {
  const noted = frames.filter((frame) => (notes[frame.id] ?? "").trim() !== "");
  if (noted.length === 0) return "";
  const title = heading?.trim();
  const head = [
    title ? `# Review notes — ${title}` : "# Review notes",
    `${noted.length} of ${frames.length} frames noted`,
  ];
  const body = noted.map((frame) => `## ${frame.label}\n\n${(notes[frame.id] ?? "").trim()}`);
  return [...head, ...body].join("\n\n") + "\n";
}

/**
 * Building a DOM id out of values that were never meant to be one.
 *
 * The cast chart addresses its gradients with `stroke="url(#<id>)"`, and the id
 * is built from a review set's image and config ids — which the schema lets be
 * any non-empty string, because they are labels in a document written by hand or
 * by a script, not identifiers in a page. One containing `)` closes the paint
 * URL early: the reference resolves to nothing, the curves lose their stroke, and
 * the record loaded perfectly well, so nothing anywhere says why.
 *
 * The encoding is reversible rather than a scrub, so two ids that differ cannot
 * collapse into one gradient — which would paint one frame's curves with
 * another's ramp, and colour *is* the encoding on that chart. Kept free of the
 * DOM so it can be tested directly.
 */

/** Everything outside `[A-Za-z0-9]` becomes `_<hex>_`, so a part holds no `-`. */
function encodePart(part: string): string {
  return part.replace(
    /[^A-Za-z0-9]/g,
    (character) => `_${(character.codePointAt(0) ?? 0).toString(16)}_`,
  );
}

/**
 * A DOM id for one chart, from the values that identify it.
 *
 * Joined with `--`, which no encoded part can contain because a literal hyphen
 * encodes to `_2d_`. The `chart` prefix keeps the result a valid XML name even
 * when the first part starts with a digit.
 */
export function domId(...parts: readonly string[]): string {
  return ["chart", ...parts.map(encodePart)].join("--");
}

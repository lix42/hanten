/**
 * What produced a cell — the provenance a build comparison is read by.
 *
 * A review set's premise is that two cells differ in exactly one declared way.
 * A build axis breaks that premise unless the page can say *which binary* each
 * cell came from, and a name typed into a matrix cannot: it is a claim, and the
 * failure this exists to prevent is a claim that is wrong. So the generator reads
 * each build's identity back off its own renders and writes it here; the matrix
 * only ever supplies the short name.
 *
 * **Tagged by `kind`** so a cell no `hanten` produced — an NLP export, a
 * hand-edited target (`analysis/review-reference-cells`) — is a variant of this
 * rather than a second block. Both answer one question: which cell did
 * nc-as-configured not render?
 */

/** A cell rendered by a named build of `hanten`. Every identity field is optional:
 * a binary built from a tarball stamps no commit, and a picture with an
 * incomplete label still beats no picture. */
export interface HantenProducer {
  readonly kind: "hanten";
  readonly label: string;
  readonly note?: string;
  readonly ncVersion?: string;
  readonly gitCommit?: string;
  readonly gitDirty?: boolean;
  readonly pipelineVersion?: number;
  readonly target?: string;
}

/** A cell some other producer made, brought into the set as a reference. */
export interface ExternalProducer {
  readonly kind: "external";
  readonly label: string;
  readonly note?: string;
}

export type Producer = HantenProducer | ExternalProducer;

/**
 * The conventional short commit, from whatever length was stamped.
 *
 * nc stamps twelve hex digits; seven is what a reader pastes back into
 * `git show`. Anything shorter is passed through rather than padded — a value
 * this did not expect is still more use displayed than hidden.
 */
export function shortCommit(commit: string): string {
  return commit.length > 7 ? commit.slice(0, 7) : commit;
}

/**
 * The one-line provenance shown beside a rendition.
 *
 * Kept out of the component because nothing in this app can test a `.tsx`, and
 * this is the line that decides whether a reader can tell two builds apart.
 *
 * A **dirty tree is said out loud**: it is the one identity field that means the
 * binary is not reproducible from what the commit names, which for a
 * patched-versus-shipped comparison is the whole point of the cell.
 *
 * The version is printed **bare, with no product word**. It is `CARGO_PKG_VERSION`
 * and nothing more; a build old enough to call itself `nc` is precisely the one the
 * generator goes out of its way to accept, and it is usually the reference arm of a
 * before/after — so printing `hanten` beside it would mislabel the cell most likely
 * to be misread.
 */
export function producerSummary(producer: Producer): string {
  if (producer.kind === "external") {
    return producer.note ?? "not rendered by hanten";
  }
  const parts: string[] = [];
  parts.push(producer.gitCommit ? shortCommit(producer.gitCommit) : "commit unknown");
  if (producer.gitDirty) parts.push("dirty tree");
  if (producer.pipelineVersion !== undefined) parts.push(`pipeline ${producer.pipelineVersion}`);
  if (producer.ncVersion !== undefined) parts.push(producer.ncVersion);
  if (producer.target !== undefined) parts.push(producer.target);
  if (producer.note) parts.push(producer.note);
  return parts.join(" · ");
}

import { createServerFn } from "@tanstack/solid-start";
import type { Review } from "../review";
import { bootId } from "./boot";
import { currentReviewSet } from "./state";

export interface ReviewSetPayload {
  readonly review: Review;
  /** Absolute path of the review.json, shown in the header and on failure. */
  readonly path: string;
  readonly source: "env" | "bundled-example";
  /**
   * Identity of the server that rendered this. The page compares `/alive`
   * against it, so a replacement is recognised even when the original was
   * already gone before the first poll.
   */
  readonly boot: string;
}

/**
 * Hand the page its review set.
 *
 * A server function rather than a plain loader because the set is read with
 * `node:fs`: the handler body is stripped from the client build and replaced
 * with a call, so nothing drags a Node builtin into the browser bundle.
 */
export const getReviewSet = createServerFn({ method: "GET" }).handler(
  async (): Promise<ReviewSetPayload> => {
    const set = await currentReviewSet();
    return { review: set.review, path: set.path, source: set.source, boot: bootId() };
  },
);

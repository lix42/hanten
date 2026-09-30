// The upload-v1 contract, read from contracts/telemetry/upload-v1/ — the same
// files the Rust side tests against, never a looser copy. Its README owns the
// 400 cases and the rejection precedence this module implements.
import { Validator, type OutputUnit, type Schema } from "@cfworker/json-schema";
import schemaJson from "../../../contracts/telemetry/upload-v1/upload-v1.schema.json";

const schema = schemaJson as unknown as Schema;
const id = schema.$id as string;

function validator(def: string, shortCircuit: boolean): Validator {
  const v = new Validator({ $ref: `${id}#/$defs/${def}` }, "2020-12", shortCircuit);
  v.addSchema(schema);
  return v;
}

const envelope = validator("envelope", true);
const event = validator("event", true);
// All errors, so a failure can be classified; run only on an event already known bad.
const eventAllErrors = validator("event", false);
const response = validator("response", true);

/** Local schema versions this Worker accepts (`source_schema_version`). */
export const SUPPORTED_SOURCE_VERSIONS: readonly number[] = [11];

/** The body cap; a larger body is a malformed request. */
export const MAX_BODY_BYTES = 262_144;

export type RejectionCode = "invalid_field" | "unsupported_version" | "out_of_range" | "release_blocked";

export interface UploadEvent {
  event_id: string;
  event_day: number;
  source_schema_version: number;
  event_name: "conversion" | "panic";
  nc_version: string;
  platform: { os: string; arch: string; cpu_bucket: string };
  command: string;
  stage: string;
  outcome?: { status: string; exit_code: number; error_kind: string };
  timing_ms?: { total: number };
  image?: { megapixels_tenths: number; input_size_bucket: string };
  conversion?: { encoding: string };
}

export interface Envelope {
  upload_schema_version: 1;
  events: unknown[];
}

/**
 * Whether `body` is a well-formed request envelope: the schema's `$defs/envelope`
 * plus the one rule a schema cannot state, unique `event_id`s.
 */
export function isValidEnvelope(body: unknown): body is Envelope {
  if (!envelope.validate(body).valid) return false;
  const ids = (body as Envelope).events.map((e) => (e as { event_id: string }).event_id);
  return new Set(ids).size === ids.length;
}

/**
 * The rejection code for one event of a valid envelope, or `null` when the event
 * is valid. Precedence: `unsupported_version`, `out_of_range`, `invalid_field`.
 * Only a numeric bound is `out_of_range`. `release_blocked` is the caller's, for
 * events that pass here.
 */
export function judgeEvent(e: unknown): Exclude<RejectionCode, "release_blocked"> | null {
  const version = (e as { source_schema_version?: unknown }).source_schema_version;
  if (typeof version === "number" && Number.isInteger(version) && !SUPPORTED_SOURCE_VERSIONS.includes(version)) {
    return "unsupported_version";
  }
  if (event.validate(e).valid) return null;
  const bound = eventAllErrors.validate(e).errors.some(isNumericBound);
  return bound ? "out_of_range" : "invalid_field";
}

function isNumericBound(u: OutputUnit): boolean {
  return ["minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum"].includes(u.keyword);
}

/** Whether `body` is a response the contract allows; tests hold the Worker to it. */
export function isValidResponse(body: unknown): boolean {
  return response.validate(body).valid;
}

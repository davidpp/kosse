// The one typed client for Kossé's API. Reads go through TanStack Query (see snapshot.ts);
// every unsafe request goes through `post`, which adds the JSON content type and the per-instance
// action token. The browser adds the same-origin `Origin` header itself.
import { queryOptions, type QueryClient } from "@tanstack/react-query";

import type {
  ActionRequest,
  ActionResult,
  ApiError,
  ApiErrorCode,
  HttpProbe,
  Preferences,
  ProbeRequest,
  SavedProfile,
  SaveProfileRequest,
  Session,
  Snapshot,
} from "@/generated/contracts";

/** Why a request did not produce the expected answer. */
export type ApiFailure =
  /** Kossé answered with an `ApiError` (or a status without one). */
  | { kind: "refused"; status: number; code: ApiErrorCode | null; message: string }
  /** No answer at all: the server is down or the connection dropped mid-request. */
  | { kind: "unreachable"; message: string }
  /** An answer this page cannot read (a different build, say). */
  | { kind: "unexpected"; message: string };

export class ApiRequestError extends Error {
  readonly failure: ApiFailure;

  constructor(failure: ApiFailure) {
    super(failure.message);
    this.failure = failure;
  }
}

export const sessionQuery = queryOptions({
  queryKey: ["session"],
  queryFn: async ({ signal }): Promise<Session> => {
    const body = await readJson(await send("/api/session", { signal }));
    if (!isRecord(body) || typeof body.token !== "string") {
      throw new ApiRequestError({ kind: "unexpected", message: "unexpected session payload" });
    }
    return { token: body.token };
  },
  staleTime: Number.POSITIVE_INFINITY,
});

/**
 * POSTs JSON with the action token. A `tokenRequired` refusal means the token is stale (Kossé
 * restarted); the request is sent once more with a fresh one.
 */
async function post(client: QueryClient, path: string, body: unknown): Promise<Response> {
  const response = await postWithToken(client, path, body);
  if (response.status !== 403) return response;
  const answer = await response
    .clone()
    .json()
    .catch(() => null);
  if (!isApiError(answer) || answer.error !== "tokenRequired") return response;
  client.removeQueries({ queryKey: sessionQuery.queryKey });
  return postWithToken(client, path, body);
}

async function postWithToken(client: QueryClient, path: string, body: unknown): Promise<Response> {
  const { token } = await client.fetchQuery(sessionQuery);
  return send(path, {
    method: "POST",
    headers: { "Content-Type": "application/json", "X-Kosse-Token": token },
    body: JSON.stringify(body),
  });
}

export async function refresh(client: QueryClient): Promise<Snapshot> {
  const response = await post(client, "/api/refresh", {});
  const body = await expectOk(response);
  if (!isSnapshot(body)) throw unexpected("refresh");
  return body;
}

export async function probe(client: QueryClient, request: ProbeRequest): Promise<HttpProbe> {
  const body = await expectOk(await post(client, "/api/probe", request));
  if (!isHttpProbe(body)) throw unexpected("probe");
  return body;
}

/**
 * Stop (`SIGTERM`) or force (`SIGKILL`). Kossé answers every request that passes its guard with
 * an `ActionResult`, whatever the status; anything else is a refusal before any check ran.
 */
export async function act(
  client: QueryClient,
  kind: "stop" | "force",
  request: ActionRequest,
): Promise<ActionResult> {
  const response = await post(client, `/api/${kind}`, request);
  const body = await readJson(response);
  if (isActionResult(body)) return body;
  throw refusal(response.status, body);
}

export async function saveProfile(
  client: QueryClient,
  request: SaveProfileRequest,
): Promise<SavedProfile> {
  const body = await expectOk(await post(client, "/api/profile", request));
  if (!isSavedProfile(body)) throw unexpected("profile");
  return body;
}

/** The saved dashboard preferences; a configuration Kossé cannot use is a refusal, not retried. */
export const preferencesQuery = queryOptions({
  queryKey: ["preferences"],
  queryFn: async ({ signal }): Promise<Preferences> =>
    asPreferences(await expectOk(await send("/api/preferences", { signal }))),
  staleTime: Number.POSITIVE_INFINITY,
  retry: false,
});

export async function savePreferences(
  client: QueryClient,
  preferences: Preferences,
): Promise<Preferences> {
  return asPreferences(await expectOk(await post(client, "/api/preferences", preferences)));
}

function asPreferences(body: unknown): Preferences {
  if (!isRecord(body) || typeof body.allListeners !== "boolean") throw unexpected("preferences");
  return { allListeners: body.allListeners };
}

export async function fetchSnapshot(signal: AbortSignal): Promise<Snapshot> {
  const body = await expectOk(await send("/api/snapshot", { signal }));
  if (!isSnapshot(body)) throw unexpected("snapshot");
  return body;
}

/** The snapshot schema this page reads (`SNAPSHOT_SCHEMA_VERSION` in crates/core). */
export const SCHEMA_VERSION = 2;

/**
 * Types are generated from Rust, but an answer can still come from another build. This checks
 * the envelope the page relies on; the rest is the generated contract.
 */
export function isSnapshot(value: unknown): value is Snapshot {
  return (
    isRecord(value) &&
    value.schemaVersion === SCHEMA_VERSION &&
    typeof value.instance === "string" &&
    typeof value.revision === "number" &&
    isRecord(value.scanner) &&
    (value.observation === null || isRecord(value.observation))
  );
}

async function send(path: string, init: RequestInit): Promise<Response> {
  try {
    return await fetch(path, { ...init, cache: "no-store" });
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") throw error;
    throw new ApiRequestError({ kind: "unreachable", message: "Kossé did not answer" });
  }
}

async function readJson(response: Response): Promise<unknown> {
  try {
    return await response.json();
  } catch {
    return null;
  }
}

async function expectOk(response: Response): Promise<unknown> {
  const body = await readJson(response);
  if (!response.ok) throw refusal(response.status, body);
  return body;
}

function refusal(status: number, body: unknown): ApiRequestError {
  return new ApiRequestError(
    isApiError(body)
      ? { kind: "refused", status, code: body.error, message: body.message }
      : { kind: "refused", status, code: null, message: `HTTP ${status}` },
  );
}

function unexpected(what: string): ApiRequestError {
  return new ApiRequestError({
    kind: "unexpected",
    message: `the ${what} answer is not in the format this page expects; reload the page`,
  });
}

const PROBE_STATES: readonly string[] = [
  "skipped",
  "pending",
  "probed",
] satisfies HttpProbe["state"][];
const OUTCOMES: readonly string[] = [
  "exited",
  "stillRunning",
  "respawned",
  "endpointTaken",
  "observationFailed",
  "stale",
  "forceNotOffered",
  "refused",
  "permissionDenied",
  "signalFailed",
  "internal",
] satisfies ActionResult["outcome"][];

function isHttpProbe(value: unknown): value is HttpProbe {
  return isRecord(value) && typeof value.state === "string" && PROBE_STATES.includes(value.state);
}

function isActionResult(value: unknown): value is ActionResult {
  return isRecord(value) && typeof value.outcome === "string" && OUTCOMES.includes(value.outcome);
}

function isSavedProfile(value: unknown): value is SavedProfile {
  return (
    isRecord(value) &&
    typeof value.root === "string" &&
    (value.profile === null || isRecord(value.profile))
  );
}

function isApiError(value: unknown): value is ApiError {
  return isRecord(value) && typeof value.error === "string" && typeof value.message === "string";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

export function failureOf(error: unknown): ApiFailure {
  return error instanceof ApiRequestError
    ? error.failure
    : { kind: "unexpected", message: error instanceof Error ? error.message : String(error) };
}

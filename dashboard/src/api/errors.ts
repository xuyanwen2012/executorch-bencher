// Normalizes an openapi-fetch call into either data, the backend's JSON
// error envelope, or an "unreachable" failure when no HTTP response
// arrived. See specs/benchmark-dashboard - "Dashboard surfaces backend
// errors and unreachability clearly".

export type ApiFailure =
  | { kind: "api"; status: number; code: string; message: string }
  | { kind: "unreachable"; message: string };

export class ApiError extends Error {
  readonly failure: ApiFailure;

  constructor(failure: ApiFailure) {
    super(failure.kind === "api" ? `${failure.code}: ${failure.message}` : failure.message);
    this.name = "ApiError";
    this.failure = failure;
  }
}

/** The minimal shape of an openapi-fetch result we rely on. */
export interface FetchResult<T> {
  data?: T;
  error?: unknown;
  response: Response;
}

interface Envelope {
  error?: { code?: unknown; message?: unknown };
}

function envelopeOf(error: unknown): { code: string; message: string } | null {
  if (typeof error !== "object" || error === null) return null;
  const body = (error as Envelope).error;
  if (typeof body !== "object" || body === null) return null;
  const { code, message } = body;
  if (typeof code !== "string") return null;
  return { code, message: typeof message === "string" ? message : "" };
}

/**
 * Awaits an openapi-fetch call and returns its data, or throws an
 * `ApiError` carrying a normalized failure:
 * - a backend error envelope becomes `{ kind: "api", status, code, message }`
 *   (a non-envelope error body becomes code `http_<status>`),
 * - a thrown fetch (no HTTP response at all) becomes `{ kind: "unreachable" }`.
 */
export async function unwrap<T>(call: Promise<FetchResult<T>>): Promise<T> {
  let result: FetchResult<T>;
  try {
    result = await call;
  } catch (err) {
    throw new ApiError({
      kind: "unreachable",
      message: `backend could not be reached: ${err instanceof Error ? err.message : String(err)}`,
    });
  }
  if (result.error !== undefined || result.data === undefined) {
    const status = result.response.status;
    const envelope = envelopeOf(result.error);
    throw new ApiError(
      envelope
        ? { kind: "api", status, ...envelope }
        : { kind: "api", status, code: `http_${status}`, message: result.response.statusText || "request failed" },
    );
  }
  return result.data;
}

/** Narrow an unknown thrown value to a normalized failure for display. */
export function failureOf(err: unknown): ApiFailure {
  if (err instanceof ApiError) return err.failure;
  return { kind: "unreachable", message: err instanceof Error ? err.message : String(err) };
}

export function isNotFound(err: unknown): boolean {
  const failure = failureOf(err);
  return failure.kind === "api" && (failure.code === "not_found" || failure.status === 404);
}

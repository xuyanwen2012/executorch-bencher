import { describe, expect, test } from "bun:test";
import { ApiError, failureOf, isNotFound, unwrap } from "./errors";

function result<T>(data: T | undefined, error: unknown, status: number) {
  return Promise.resolve({ data, error, response: new Response(null, { status }) });
}

describe("unwrap", () => {
  test("returns data on success", async () => {
    await expect(unwrap(result({ ok: 1 }, undefined, 200))).resolves.toEqual({ ok: 1 });
  });

  test("turns the backend envelope into an api failure", async () => {
    const call = result(undefined, { error: { code: "not_found", message: "run not found" } }, 404);
    const err = await unwrap(call).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect((err as ApiError).failure).toEqual({
      kind: "api",
      status: 404,
      code: "not_found",
      message: "run not found",
    });
    expect(isNotFound(err)).toBe(true);
  });

  test("a non-envelope error body still becomes an api failure", async () => {
    const err = await unwrap(result(undefined, "oops", 502)).catch((e: unknown) => e);
    expect(failureOf(err)).toEqual({ kind: "api", status: 502, code: "http_502", message: "request failed" });
    expect(isNotFound(err)).toBe(false);
  });

  test("a thrown fetch becomes an unreachable failure", async () => {
    const err = await unwrap(Promise.reject(new TypeError("fetch failed"))).catch((e: unknown) => e);
    const failure = failureOf(err);
    expect(failure.kind).toBe("unreachable");
    expect(failure.message).toContain("fetch failed");
    expect(isNotFound(err)).toBe(false);
  });
});

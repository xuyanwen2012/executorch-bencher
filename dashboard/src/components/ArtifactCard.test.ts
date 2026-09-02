import { describe, expect, test } from "bun:test";
import { INLINE_PREVIEW_MAX_BYTES, canViewInline } from "./ArtifactCard";

describe("canViewInline", () => {
  test("small text artifacts are viewable", () => {
    expect(canViewInline({ available: true, media_type: "text/plain", size_bytes: 1024 })).toBe(true);
    expect(canViewInline({ available: true, media_type: "Text/Plain; charset=utf-8", size_bytes: INLINE_PREVIEW_MAX_BYTES })).toBe(true);
  });
  test("large, binary, untyped, or unavailable artifacts are download-only", () => {
    expect(canViewInline({ available: true, media_type: "text/plain", size_bytes: INLINE_PREVIEW_MAX_BYTES + 1 })).toBe(false);
    expect(canViewInline({ available: true, media_type: "application/octet-stream", size_bytes: 10 })).toBe(false);
    expect(canViewInline({ available: true, media_type: null, size_bytes: 10 })).toBe(false);
    expect(canViewInline({ available: false, media_type: "text/plain", size_bytes: 10 })).toBe(false);
  });
});

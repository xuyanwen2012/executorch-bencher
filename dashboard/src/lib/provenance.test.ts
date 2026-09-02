import { describe, expect, test } from "bun:test";
import { gitNotes, modifiedFiles } from "./provenance";

describe("modifiedFiles", () => {
  test("lifts the importer's file list out of input_parameters", () => {
    expect(modifiedFiles({ git_modified_files: ["src/a.cpp", "src/b.h"], backend: "vulkan" })).toEqual([
      "src/a.cpp",
      "src/b.h",
    ]);
  });

  test("anything that is not a non-empty list of strings is absent", () => {
    expect(modifiedFiles({ git_modified_files: [] })).toBeNull();
    expect(modifiedFiles({ git_modified_files: "src/a.cpp" })).toBeNull();
    expect(modifiedFiles({ backend: "vulkan" })).toBeNull();
    expect(modifiedFiles(null)).toBeNull();
    expect(modifiedFiles("not an object")).toBeNull();
  });

  test("non-string entries are dropped rather than rendered as [object Object]", () => {
    expect(modifiedFiles({ git_modified_files: ["a", 3, null] })).toEqual(["a"]);
  });
});

describe("gitNotes", () => {
  test("returns a non-empty note only", () => {
    expect(gitNotes({ git_notes: "cherry-picked fix" })).toBe("cherry-picked fix");
    expect(gitNotes({ git_notes: "" })).toBeNull();
    expect(gitNotes({})).toBeNull();
    expect(gitNotes(undefined)).toBeNull();
  });
});

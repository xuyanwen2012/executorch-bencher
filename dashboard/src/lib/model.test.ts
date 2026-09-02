import { describe, expect, test } from "bun:test";
import { parseModelName } from "./model";

describe("parseModelName", () => {
  test("splits the exporter's naming convention into identity and qualifiers", () => {
    expect(parseModelName("llama3_2-1b_vulkan_8da4w.pte")).toEqual({
      label: "llama3_2-1b_vulkan_8da4w",
      identity: "llama3.2 1B",
      qualifiers: "vulkan · 8da4w",
    });
    expect(parseModelName("llama3_2-3b_vulkan_4w.pte")).toEqual({
      label: "llama3_2-3b_vulkan_4w",
      identity: "llama3.2 3B",
      qualifiers: "vulkan · 4w",
    });
  });

  test("a family without a minor version, and a model with no qualifiers", () => {
    expect(parseModelName("phi-3b.pte")).toEqual({ label: "phi-3b", identity: "phi 3B", qualifiers: "" });
  });

  test("a name outside the convention keeps its label intact", () => {
    for (const name of ["e2e-example-model.pte", "llama3_1_8b_4w.pte", "weights.bin"]) {
      const parsed = parseModelName(name);
      expect(parsed.identity).toBe(parsed.label);
      expect(parsed.qualifiers).toBe("");
    }
    expect(parseModelName("e2e-example-model.pte").label).toBe("e2e-example-model");
  });
});

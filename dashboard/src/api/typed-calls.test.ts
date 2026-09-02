// Compile-time check that the generated types cover the three operations
// the pages depend on; the calls are typed but never executed.
import { test } from "bun:test";
import { api } from "./client";

test("typed calls compile against the generated contract", () => {
  const calls = () => [
    api.GET("/api/v1/results", { params: { query: { device_serial: "x" } } }),
    api.GET("/api/v1/runs", { params: { query: { limit: 50, cursor: undefined } } }),
    api.GET("/api/v1/runs/{id}", { params: { path: { id: "00000000-0000-0000-0000-000000000000" } } }),
  ];
  void calls;
});

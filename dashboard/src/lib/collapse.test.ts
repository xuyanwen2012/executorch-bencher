import { describe, expect, test } from "bun:test";
import { collapseColumns } from "./collapse";

interface Row {
  bsp: string;
  driver: string;
  gpu: number;
}
const columns = [
  { key: "bsp", label: "BSP", value: (r: Row) => r.bsp },
  { key: "driver", label: "Driver", value: (r: Row) => r.driver },
  { key: "gpu", label: "GPU", value: (r: Row) => String(r.gpu) },
] as const;

describe("collapseColumns", () => {
  test("all-equal columns collapse into the shared line", () => {
    const rows: Row[] = [
      { bsp: "1", driver: "a", gpu: 980 },
      { bsp: "1", driver: "a", gpu: 980 },
    ];
    expect(collapseColumns(rows, columns)).toEqual({
      varying: [],
      shared: [
        { key: "bsp", label: "BSP", value: "1" },
        { key: "driver", label: "Driver", value: "a" },
        { key: "gpu", label: "GPU", value: "980" },
      ],
    });
  });

  test("a column that varies stays in the table", () => {
    const rows: Row[] = [
      { bsp: "1", driver: "a", gpu: 980 },
      { bsp: "1", driver: "b", gpu: 980 },
    ];
    const result = collapseColumns(rows, columns);
    expect(result.varying).toEqual(["driver"]);
    expect(result.shared.map((s) => s.key)).toEqual(["bsp", "gpu"]);
  });

  test("no rows leaves every column visible and nothing shared", () => {
    expect(collapseColumns([], columns)).toEqual({ varying: ["bsp", "driver", "gpu"], shared: [] });
  });

  test("a single row collapses everything", () => {
    const result = collapseColumns([{ bsp: "2", driver: "z", gpu: 1 }], columns);
    expect(result.varying).toEqual([]);
    expect(result.shared).toHaveLength(3);
  });
});

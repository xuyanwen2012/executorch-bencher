import { describe, expect, test } from "bun:test";
import { click, keydown, mount, tick, type, select, waitFor } from "../../tests/render";
import { FilterBar, type FilterField } from "./FilterBar";

type Key = "device_serial" | "platform";

const FIELDS: readonly FilterField<Key>[] = [
  { key: "device_serial", label: "Host", placeholder: "serial" },
  {
    key: "platform",
    label: "Platform",
    options: [
      { value: "android", label: "android" },
      { value: "linux", label: "linux" },
    ],
  },
];

function harness(values: Partial<Record<Key, string>>, debounceMs = 60) {
  const calls: [Key, string][] = [];
  const element = (
    <FilterBar
      fields={FIELDS}
      values={values}
      onChange={(key, value) => calls.push([key, value])}
      onClear={() => calls.push(["platform", "<clear>"])}
      status="3 runs"
      debounceMs={debounceMs}
    />
  );
  return { calls, element };
}

describe("FilterBar", () => {
  test("a typed filter is committed once, after the debounce window", async () => {
    const { calls, element } = harness({});
    const m = await mount(element);
    const input = m.container.querySelector<HTMLInputElement>("#filter-device_serial")!;
    await type(input, "R5");
    await type(input, "R5C");
    await type(input, "R5CX");
    expect(input.value).toBe("R5CX");
    expect(calls).toEqual([]);
    await tick(30);
    expect(calls).toEqual([]);
    await waitFor(() => calls.length === 1);
    expect(calls).toEqual([["device_serial", "R5CX"]]);
    // The draft stays visible until the parent's value catches up.
    expect(input.value).toBe("R5CX");
    await m.rerender(harness({ device_serial: "R5CX" }).element);
    expect(input.value).toBe("R5CX");
    await m.unmount();
  });

  test("Enter commits a typed filter without waiting", async () => {
    const { calls, element } = harness({}, 10_000);
    const m = await mount(element);
    const input = m.container.querySelector<HTMLInputElement>("#filter-device_serial")!;
    await type(input, "box-a");
    expect(calls).toEqual([]);
    await keydown(input, "Enter");
    expect(calls).toEqual([["device_serial", "box-a"]]);
    await m.unmount();
  });

  test("a select commits immediately", async () => {
    const { calls, element } = harness({});
    const m = await mount(element);
    const control = m.container.querySelector<HTMLSelectElement>("#filter-platform")!;
    await select(control, "linux");
    expect(calls).toEqual([["platform", "linux"]]);
    await m.unmount();
  });

  test("removing a chip clears that filter at once", async () => {
    const { calls, element } = harness({ device_serial: "box-a", platform: "linux" });
    const m = await mount(element);
    expect(m.container.textContent).toContain("2 active");
    const remove = m.container.querySelector('button[aria-label="Remove the Host filter"]')!;
    await click(remove);
    expect(calls).toEqual([["device_serial", ""]]);
    await m.unmount();
  });

  test("clear all drops a pending draft instead of committing it later", async () => {
    const { calls, element } = harness({ platform: "linux" }, 30);
    const m = await mount(element);
    const input = m.container.querySelector<HTMLInputElement>("#filter-device_serial")!;
    await type(input, "half-typed");
    const clear = Array.from(m.container.querySelectorAll("button")).find((b) => b.textContent === "Clear all")!;
    await click(clear);
    await tick(80);
    expect(calls).toEqual([["platform", "<clear>"]]);
    expect(input.value).toBe("");
    await m.unmount();
  });

  test("the result count is a polite live region", async () => {
    const { element } = harness({});
    const m = await mount(element);
    const status = m.container.querySelector('[data-testid="filter-status"]')!;
    expect(status.getAttribute("aria-live")).toBe("polite");
    expect(status.textContent).toBe("3 runs");
    await m.unmount();
  });
});

import { describe, expect, test } from "bun:test";
import { renderToString } from "react-dom/server";
import {
  CorrectnessBadge,
  DeviceClassBadge,
  DirtyBadge,
  ExitBadge,
  ThrottledBadge,
  correctnessTone,
  exitTone,
} from "./Status";

describe("Status", () => {
  test("only the expected outcome is quiet", () => {
    expect(exitTone("succeeded")).toEqual({ tone: "ok", plain: true });
    expect(exitTone("crashed")).toEqual({ tone: "danger", plain: false });
    expect(exitTone("infrastructure_error")).toEqual({ tone: "danger", plain: false });
    expect(correctnessTone("passed")).toEqual({ tone: "ok", plain: true });
    expect(correctnessTone("not_checked")).toEqual({ tone: "neutral", plain: true });
    expect(correctnessTone("failed")).toEqual({ tone: "danger", plain: false });
    expect(correctnessTone("validator_error")).toEqual({ tone: "warn", plain: false });
  });

  test("badges spell tokens out and explain themselves on hover", () => {
    const exit = renderToString(<ExitBadge status="infrastructure_error" />);
    expect(exit).toContain(">infrastructure error<");
    expect(exit).toContain('title="Process exit status: infrastructure error"');
    const correctness = renderToString(<CorrectnessBadge result="validator_error" />);
    expect(correctness).toContain(">validator error<");
    expect(renderToString(<ThrottledBadge />)).toContain(">throttled<");
    expect(renderToString(<DirtyBadge />)).toContain(">dirty<");
  });

  test("the external device class is the marked case", () => {
    const external = renderToString(<DeviceClassBadge deviceClass="external" />);
    expect(external).toContain("border-dashed");
    expect(external).toContain(">external<");
    const internal = renderToString(<DeviceClassBadge deviceClass="internal" />);
    expect(internal).not.toContain("border-dashed");
    expect(internal).toContain(">internal<");
  });
});

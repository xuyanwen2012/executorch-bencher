import { describe, expect, test } from "bun:test";
import { createCoalescer, parseRunCreated } from "./live";

/** Deterministic fake timers: runs callbacks only when `advance` passes their due time. */
function fakeTimers() {
  let now = 0;
  let next = 1;
  const due = new Map<number, { at: number; fn: () => void }>();
  return {
    timers: {
      setTimeout: ((fn: () => void, ms: number) => {
        const id = next++;
        due.set(id, { at: now + ms, fn });
        return id as unknown as ReturnType<typeof setTimeout>;
      }) as typeof setTimeout,
      clearTimeout: ((id: ReturnType<typeof setTimeout>) => {
        due.delete(id as unknown as number);
      }) as typeof clearTimeout,
    },
    advance(ms: number) {
      now += ms;
      for (const [id, entry] of [...due]) {
        if (entry.at <= now) {
          due.delete(id);
          entry.fn();
        }
      }
    },
  };
}

describe("live updates", () => {
  test("a burst of events coalesces into one refresh", () => {
    const clock = fakeTimers();
    let calls = 0;
    const c = createCoalescer(500, () => calls++, clock.timers);
    for (let i = 0; i < 18; i++) {
      c.trigger();
      clock.advance(100);
    }
    expect(calls).toBe(0);
    expect(c.pending).toBe(18);
    clock.advance(500);
    expect(calls).toBe(1);
    expect(c.pending).toBe(0);
    c.trigger();
    clock.advance(500);
    expect(calls).toBe(2);
  });

  test("cancel drops a pending refresh", () => {
    const clock = fakeTimers();
    let calls = 0;
    const c = createCoalescer(500, () => calls++, clock.timers);
    c.trigger();
    c.cancel();
    clock.advance(1000);
    expect(calls).toBe(0);
  });

  test("run.created payloads parse and junk does not", () => {
    const payload = {
      id: "01a0-run",
      device_serial: "box-a",
      platform: "linux",
      device_class: "external",
      prefill_tokens_per_sec: 385.6,
    };
    expect(parseRunCreated(JSON.stringify(payload))?.id).toBe("01a0-run");
    expect(parseRunCreated("not json")).toBeNull();
    expect(parseRunCreated(JSON.stringify({ kind: "stdout" }))).toBeNull();
  });
});

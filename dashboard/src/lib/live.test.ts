import { describe, expect, test } from "bun:test";
import {
  type EventSourceLike,
  createCoalescer,
  openLiveStream,
  parseRunCreated,
  reconnectDelay,
  trimToFirstPage,
} from "./live";

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

describe("reconnect backoff", () => {
  test("doubles from one second and caps at thirty", () => {
    expect([0, 1, 2, 3, 4, 5, 6, 20].map((n) => reconnectDelay(n))).toEqual([
      1000, 2000, 4000, 8000, 16000, 30000, 30000, 30000,
    ]);
    expect(reconnectDelay(-3)).toBe(1000);
    expect(reconnectDelay(2, 100, 250)).toBe(250);
  });

  /** A scriptable EventSource stand-in. */
  class FakeSource implements EventSourceLike {
    readyState = 0;
    onopen: ((event: Event) => void) | null = null;
    onerror: ((event: Event) => void) | null = null;
    closedCalls = 0;
    private listeners = new Map<string, ((event: MessageEvent<string>) => void)[]>();
    addEventListener(type: string, listener: (event: MessageEvent<string>) => void) {
      this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
    }
    close() {
      this.closedCalls += 1;
      this.readyState = 2;
    }
    open() {
      this.readyState = 1;
      this.onopen?.(new Event("open"));
    }
    fail(readyState: number) {
      this.readyState = readyState;
      this.onerror?.(new Event("error"));
    }
    emit(type: string, data: string) {
      for (const l of this.listeners.get(type) ?? []) l({ data } as MessageEvent<string>);
    }
  }
  const fakeSource = () => new FakeSource();

  test("a CLOSED stream is reopened on the backoff schedule and the schedule resets on open", () => {
    const clock = fakeTimers();
    const sources: ReturnType<typeof fakeSource>[] = [];
    const states: string[] = [];
    const stream = openLiveStream({
      connect: () => {
        const s = fakeSource();
        sources.push(s);
        return s;
      },
      onState: (s) => states.push(s),
      onRunCreated: () => {},
      timers: clock.timers,
    });
    expect(sources).toHaveLength(1);
    expect(states).toEqual(["connecting"]);

    // The browser's own retry: still connecting, nothing to do.
    sources[0]!.fail(0);
    expect(states.at(-1)).toBe("connecting");
    expect(sources).toHaveLength(1);

    // Closed for good: off now, a new source after 1 s, not before.
    sources[0]!.fail(2);
    expect(states.at(-1)).toBe("off");
    expect(stream.attempts).toBe(1);
    clock.advance(999);
    expect(sources).toHaveLength(1);
    clock.advance(1);
    expect(sources).toHaveLength(2);
    expect(states.at(-1)).toBe("connecting");

    // Second failure waits 2 s, third 4 s.
    sources[1]!.fail(2);
    clock.advance(1999);
    expect(sources).toHaveLength(2);
    clock.advance(1);
    expect(sources).toHaveLength(3);
    sources[2]!.fail(2);
    clock.advance(3999);
    expect(sources).toHaveLength(3);
    clock.advance(1);
    expect(sources).toHaveLength(4);
    expect(stream.attempts).toBe(3);

    // A successful open resets the schedule to 1 s.
    sources[3]!.open();
    expect(states.at(-1)).toBe("live");
    expect(stream.attempts).toBe(0);
    sources[3]!.fail(2);
    clock.advance(1000);
    expect(sources).toHaveLength(5);
    stream.close();
  });

  test("close cancels a pending reopen and closes the live source", () => {
    const clock = fakeTimers();
    const sources: ReturnType<typeof fakeSource>[] = [];
    const stream = openLiveStream({
      connect: () => {
        const s = fakeSource();
        sources.push(s);
        return s;
      },
      onState: () => {},
      onRunCreated: () => {},
      timers: clock.timers,
    });
    sources[0]!.fail(2);
    stream.close();
    clock.advance(60_000);
    expect(sources).toHaveLength(1);

    const again = openLiveStream({
      connect: () => {
        const s = fakeSource();
        sources.push(s);
        return s;
      },
      onState: () => {},
      onRunCreated: () => {},
      timers: clock.timers,
    });
    sources[1]!.open();
    again.close();
    expect(sources[1]!.closedCalls).toBe(1);
  });

  test("run.created payloads reach the callback; junk does not", () => {
    const clock = fakeTimers();
    let source!: ReturnType<typeof fakeSource>;
    const seen: string[] = [];
    const stream = openLiveStream({
      connect: () => (source = fakeSource()),
      onState: () => {},
      onRunCreated: (event) => seen.push(event.id),
      timers: clock.timers,
    });
    source.emit("run.created", JSON.stringify({ id: "r-1", device_serial: "box" }));
    source.emit("run.created", "nope");
    expect(seen).toEqual(["r-1"]);
    stream.close();
  });
});

describe("trimToFirstPage", () => {
  test("keeps only the head page and its cursor", () => {
    const data = { pages: [{ items: [1] }, { items: [2] }, { items: [3] }], pageParams: [undefined, "c1", "c2"] };
    expect(trimToFirstPage(data)).toEqual({ pages: [{ items: [1] }], pageParams: [undefined] });
    const single = { pages: [{ items: [1] }], pageParams: [undefined] };
    expect(trimToFirstPage(single)).toBe(single);
    expect(trimToFirstPage(undefined)).toBeUndefined();
  });
});

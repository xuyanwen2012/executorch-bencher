// Live updates from the backend's Server-Sent Events stream. The stream is
// a signal, not state: on `run.created` the pages' queries are invalidated
// so TanStack Query re-fetches them, keeping the current data (and the
// user's filters, paging, and scroll) on screen until the fresh page
// arrives. See specs/benchmark-dashboard - "Dashboard refreshes live from
// the event stream".
import { type InfiniteData, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import type { components } from "../api/client";

export type RunCreated = components["schemas"]["RunCreatedEvent"];
export type LiveState = "connecting" | "live" | "off";

type Timers = Pick<typeof globalThis, "setTimeout" | "clearTimeout">;

/** Coalesces a burst of calls into one trailing callback per `delayMs`
 * window, so eighteen runs landing in a few seconds refresh once. */
export function createCoalescer(delayMs: number, callback: () => void, timers: Timers = globalThis) {
  let handle: ReturnType<typeof setTimeout> | undefined;
  let pending = 0;
  return {
    trigger() {
      pending += 1;
      if (handle !== undefined) timers.clearTimeout(handle);
      handle = timers.setTimeout(() => {
        handle = undefined;
        pending = 0;
        callback();
      }, delayMs);
    },
    /** Calls waiting for the window to close. */
    get pending() {
      return pending;
    },
    cancel() {
      if (handle !== undefined) timers.clearTimeout(handle);
      handle = undefined;
      pending = 0;
    },
  };
}

/** Parses a `run.created` payload; null when it is not one. */
export function parseRunCreated(data: string): RunCreated | null {
  try {
    const value = JSON.parse(data) as Partial<RunCreated>;
    if (typeof value === "object" && value !== null && typeof value.id === "string" && typeof value.device_serial === "string") {
      return value as RunCreated;
    }
    return null;
  } catch {
    return null;
  }
}

// ---- Reopening a closed stream ---------------------------------------------
//
// `EventSource` retries on its own only while it is CONNECTING. Once it
// reaches CLOSED (the server refused, a proxy dropped the response, the
// browser gave up) it stays closed for good, so the page has to open a new
// one. Attempts back off exponentially so a backend that is down for an
// hour is polled every 30 s, not every second.

export const RECONNECT_BASE_MS = 1_000;
export const RECONNECT_CAP_MS = 30_000;

/** Delay before reopen `attempt` (0-based): 1 s, 2 s, 4 s, ... capped at 30 s. */
export function reconnectDelay(attempt: number, baseMs = RECONNECT_BASE_MS, capMs = RECONNECT_CAP_MS): number {
  const exponent = Math.max(0, Math.floor(attempt));
  // 2 ** exponent overflows to Infinity for a very long outage; Math.min copes.
  return Math.min(capMs, baseMs * 2 ** exponent);
}

/** The `EventSource.readyState` constants, spelled out so the state machine
 * needs no browser global. */
const CONNECTING = 0;
const CLOSED = 2;

/** The subset of `EventSource` the state machine drives. */
export interface EventSourceLike {
  readonly readyState: number;
  onopen: ((event: Event) => void) | null;
  onerror: ((event: Event) => void) | null;
  addEventListener(type: string, listener: (event: MessageEvent<string>) => void): void;
  close(): void;
}

export interface LiveStreamOptions {
  /** Opens a fresh stream; called once up front and again after every CLOSED. */
  connect: () => EventSourceLike;
  onState: (state: LiveState) => void;
  onRunCreated: (event: RunCreated) => void;
  timers?: Timers;
  baseMs?: number;
  capMs?: number;
}

export interface LiveStream {
  /** Consecutive failed attempts since the stream was last open. */
  readonly attempts: number;
  /** Closes the current stream and cancels any pending reopen. */
  close(): void;
}

/**
 * Keeps one stream open for as long as it can: mirrors the browser's own
 * CONNECTING retries as `connecting`, and when the source reaches CLOSED
 * reports `off`, waits `reconnectDelay(attempts)`, and connects again. A
 * successful open resets the backoff.
 */
export function openLiveStream(options: LiveStreamOptions): LiveStream {
  const { connect, onState, onRunCreated, timers = globalThis, baseMs, capMs } = options;
  let source: EventSourceLike | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let attempts = 0;
  let closed = false;

  const open = () => {
    timer = undefined;
    if (closed) return;
    onState("connecting");
    const next = connect();
    source = next;
    next.onopen = () => {
      if (closed || source !== next) return;
      attempts = 0;
      onState("live");
    };
    next.onerror = () => {
      if (closed || source !== next) return;
      if (next.readyState === CONNECTING) {
        onState("connecting");
        return;
      }
      if (next.readyState !== CLOSED) return;
      next.close();
      source = undefined;
      onState("off");
      timer = timers.setTimeout(open, reconnectDelay(attempts, baseMs, capMs));
      attempts += 1;
    };
    next.addEventListener("run.created", (event) => {
      if (closed || source !== next) return;
      const parsed = parseRunCreated(event.data);
      if (parsed) onRunCreated(parsed);
    });
  };

  open();

  return {
    get attempts() {
      return attempts;
    },
    close() {
      closed = true;
      if (timer !== undefined) timers.clearTimeout(timer);
      timer = undefined;
      source?.close();
      source = undefined;
    },
  };
}

// ---- Query invalidation ----------------------------------------------------

/** The query keys a new run can change. */
export const LIVE_QUERY_KEYS: readonly string[] = ["results", "runs"];

/**
 * Drops every page after the first from an infinite query's cached data, so
 * the invalidation that follows refetches one page instead of walking every
 * loaded cursor. A new run lands at the head of the list; the reader sees
 * it at once and pages forward again from a fresh cursor chain.
 */
export function trimToFirstPage<TData, TParam>(
  data: InfiniteData<TData, TParam> | undefined,
): InfiniteData<TData, TParam> | undefined {
  if (!data || data.pages.length <= 1) return data;
  return { pages: data.pages.slice(0, 1), pageParams: data.pageParams.slice(0, 1) };
}

/**
 * Subscribes to `/api/v1/events` for the lifetime of the component and
 * invalidates the results and runs queries (coalesced) on `run.created`.
 * The returned state reads "off" when the browser has no EventSource or
 * the stream is down and waiting to reopen.
 */
export function useLiveEvents(delayMs = 500): LiveState {
  const queryClient = useQueryClient();
  const [state, setState] = useState<LiveState>("connecting");

  useEffect(() => {
    if (typeof EventSource === "undefined") {
      setState("off");
      return;
    }
    const refresh = createCoalescer(delayMs, () => {
      queryClient.setQueriesData<InfiniteData<unknown, unknown>>({ queryKey: ["runs"] }, trimToFirstPage);
      for (const key of LIVE_QUERY_KEYS) void queryClient.invalidateQueries({ queryKey: [key] });
    });
    const stream = openLiveStream({
      connect: () => new EventSource("/api/v1/events"),
      onState: setState,
      onRunCreated: () => refresh.trigger(),
    });
    return () => {
      refresh.cancel();
      stream.close();
    };
  }, [queryClient, delayMs]);

  return state;
}

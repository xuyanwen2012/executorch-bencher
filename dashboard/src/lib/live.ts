// Live updates from the backend's Server-Sent Events stream. The stream is
// a signal, not state: on `run.created` the pages' queries are invalidated
// so TanStack Query re-fetches them, keeping the current data (and the
// user's filters, paging, and scroll) on screen until the fresh page
// arrives. See specs/benchmark-dashboard - "Dashboard refreshes live from
// the event stream".
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import type { components } from "../api/client";

export type RunCreated = components["schemas"]["RunCreatedEvent"];
export type LiveState = "connecting" | "live" | "off";

/** Coalesces a burst of calls into one trailing callback per `delayMs`
 * window, so eighteen runs landing in a few seconds refresh once. */
export function createCoalescer(delayMs: number, callback: () => void, timers: Pick<typeof globalThis, "setTimeout" | "clearTimeout"> = globalThis) {
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

/** The query keys a new run can change. */
export const LIVE_QUERY_KEYS: readonly string[] = ["results", "runs"];

/**
 * Subscribes to `/api/v1/events` for the lifetime of the component and
 * invalidates the results and runs queries (coalesced) on `run.created`.
 * `EventSource` reconnects on its own; the returned state mirrors it and
 * reads "off" when the browser has no EventSource or the stream is down.
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
      for (const key of LIVE_QUERY_KEYS) void queryClient.invalidateQueries({ queryKey: [key] });
    });
    const source = new EventSource("/api/v1/events");
    source.onopen = () => setState("live");
    source.onerror = () => setState(source.readyState === EventSource.CONNECTING ? "connecting" : "off");
    source.addEventListener("run.created", (event) => {
      if (parseRunCreated((event as MessageEvent<string>).data)) refresh.trigger();
    });
    return () => {
      refresh.cancel();
      source.close();
    };
  }, [queryClient, delayMs]);

  return state;
}

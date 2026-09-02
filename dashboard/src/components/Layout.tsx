import { useQuery } from "@tanstack/react-query";
import { NavLink, Outlet } from "react-router";
import { api } from "../api/client";
import { unwrap } from "../api/errors";
import { type LiveState, RECONNECT_CAP_MS, useLiveEvents } from "../lib/live";

const linkClass = ({ isActive }: { isActive: boolean }) =>
  [
    "eyebrow relative -mb-px border-b-2 px-1 py-3 transition-colors",
    isActive ? "border-prefill text-ink" : "border-transparent text-ink-3 hover:border-rule-strong hover:text-ink-2",
  ].join(" ");

export function Layout() {
  const live = useLiveEvents();
  return (
    <div className="flex min-h-screen flex-col">
      <header className="border-b border-rule bg-paper">
        <div className="mx-auto flex max-w-[1600px] flex-wrap items-center gap-x-8 gap-y-1 px-4 sm:px-5">
          <span className="flex items-baseline gap-1.5 py-3">
            <span className="font-cond text-[15px] font-semibold tracking-[0.13em] text-ink uppercase">
              ExecuTorch
            </span>
            <span className="text-rule-strong">/</span>
            <span className="font-mono text-[13px] text-ink-2">bencher</span>
          </span>
          <nav className="flex gap-6 self-end">
            <NavLink to="/" end className={linkClass}>
              Results
            </NavLink>
            <NavLink to="/runs" className={linkClass}>
              Runs
            </NavLink>
          </nav>
          <LiveIndicator state={live} />
        </div>
      </header>
      <main className="mx-auto w-full max-w-[1600px] flex-1 px-4 py-6 sm:px-5">
        <Outlet />
      </main>
      <Footer />
    </div>
  );
}

const LIVE_LABEL: Record<LiveState, string> = {
  live: "live",
  connecting: "connecting",
  off: "live updates off",
};

const LIVE_TITLE: Record<LiveState, string> = {
  live: "New runs appear automatically as they are recorded.",
  connecting: "Opening the event stream; new runs will appear automatically once it is up.",
  off: `Not receiving live updates. The dashboard reopens the stream on its own, waiting longer between attempts (up to ${
    RECONNECT_CAP_MS / 1000
  } s); reload to see new runs sooner.`,
};

const LIVE_DOT: Record<LiveState, string> = {
  live: "bg-ok",
  connecting: "bg-warn",
  off: "bg-rule-strong",
};

/** Whether the page is receiving live change notifications. When the
 * stream is off, everything still works by navigation and reload, and the
 * stream is reopened with exponential backoff (see `openLiveStream`). */
export function LiveIndicator({ state }: { state: LiveState }) {
  return (
    <span
      className="ml-auto flex items-center gap-1.5 font-mono text-[11px] text-ink-3"
      title={LIVE_TITLE[state]}
      data-testid="live-indicator"
      data-state={state}
      role="status"
      aria-live="polite"
    >
      <span className={`inline-block h-1.5 w-1.5 rounded-full ${LIVE_DOT[state]}`} aria-hidden="true" />
      {LIVE_LABEL[state]}
    </span>
  );
}

/** The backend's own identity, so a reader knows which contract produced
 * the numbers on screen. When the call fails it says so briefly; the pages
 * report unreachability in full where it matters. */
function Footer() {
  const version = useQuery({
    queryKey: ["version"],
    queryFn: () => unwrap(api.GET("/api/v1/version")),
    staleTime: 5 * 60_000,
  });
  return (
    <footer className="mt-8 border-t border-rule px-4 py-4 sm:px-5">
      <div className="mx-auto flex max-w-[1600px] flex-wrap items-center gap-x-5 gap-y-1 font-mono text-xs text-ink-3">
        {version.data ? (
          <>
            <span title="Backend API contract version">api {version.data.api_version}</span>
            <span title="Database schema version">schema v{version.data.schema_version}</span>
            <span title="Server build version">server {version.data.server_version}</span>
          </>
        ) : version.isError ? (
          <span title="GET /api/v1/version failed; the backend may be unreachable." data-testid="version-unavailable">
            version unavailable
          </span>
        ) : null}
        <a href="/docs" className="ml-auto underline decoration-rule-strong underline-offset-2 hover:text-ink">
          API docs
        </a>
      </div>
    </footer>
  );
}

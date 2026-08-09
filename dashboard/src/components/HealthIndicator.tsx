import { useEffect, useState } from "react";
import { getHealth } from "../api";
import type { Health } from "../types";

type State = { kind: "loading" } | { kind: "ok"; health: Health } | { kind: "down" };

const POLL_MS = 20000;

export function HealthIndicator() {
  const [state, setState] = useState<State>({ kind: "loading" });

  useEffect(() => {
    let cancelled = false;

    async function poll() {
      try {
        const health = await getHealth();
        if (!cancelled) setState({ kind: "ok", health });
      } catch {
        if (!cancelled) setState({ kind: "down" });
      }
    }

    poll();
    const id = setInterval(poll, POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, []);

  const dotClass = state.kind === "ok" ? "ok" : state.kind === "down" ? "down" : "";
  const label =
    state.kind === "loading"
      ? "checking backend\u2026"
      : state.kind === "ok"
        ? `api ok \u00b7 db ${state.health.db}`
        : "backend unreachable";

  return (
    <span className="health-indicator" title="GET /health, polled every 20s">
      <span className={`health-dot ${dotClass}`} />
      {label}
    </span>
  );
}

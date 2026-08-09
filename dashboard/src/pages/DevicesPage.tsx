import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { ApiError, listDevices } from "../api";
import { BootLine } from "../components/BootLine";
import { StatusPill } from "../components/StatusPill";
import { LoadingState, ErrorState, EmptyState } from "../components/AsyncState";
import { relativeTime, truncateId } from "../lib/format";
import type { DeviceList } from "../types";

const PAGE_SIZE = 50;
const AUTO_REFRESH_MS = 15000;

export function DevicesPage() {
  const [data, setData] = useState<DeviceList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [offset, setOffset] = useState(0);
  const [query, setQuery] = useState("");
  const [autoRefresh, setAutoRefresh] = useState(true);

  useEffect(() => {
    let cancelled = false;

    async function load() {
      try {
        const result = await listDevices({ limit: PAGE_SIZE, offset });
        if (!cancelled) {
          setData(result);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) setError(e instanceof ApiError ? e.message : "failed to load devices");
      }
    }

    load();
    if (!autoRefresh) return () => {
      cancelled = true;
    };
    const id = setInterval(load, AUTO_REFRESH_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [offset, autoRefresh]);

  const filtered = useMemo(() => {
    if (!data) return [];
    const q = query.trim().toLowerCase();
    if (!q) return data.devices;
    return data.devices.filter(
      (d) =>
        d.device_id.toLowerCase().includes(q) || (d.hostname ?? "").toLowerCase().includes(q),
    );
  }, [data, query]);

  return (
    <>
      <BootLine>enumerating registered devices\u2026</BootLine>
      <h1 className="page-title">Fleet</h1>
      <p className="page-sub">
        {data ? `${data.total} device${data.total === 1 ? "" : "s"} registered` : "\u2014"}
      </p>

      <div className="toolbar">
        <input
          className="input"
          type="search"
          placeholder="filter by device id or hostname\u2026"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <button
          className="btn btn-toggle"
          aria-pressed={autoRefresh}
          onClick={() => setAutoRefresh((v) => !v)}
          title="Poll the fleet list every 15s"
        >
          {autoRefresh ? "auto-refresh: on" : "auto-refresh: off"}
        </button>
      </div>

      <div className="panel">
        <div className="panel-body no-pad">
          {error && <ErrorState message={error} />}
          {!error && !data && <LoadingState label="reading device table\u2026" />}
          {!error && data && filtered.length === 0 && (
            <EmptyState
              message={
                query
                  ? "no devices match that filter"
                  : "no devices have registered yet \u2014 waiting on the first diagnostic boot"
              }
            />
          )}
          {!error && data && filtered.length > 0 && (
            <table>
              <thead>
                <tr>
                  <th>device</th>
                  <th>hostname</th>
                  <th>last seen</th>
                  <th>latest score</th>
                  <th className="num">reports</th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((d) => (
                  <DeviceRow key={d.id} deviceId={d.device_id} hostname={d.hostname} lastSeenAt={d.last_seen_at} reportCount={d.report_count} score={d.latest_report?.score} scoreLabel={d.latest_report?.score_label} />
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>

      {data && data.total > PAGE_SIZE && (
        <div className="pager">
          <button className="btn" disabled={offset === 0} onClick={() => setOffset((o) => Math.max(0, o - PAGE_SIZE))}>
            &larr; prev
          </button>
          <button
            className="btn"
            disabled={offset + PAGE_SIZE >= data.total}
            onClick={() => setOffset((o) => o + PAGE_SIZE)}
          >
            next &rarr;
          </button>
        </div>
      )}

      <p className="footer-note">GET /devices &middot; polling every {AUTO_REFRESH_MS / 1000}s when auto-refresh is on</p>
    </>
  );
}

function DeviceRow({
  deviceId,
  hostname,
  lastSeenAt,
  reportCount,
  score,
  scoreLabel,
}: {
  deviceId: string;
  hostname: string | null;
  lastSeenAt: string;
  reportCount: number;
  score?: number;
  scoreLabel?: string;
}) {
  const navigate = useNavigate();
  const path = `/devices/${encodeURIComponent(deviceId)}`;

  return (
    <tr className="row-link" onClick={() => navigate(path)}>
      <td>
        <Link to={path} title={deviceId} onClick={(e) => e.stopPropagation()}>
          {truncateId(deviceId)}
        </Link>
      </td>
      <td className={hostname ? undefined : "mono-dim"}>{hostname ?? "\u2014"}</td>
      <td className="mono-dim" title={lastSeenAt}>
        {relativeTime(lastSeenAt)}
      </td>
      <td>{scoreLabel ? <StatusPill label={scoreLabel} score={score} /> : <span className="mono-dim">no reports yet</span>}</td>
      <td className="num mono-dim">{reportCount}</td>
    </tr>
  );
}

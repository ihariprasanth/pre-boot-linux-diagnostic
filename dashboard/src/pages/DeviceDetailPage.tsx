import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { ApiError, getDevice, listDeviceReports } from "../api";
import { BootLine } from "../components/BootLine";
import { KeyValueGrid } from "../components/KeyValueGrid";
import { StatusPill } from "../components/StatusPill";
import { LoadingState, ErrorState, EmptyState } from "../components/AsyncState";
import { absoluteTime, relativeTime, truncateId } from "../lib/format";
import type { Device, ReportList } from "../types";

const PAGE_SIZE = 25;

export function DeviceDetailPage() {
  const { deviceId = "" } = useParams();
  const [device, setDevice] = useState<Device | null>(null);
  const [history, setHistory] = useState<ReportList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [offset, setOffset] = useState(0);

  useEffect(() => {
    let cancelled = false;
    setDevice(null);
    setHistory(null);
    setError(null);
    setOffset(0);

    getDevice(deviceId)
      .then((d) => !cancelled && setDevice(d))
      .catch((e) => !cancelled && setError(e instanceof ApiError ? e.message : "failed to load device"));

    return () => {
      cancelled = true;
    };
  }, [deviceId]);

  useEffect(() => {
    let cancelled = false;
    listDeviceReports(deviceId, { limit: PAGE_SIZE, offset })
      .then((h) => !cancelled && setHistory(h))
      .catch((e) => !cancelled && setError(e instanceof ApiError ? e.message : "failed to load boot history"));
    return () => {
      cancelled = true;
    };
  }, [deviceId, offset]);

  return (
    <>
      <p className="breadcrumbs">
        <Link to="/">devices</Link> / {truncateId(deviceId, 12, 8)}
      </p>
      <BootLine>{`reading device record ${truncateId(deviceId)}\u2026`}</BootLine>

      {error && !device && <ErrorState message={error} />}
      {!error && !device && <LoadingState label="reading device record\u2026" />}

      {device && (
        <>
          <h1 className="page-title">{device.hostname ?? "(no hostname reported)"}</h1>
          <p className="page-sub mono-dim" title={device.device_id}>
            {device.device_id}
          </p>

          <section className="panel">
            <div className="panel-header">
              <h2>device</h2>
            </div>
            <div className="panel-body">
              <KeyValueGrid
                items={[
                  { label: "first seen", value: absoluteTime(device.first_seen_at) },
                  { label: "last seen", value: `${absoluteTime(device.last_seen_at)} (${relativeTime(device.last_seen_at)})` },
                  { label: "reports submitted", value: device.report_count },
                  {
                    label: "latest score",
                    value: device.latest_report ? (
                      <StatusPill label={device.latest_report.score_label} score={device.latest_report.score} />
                    ) : (
                      "\u2014"
                    ),
                  },
                ]}
              />
            </div>
          </section>

          <section className="panel">
            <div className="panel-header">
              <h2>boot history</h2>
            </div>
            <div className="panel-body no-pad">
              {error && !history && <ErrorState message={error} />}
              {!history && !error && <LoadingState label="reading boot history\u2026" />}
              {history && history.reports.length === 0 && (
                <EmptyState message="no diagnostic reports submitted by this device yet" />
              )}
              {history && history.reports.length > 0 && (
                <ul className="boot-history">
                  {history.reports.map((r) => (
                    <li key={r.report_id}>
                      <StatusPill label={r.score_label} score={r.score} />{" "}
                      <Link to={`/reports/${encodeURIComponent(r.report_id)}`}>
                        boot {truncateId(r.boot_id, 8, 4)}
                      </Link>
                      <span className="meta" title={r.generated_at}>
                        generated {relativeTime(r.generated_at)} &middot; received {absoluteTime(r.received_at)}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </section>

          {history && history.total > PAGE_SIZE && (
            <div className="pager">
              <button className="btn" disabled={offset === 0} onClick={() => setOffset((o) => Math.max(0, o - PAGE_SIZE))}>
                &larr; newer
              </button>
              <button
                className="btn"
                disabled={offset + PAGE_SIZE >= history.total}
                onClick={() => setOffset((o) => o + PAGE_SIZE)}
              >
                older &rarr;
              </button>
            </div>
          )}
        </>
      )}
    </>
  );
}

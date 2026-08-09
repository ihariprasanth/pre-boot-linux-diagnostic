import type { Device, DeviceList, Health, ReportDetail, ReportList } from "./types";

// The dashboard is read-only (docs/architecture.md — "Dashboard: React/
// TypeScript, read-only view over backend data"), so this client only
// ever issues GET requests. No auth headers: Phase 7's device-signature
// scheme is for the diagnostic agent, not a human-facing viewer — if the
// backend ever needs to gate read access, that's a separate, simpler
// credential, not this one.
const API_BASE_URL: string =
  (import.meta.env.VITE_API_BASE_URL as string | undefined) ?? "http://localhost:8000";

export class ApiError extends Error {
  status: number;
  detail: unknown;

  constructor(status: number, message: string, detail?: unknown) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.detail = detail;
  }
}

async function getJson<T>(path: string): Promise<T> {
  let res: Response;
  try {
    res = await fetch(`${API_BASE_URL}${path}`, {
      method: "GET",
      headers: { Accept: "application/json" },
    });
  } catch {
    throw new ApiError(0, `Could not reach the backend at ${API_BASE_URL}.`);
  }

  if (!res.ok) {
    let detail: unknown;
    try {
      detail = await res.json();
    } catch {
      // body wasn't JSON — fine, we just won't have a detail to show
    }
    const detailMsg =
      detail && typeof detail === "object" && "detail" in detail
        ? String((detail as { detail: unknown }).detail)
        : res.statusText;
    throw new ApiError(res.status, detailMsg, detail);
  }

  return (await res.json()) as T;
}

export function getHealth(): Promise<Health> {
  return getJson<Health>("/health");
}

export function listDevices(opts: { limit?: number; offset?: number } = {}): Promise<DeviceList> {
  const params = new URLSearchParams();
  params.set("limit", String(opts.limit ?? 50));
  params.set("offset", String(opts.offset ?? 0));
  return getJson<DeviceList>(`/devices?${params.toString()}`);
}

export function getDevice(deviceId: string): Promise<Device> {
  return getJson<Device>(`/devices/${encodeURIComponent(deviceId)}`);
}

export function listDeviceReports(
  deviceId: string,
  opts: { limit?: number; offset?: number } = {},
): Promise<ReportList> {
  const params = new URLSearchParams();
  params.set("limit", String(opts.limit ?? 50));
  params.set("offset", String(opts.offset ?? 0));
  return getJson<ReportList>(`/devices/${encodeURIComponent(deviceId)}/reports?${params.toString()}`);
}

export function getReport(reportId: string): Promise<ReportDetail> {
  return getJson<ReportDetail>(`/reports/${encodeURIComponent(reportId)}`);
}

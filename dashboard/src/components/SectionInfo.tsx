import type {
  CpuInfo,
  GpuDevice,
  KernelInfo,
  MemoryInfo,
  NetworkInterface,
  PciDevice,
  SensorReading,
  StorageDevice,
  UsbDevice,
} from "../types";
import { formatBytes, orDash } from "../lib/format";
import { KeyValueGrid } from "./KeyValueGrid";
import { StatusPill } from "./StatusPill";

export function CpuInfoView({ info }: { info: CpuInfo }) {
  return (
    <KeyValueGrid
      items={[
        { label: "model", value: orDash(info.model) },
        { label: "vendor", value: orDash(info.vendor) },
        { label: "architecture", value: info.architecture },
        { label: "physical cores", value: orDash(info.physical_cores) },
        { label: "logical threads", value: orDash(info.logical_threads) },
        { label: "online cpus", value: info.online_cpus.length, dim: true },
        { label: "offline cpus", value: info.offline_cpus.length, dim: true },
        {
          label: "frequency",
          value:
            info.current_freq_mhz != null
              ? `${info.current_freq_mhz} MHz${info.max_freq_mhz ? ` / ${info.max_freq_mhz} MHz max` : ""}`
              : "\u2014",
        },
        { label: "governor", value: orDash(info.governor), dim: true },
        {
          label: "temperature",
          value: info.temperature_celsius != null ? `${info.temperature_celsius}\u00b0C` : "\u2014",
        },
      ]}
    />
  );
}

export function MemoryInfoView({ info }: { info: MemoryInfo }) {
  return (
    <KeyValueGrid
      items={[
        { label: "total", value: formatBytes(info.total_bytes) },
        { label: "available", value: formatBytes(info.available_bytes) },
        { label: "free", value: formatBytes(info.free_bytes), dim: true },
        { label: "swap total", value: formatBytes(info.swap_total_bytes), dim: true },
        { label: "swap free", value: formatBytes(info.swap_free_bytes), dim: true },
        { label: "online blocks", value: info.memory_online_blocks, dim: true },
        { label: "offline blocks", value: info.memory_offline_blocks, dim: true },
        { label: "ecc", value: info.ecc === null ? "\u2014" : info.ecc ? "yes" : "no" },
      ]}
    />
  );
}

export function KernelInfoView({ info }: { info: KernelInfo }) {
  return (
    <>
      <KeyValueGrid
        items={[
          { label: "version", value: orDash(info.version) },
          { label: "tainted", value: info.tainted ? `yes (code ${orDash(info.taint_code)})` : "no" },
          { label: "cmdline", value: orDash(info.cmdline), dim: true },
        ]}
      />
      {info.log_entries.length > 0 && (
        <table style={{ marginTop: 14 }}>
          <thead>
            <tr>
              <th>severity</th>
              <th>line</th>
            </tr>
          </thead>
          <tbody>
            {info.log_entries.map((e, i) => (
              <tr key={i}>
                <td>
                  <StatusPill label={e.severity} />
                </td>
                <td className="mono-dim">{e.line}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </>
  );
}

export function PciInfoView({ info }: { info: PciDevice[] }) {
  if (info.length === 0) return <p className="state-msg">no PCI devices enumerated</p>;
  return (
    <table>
      <thead>
        <tr>
          <th>address</th>
          <th>vendor</th>
          <th>device</th>
          <th>class</th>
          <th>driver</th>
          <th>link</th>
        </tr>
      </thead>
      <tbody>
        {info.map((d) => (
          <tr key={d.address}>
            <td>{d.address}</td>
            <td className="mono-dim">{orDash(d.vendor_id)}</td>
            <td className="mono-dim">{orDash(d.device_id)}</td>
            <td>{orDash(d.class)}</td>
            <td>{orDash(d.driver)}</td>
            <td className="mono-dim">{orDash(d.link_speed)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function StorageInfoView({ info }: { info: StorageDevice[] }) {
  if (info.length === 0) return <p className="state-msg">no storage devices enumerated</p>;
  return (
    <table>
      <thead>
        <tr>
          <th>name</th>
          <th>model</th>
          <th>size</th>
          <th>removable</th>
          <th>nvme</th>
          <th>smart</th>
        </tr>
      </thead>
      <tbody>
        {info.map((d) => (
          <tr key={d.name}>
            <td>{d.name}</td>
            <td>{orDash(d.model)}</td>
            <td className="mono-dim">{formatBytes(d.size_bytes)}</td>
            <td>{d.removable ? "yes" : "no"}</td>
            <td>{d.is_nvme ? "yes" : "no"}</td>
            <td>
              {d.smart_healthy === null ? (
                "\u2014"
              ) : (
                <StatusPill label={d.smart_healthy ? "PASS" : "FAIL"} />
              )}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function GpuInfoView({ info }: { info: GpuDevice[] }) {
  if (info.length === 0) return <p className="state-msg">no GPU devices enumerated (headless or VM)</p>;
  return (
    <table>
      <thead>
        <tr>
          <th>name</th>
          <th>vendor</th>
          <th>device</th>
          <th>driver</th>
          <th>temp</th>
        </tr>
      </thead>
      <tbody>
        {info.map((d) => (
          <tr key={d.name}>
            <td>{d.name}</td>
            <td className="mono-dim">{orDash(d.vendor_id)}</td>
            <td className="mono-dim">{orDash(d.device_id)}</td>
            <td>{orDash(d.driver)}</td>
            <td>{d.temperature_celsius != null ? `${d.temperature_celsius}\u00b0C` : "\u2014"}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function UsbInfoView({ info }: { info: UsbDevice[] }) {
  if (info.length === 0) return <p className="state-msg">no USB devices enumerated</p>;
  return (
    <table>
      <thead>
        <tr>
          <th>bus path</th>
          <th>vendor</th>
          <th>product</th>
          <th>manufacturer</th>
          <th>speed</th>
        </tr>
      </thead>
      <tbody>
        {info.map((d) => (
          <tr key={d.bus_path}>
            <td>{d.bus_path}</td>
            <td className="mono-dim">{orDash(d.vendor_id)}</td>
            <td>{orDash(d.product)}</td>
            <td>{orDash(d.manufacturer)}</td>
            <td className="mono-dim">{orDash(d.speed_mbps)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function NetworkInfoView({ info }: { info: NetworkInterface[] }) {
  if (info.length === 0) return <p className="state-msg">no network interfaces enumerated</p>;
  return (
    <table>
      <thead>
        <tr>
          <th>name</th>
          <th>state</th>
          <th>carrier</th>
          <th>mac</th>
          <th>speed</th>
          <th>wireless</th>
        </tr>
      </thead>
      <tbody>
        {info.map((d) => (
          <tr key={d.name}>
            <td>{d.name}</td>
            <td>{d.oper_state ? <StatusPill label={d.oper_state === "up" ? "PASS" : "WARN"} /> : "\u2014"}</td>
            <td>{d.carrier === null ? "\u2014" : d.carrier ? "yes" : "no"}</td>
            <td className="mono-dim">{orDash(d.mac_address)}</td>
            <td className="mono-dim">{d.speed_mbps != null ? `${d.speed_mbps} Mbps` : "\u2014"}</td>
            <td>{d.is_wireless ? "yes" : "no"}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function SensorsInfoView({ info }: { info: SensorReading[] }) {
  if (info.length === 0) return <p className="state-msg">no sensors enumerated</p>;
  return (
    <table>
      <thead>
        <tr>
          <th>chip</th>
          <th>label</th>
          <th>kind</th>
          <th className="num">value</th>
        </tr>
      </thead>
      <tbody>
        {info.map((s, i) => (
          <tr key={`${s.chip}-${s.label}-${i}`}>
            <td className="mono-dim">{s.chip}</td>
            <td>{s.label}</td>
            <td className="mono-dim">{s.kind}</td>
            <td className="num">
              {s.value}
              {s.unit}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

import type { DiagnosticResult } from "../types";
import { StatusPill } from "./StatusPill";

export function ResultsTable({ results }: { results: DiagnosticResult[] }) {
  if (results.length === 0) {
    return <p className="state-msg">no diagnostic results in this section</p>;
  }
  return (
    <table>
      <thead>
        <tr>
          <th>test</th>
          <th>component</th>
          <th>status</th>
          <th>severity</th>
          <th>message</th>
          <th className="num">ms</th>
        </tr>
      </thead>
      <tbody>
        {results.map((r, i) => (
          <tr key={`${r.test}-${r.component}-${i}`}>
            <td>{r.test}</td>
            <td className="mono-dim">{r.component}</td>
            <td>
              <StatusPill label={r.status} />
            </td>
            <td>
              <StatusPill label={r.severity} />
            </td>
            <td>{r.message}</td>
            <td className="num mono-dim">{r.duration_ms}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

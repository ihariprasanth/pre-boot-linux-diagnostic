import { useState, type ReactNode } from "react";
import type { DiagnosticResult } from "../types";
import { ResultsTable } from "./ResultsTable";

function worstStatus(results: DiagnosticResult[]): string | null {
  const order = ["FAIL", "WARN", "UNKNOWN", "SKIPPED", "PASS"];
  let worst: string | null = null;
  for (const r of results) {
    if (worst === null || order.indexOf(r.status) < order.indexOf(worst)) {
      worst = r.status;
    }
  }
  return worst;
}

export function SectionPanel({
  title,
  results,
  children,
  defaultOpen = false,
}: {
  title: string;
  results: DiagnosticResult[];
  children: ReactNode;
  defaultOpen?: boolean;
}) {
  const worst = worstStatus(results);
  const [open, setOpen] = useState(defaultOpen || worst === "FAIL" || worst === "WARN");

  return (
    <section className="panel">
      <button className="section-toggle" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        <h2>
          {title}
          <span className="section-count">
            {results.length} result{results.length === 1 ? "" : "s"}
          </span>
        </h2>
        <span className="chevron" aria-hidden="true">
          &#9656;
        </span>
      </button>
      {open && (
        <div className="panel-body">
          {children}
          {results.length > 0 && (
            <div style={{ marginTop: 16 }}>
              <ResultsTable results={results} />
            </div>
          )}
        </div>
      )}
    </section>
  );
}

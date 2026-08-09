interface StatusPillProps {
  label: string;
  score?: number;
}

const CLASS_BY_LABEL: Record<string, string> = {
  GOOD: "pill-good",
  PASS: "pill-good",
  WARNING: "pill-warn",
  WARN: "pill-warn",
  POOR: "pill-fail",
  FAIL: "pill-fail",
  CRITICAL: "pill-critical",
  SKIPPED: "pill-skip",
  UNKNOWN: "pill-unknown",
};

/** Renders a score/status label the way a POST screen renders a status
 * field: `[ 92 GOOD ]`. Used for score_label, DiagnosticResult.status,
 * and DiagnosticResult.severity alike — the color vocabulary is shared
 * across all of them on purpose, since they're all the same PASS/WARN/
 * FAIL semantics underneath. */
export function StatusPill({ label, score }: StatusPillProps) {
  const cls = CLASS_BY_LABEL[label] ?? "pill-unknown";
  return (
    <span className={`pill ${cls}`}>
      {score !== undefined ? `${score} ` : ""}
      {label}
    </span>
  );
}

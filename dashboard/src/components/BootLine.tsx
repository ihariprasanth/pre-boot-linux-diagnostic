export function BootLine({ children }: { children: string }) {
  return (
    <p className="boot-line">
      <span>{children}</span>
      <span className="caret" aria-hidden="true" />
    </p>
  );
}

export function LoadingState({ label = "reading\u2026" }: { label?: string }) {
  return <p className="state-msg">{label}</p>;
}

export function ErrorState({ message }: { message: string }) {
  return <p className="state-msg error">[ FAIL ] {message}</p>;
}

export function EmptyState({ message }: { message: string }) {
  return <p className="state-msg">{message}</p>;
}

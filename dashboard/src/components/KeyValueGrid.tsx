import type { ReactNode } from "react";

export interface KvItem {
  label: string;
  value: ReactNode;
  dim?: boolean;
  title?: string;
}

export function KeyValueGrid({ items }: { items: KvItem[] }) {
  return (
    <dl className="kv-grid">
      {items.map((item) => (
        <div className="kv" key={item.label}>
          <dt>{item.label}</dt>
          <dd className={item.dim ? "dim" : undefined} title={item.title}>
            {item.value}
          </dd>
        </div>
      ))}
    </dl>
  );
}

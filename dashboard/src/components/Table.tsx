import type { ReactNode } from "react";

/** A column heading shared by the results and runs tables. */
export function Th({ children, className = "", hint }: { children: ReactNode; className?: string; hint?: string }) {
  return (
    <th scope="col" title={hint} className={`eyebrow px-2 py-1.5 align-bottom text-ink-3 ${className}`}>
      {children}
    </th>
  );
}

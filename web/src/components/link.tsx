import { ArrowUpRight } from "lucide-react";
import type { ReactNode } from "react";

import { safeLink } from "@/model/links";

/**
 * The only component that renders an `href` from data (see `safeLink`). The destination host is
 * always visible; anything that is not `http(s)` renders as plain text.
 */
export function OutLink({
  url,
  children,
  className = "",
  withPort = true,
}: {
  url: string;
  children?: ReactNode;
  className?: string;
  /** `false` where the port is already shown next to the link (a probed endpoint). */
  withPort?: boolean;
}) {
  const link = safeLink(url);
  if (link === null) return <span className="text-ink-muted">invalid link</span>;
  return (
    <a
      href={link.href}
      target="_blank"
      rel="noopener noreferrer"
      className={`group/link inline-flex min-w-0 items-center gap-1.5 rounded-control text-ink ${className}`}
    >
      {children === undefined ? null : (
        <span className="underline decoration-line underline-offset-3 group-hover/link:decoration-ink">
          {children}
        </span>
      )}
      <span className="truncate font-mono text-data text-ink-muted group-hover/link:text-ink">
        {withPort ? link.host : link.hostname}
      </span>
      <ArrowUpRight className="-ml-1 size-3.5 shrink-0 text-ink-muted" aria-hidden />
    </a>
  );
}

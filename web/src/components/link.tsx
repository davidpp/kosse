import { ArrowUpRight } from "lucide-react";
import type { ReactNode } from "react";

import { safeLink } from "@/model/links";

/**
 * The only component that renders an `href` from data (see `safeLink`). The destination host is
 * visible, except on a quiet Open whose `:port` starts its line (the host is then in its
 * accessible name); anything that is not `http(s)` renders as plain text.
 */
export function OutLink({
  url,
  children,
  className = "",
  tone = "plain",
}: {
  url: string;
  children?: ReactNode;
  className?: string;
  /**
   * `target`: a service's open target, the row's one accent element. `quiet`: another endpoint's
   * Open, below the target: the label and arrow only, the host in the accessible name.
   */
  tone?: "plain" | "target" | "quiet";
}) {
  const link = safeLink(url);
  if (link === null) return <span className="text-ink-muted">invalid link</span>;
  return (
    <a
      href={link.href}
      target="_blank"
      rel="noopener noreferrer"
      aria-label={
        tone === "quiet" && typeof children === "string" ? `${children} ${link.host}` : undefined
      }
      className={`group/link inline-flex min-w-0 items-center gap-1.5 rounded-control ${TONE[tone].link} ${className}`}
    >
      {children === undefined ? null : (
        <span className={`underline underline-offset-3 ${TONE[tone].label}`}>{children}</span>
      )}
      {tone === "quiet" ? null : (
        <span className={`truncate font-mono text-data ${TONE[tone].host}`}>{link.host}</span>
      )}
      <ArrowUpRight className={`-ml-1 size-3.5 shrink-0 ${TONE[tone].arrow}`} aria-hidden />
    </a>
  );
}

const TONE = {
  plain: {
    link: "text-ink",
    label: "decoration-line group-hover/link:decoration-ink",
    host: "text-ink-muted group-hover/link:text-ink",
    arrow: "text-ink-muted",
  },
  target: {
    link: "text-accent",
    label: "font-semibold decoration-accent/40 group-hover/link:decoration-accent",
    host: "font-medium text-ink",
    arrow: "text-accent",
  },
  quiet: {
    link: "text-ink-muted",
    label: "decoration-line group-hover/link:text-ink group-hover/link:decoration-ink",
    host: "",
    arrow: "text-ink-muted",
  },
} as const;

/** A link the page may render: an `http(s)` URL, and its host (with and without the port). */
export type SafeLink = { href: string; host: string; hostname: string };

/**
 * The only way the dashboard turns data into an `href`. Redact, unlike React 19, renders a
 * `javascript:` URL as given, so anything process-derived or configured passes through here:
 * only absolute `http:`/`https:` URLs without credentials survive. The server's CSP is the
 * backstop, not the guard.
 */
export function safeLink(raw: string): SafeLink | null {
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    return null;
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") return null;
  if (url.username !== "" || url.password !== "" || url.hostname === "") return null;
  return { href: url.href, host: url.host, hostname: url.hostname };
}

/** Whether two URLs are the same link once parsed (`http://localhost:3000` is `…:3000/`). */
export function sameLink(a: string, b: string): boolean {
  const left = safeLink(a);
  return left !== null && left.href === safeLink(b)?.href;
}

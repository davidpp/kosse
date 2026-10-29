// Reading the native window's control-socket replies, shared by `bun run app` and
// `bun run test:window`. Replies are untyped JSON; everything here validates before use.

/** One rendered Rune element, as the window's `tree` reply lists it. */
export type Element = {
  key: string | null;
  role: string;
  text: string | null;
  label: string | null;
  /** x, y, width, height in window points (top-left origin). */
  rect: [number, number, number, number];
  opacity: number;
  inert: boolean;
};

export const delay = (ms: number) => new Promise((done) => setTimeout(done, ms));

export function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function nullableString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

/** The elements of a `tree` reply; throws on anything that is not one. */
export function elements(reply: Record<string, unknown>): Element[] {
  if (!Array.isArray(reply.elements)) throw new Error("no rendered element tree");
  return reply.elements.map((value: unknown): Element => {
    if (!isRecord(value)) throw new Error("tree element is not an object");
    const { key, role, text, label, rect, opacity, inert } = value;
    if (
      !nullableString(key) ||
      !nullableString(text) ||
      !nullableString(label) ||
      typeof role !== "string" ||
      typeof opacity !== "number" ||
      typeof inert !== "boolean" ||
      !Array.isArray(rect) ||
      rect.length !== 4
    )
      throw new Error(`unexpected tree element ${JSON.stringify(value)}`);
    const [x, y, width, height]: unknown[] = rect;
    if (
      typeof x !== "number" ||
      typeof y !== "number" ||
      typeof width !== "number" ||
      typeof height !== "number"
    )
      throw new Error(`unexpected element rect ${JSON.stringify(rect)}`);
    return { key, role, text, label, rect: [x, y, width, height], opacity, inert };
  });
}

/** Polls `read` until it returns a value; throws `message` after `timeout` ms. */
export async function until<T>(
  read: () => Promise<T | undefined>,
  message: string,
  timeout = 15000,
): Promise<T> {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    // Polling is sequential by design: each read waits for the previous one.
    // oxlint-disable-next-line no-await-in-loop
    const found = await read().catch(() => undefined);
    if (found !== undefined) return found;
    // oxlint-disable-next-line no-await-in-loop
    await delay(100);
  }
  throw new Error(`Timed out: ${message}`);
}

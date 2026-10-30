// Shared class lists, so every control in the dashboard has one shape (web/DESIGN.md).

const control =
  "inline-flex h-7 shrink-0 items-center justify-center gap-1.5 rounded-control px-2.5 font-medium whitespace-nowrap transition-colors duration-150 ease-out disabled:cursor-default disabled:opacity-55 aria-disabled:cursor-default aria-disabled:opacity-55";

/** Bordered secondary button on the surface. */
export const button = `${control} border border-line-strong bg-surface text-ink not-disabled:hover:bg-canvas not-disabled:active:translate-y-px`;

/** Light button: a hairline border at rest; red `danger` text for Stop in rows. */
export const quiet = `${control} border border-hairline px-2 text-ink not-disabled:hover:border-line not-disabled:hover:bg-canvas`;
export const quietDanger = `${control} border border-hairline px-2 text-danger not-disabled:hover:border-danger not-disabled:hover:bg-failed-wash`;

/**
 * A service row's Inspect and Stop: muted and borderless at rest, so they never compete with the
 * row's open target; hover and keyboard focus bring back the border, and Stop's `danger` text.
 */
export const rowAction = `${control} border border-transparent px-2 text-ink-muted not-disabled:hover:border-line not-disabled:hover:bg-canvas not-disabled:hover:text-ink focus-visible:text-ink`;
export const rowDanger = `${control} border border-transparent px-2 text-ink-muted not-disabled:hover:border-danger not-disabled:hover:bg-failed-wash not-disabled:hover:text-danger focus-visible:text-danger`;

/** Row-height buttons (20px of layout, 24px drawn), so a row's columns share one first line. */
export const rowButton = "-my-0.5 h-6";

/**
 * A quiet button inside a line of text ("Try HTTP"): underlined muted text like a quiet Open, so
 * an offer never outweighs what was observed. Keyboard focus draws the global ring.
 */
export const small =
  "inline-flex h-5 shrink-0 items-center rounded-[4px] text-ink-muted underline decoration-line underline-offset-3 transition-colors duration-150 ease-out hover:text-ink hover:decoration-ink";

/** Solid accent (Québec blue): the primary action of a dialog (Save). */
export const primary = `${control} bg-accent text-on-accent not-disabled:hover:bg-accent/90 not-disabled:active:translate-y-px`;

/** Solid danger: only the final Stop and Force confirmations. */
export const danger = `${control} bg-danger text-on-danger not-disabled:hover:bg-danger/90 not-disabled:active:translate-y-px`;

export const input =
  "h-8 w-full min-w-0 rounded-control border border-line-strong bg-surface px-2.5 text-ink placeholder:text-ink-muted";

export const label = "text-small font-medium text-ink-muted";

export const mono = "font-mono text-data tabular-nums";

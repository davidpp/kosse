import { X } from "lucide-react";
import { useEffect, useRef, type ReactNode } from "react";

/**
 * A native modal `<dialog>`: the browser traps focus, makes the page inert and closes on Escape.
 * `sheet` docks it to the right edge (a full pane on narrow widths). Focus starts on the element
 * marked `data-autofocus`, else on the close button. While `locked` (a request that cannot be
 * taken back is running) Escape and the close button do nothing.
 */
export function Dialog({
  title,
  sheet = false,
  onClose,
  children,
  labelledBy,
  locked = false,
}: {
  title: ReactNode;
  sheet?: boolean;
  onClose: () => void;
  children: ReactNode;
  labelledBy: string;
  locked?: boolean;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const closing = useRef(onClose);
  const isLocked = useRef(locked);
  useEffect(() => {
    closing.current = onClose;
    isLocked.current = locked;
  });

  useEffect(() => {
    const dialog = ref.current;
    if (dialog === null) return;
    // Escape and form[method=dialog] close the element; the owner then unmounts it.
    const closed = () => closing.current();
    const cancel = (event: Event) => {
      if (isLocked.current) event.preventDefault();
    };
    dialog.addEventListener("close", closed);
    dialog.addEventListener("cancel", cancel);
    if (!dialog.open) dialog.showModal();
    const first = dialog.querySelector<HTMLElement>("[data-autofocus]");
    first?.focus();
    return () => {
      dialog.removeEventListener("close", closed);
      dialog.removeEventListener("cancel", cancel);
      if (dialog.open) dialog.close();
    };
  }, []);

  // The modal is sized by its content (the UA's `height: fit-content`), so nothing inside may size
  // itself from the dialog's height: macOS's WKWebView resolves such a percentage (the UA's
  // `max-height: calc(100% - …)` inherited, or a `flex-basis: 0%`) as 0 and draws a 2px dialog
  // behind a visible backdrop (S11). Dialog and column cap themselves in the same viewport units
  // instead (the column 2px less, the dialog's border), so only the body scrolls; the sheet has a
  // definite height, so its column can fill it.
  const placement = sheet
    ? "m-0 ml-auto h-dvh max-h-none w-full max-w-none border-l sm:w-[34rem]"
    : "m-auto max-h-[calc(100dvh-2rem)] w-[calc(100%-2rem)] max-w-lg overflow-hidden rounded-panel border";
  return (
    <dialog
      ref={ref}
      aria-labelledby={labelledBy}
      className={`${placement} border-line bg-surface p-0 text-ink`}
    >
      <div
        className={sheet ? "flex h-full flex-col" : "flex max-h-[calc(100dvh-2rem-2px)] flex-col"}
      >
        <div className="flex items-start gap-3 border-b border-hairline px-4 py-3">
          <h2
            id={labelledBy}
            className="min-w-0 flex-1 text-base font-semibold tracking-tight text-pretty"
          >
            {title}
          </h2>
          <button
            type="button"
            className="-m-1 rounded-control p-1 text-ink-muted not-aria-disabled:hover:bg-canvas not-aria-disabled:hover:text-ink aria-disabled:opacity-40"
            aria-label="Close"
            aria-disabled={locked}
            onClick={() => {
              if (!locked) ref.current?.close();
            }}
          >
            <X className="size-4" aria-hidden />
          </button>
        </div>
        {children}
      </div>
    </dialog>
  );
}

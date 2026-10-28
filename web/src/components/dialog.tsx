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

  const placement = sheet
    ? "m-0 ml-auto h-dvh max-h-none w-full max-w-none border-l sm:w-[34rem]"
    : "m-auto w-[calc(100%-2rem)] max-w-lg rounded-panel border";
  return (
    <dialog
      ref={ref}
      aria-labelledby={labelledBy}
      className={`${placement} border-line bg-surface p-0 text-ink`}
    >
      <div className={sheet ? "flex h-full flex-col" : "flex max-h-[inherit] flex-col"}>
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

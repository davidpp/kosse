import { useEffect, type RefObject } from "react";

/**
 * Page keys: `/` focuses search; `j`/`k` or the arrow keys move between service rows; Enter on a
 * focused row presses its Inspect button. Keys typed into a field or inside a dialog are left
 * alone; Escape is the native dialog/close behaviour.
 */
export function useKeyboard(searchRef: RefObject<HTMLInputElement | null>): void {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.metaKey || event.ctrlKey || event.altKey) return;
      const target = event.target instanceof HTMLElement ? event.target : null;
      if (target?.closest("dialog") || document.querySelector("dialog[open]")) return;
      const typing =
        target !== null &&
        (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName));
      if (event.key === "/" && !typing) {
        event.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
        return;
      }
      if (event.key === "Enter" && target?.dataset.row !== undefined) {
        const inspect = target.querySelector<HTMLButtonElement>("[data-inspect]");
        if (inspect !== null) {
          event.preventDefault();
          inspect.click();
        }
        return;
      }
      const step =
        event.key === "j" || event.key === "ArrowDown"
          ? 1
          : event.key === "k" || event.key === "ArrowUp"
            ? -1
            : 0;
      if (step === 0) return;
      // From the search field only the arrows move into the list; j and k are text there.
      if (typing && !(target === searchRef.current && event.key.startsWith("Arrow"))) return;
      const rows = [...document.querySelectorAll<HTMLElement>("[data-row]")];
      if (rows.length === 0) return;
      const current = target?.closest<HTMLElement>("[data-row]") ?? null;
      const index = current === null ? (step === 1 ? -1 : rows.length) : rows.indexOf(current);
      const next = rows[Math.max(0, Math.min(rows.length - 1, index + step))];
      if (next === undefined) return;
      event.preventDefault();
      next.focus();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [searchRef]);
}

// Which profile an open Customize draft shows, and whether another writer changed it. Two writers
// share one central profile (the window and an agent's `kosse profile set`), so a draft keeps the
// revision it started from and never overwrites another writer's change silently.

/** A central profile as one snapshot showed it. */
export type Based<T> = { revision: string; profile: T };

export type DraftView<T> = {
  /** What the form shows and saves against (`expectedRevision`). */
  shown: Based<T>;
  /**
   * The profile changed under an edited draft, or a save was refused as a conflict: the draft is
   * kept until the user reloads. `reloadable` once the snapshot shows the newer profile (a refused
   * save can arrive before it).
   */
  changedOutside: { reloadable: boolean } | null;
};

/**
 * `pinned` is the base of an edited (or refused) draft, `null` while the draft is untouched. An
 * untouched draft follows the live profile; a pinned one stays on its base.
 */
export function draftView<T>({
  live,
  pinned,
  conflicted,
}: {
  live: Based<T>;
  pinned: Based<T> | null;
  conflicted: boolean;
}): DraftView<T> {
  if (pinned === null) return { shown: live, changedOutside: null };
  const changed = pinned.revision !== live.revision;
  return {
    shown: pinned,
    changedOutside: changed || conflicted ? { reloadable: changed } : null,
  };
}

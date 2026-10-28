import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState } from "react";

import { failureOf, preferencesQuery, savePreferences } from "@/api/client";

/** Kossé accepts one preferences save per second (`POST /api/preferences`). */
const SAVE_SPACING_MS = 1000;
/**
 * A choice still waiting for its save slot when the page goes away, for the next load of this
 * tab. Flushing it with a keepalive request would not help: it is pending exactly because a save
 * was accepted less than a second ago, so Kossé would answer 429.
 */
const UNSENT_KEY = "kosse.unsentAllListeners";

/**
 * "All listeners": starts from the saved preference and saves each choice. The choice applies to
 * this page at once whatever the server says; a failed read or save is reported, never hidden.
 * Saves are spaced to Kossé's limit and only the latest pending choice is sent.
 */
export function useAllListeners(recovered: string) {
  const client = useQueryClient();
  const saved = useQuery(preferencesQuery);
  const [unsent] = useState(readUnsent);
  const [chosen, setChosen] = useState<boolean | null>(unsent);
  const [readNoticed, setReadNoticed] = useState(false);
  const lastSent = useRef(0);
  const pending = useRef<{ timer: ReturnType<typeof setTimeout>; value: boolean } | null>(null);
  const save = useMutation({
    // Another page (or this one before a reload) may have saved within the second: retry once.
    mutationFn: async (allListeners: boolean) => {
      try {
        return await savePreferences(client, { allListeners });
      } catch (error) {
        const failure = failureOf(error);
        if (failure.kind !== "refused" || failure.code !== "rateLimited") throw error;
        lastSent.current = Date.now() + SAVE_SPACING_MS;
        await new Promise((resolve) => setTimeout(resolve, SAVE_SPACING_MS));
        return savePreferences(client, { allListeners });
      }
    },
    onSuccess: (stored) => client.setQueryData(preferencesQuery.queryKey, stored),
  });
  const { mutate } = save;

  const schedule = useCallback(
    (value: boolean) => {
      if (pending.current !== null) clearTimeout(pending.current.timer);
      const timer = setTimeout(
        () => {
          pending.current = null;
          lastSent.current = Date.now();
          mutate(value);
        },
        Math.max(0, lastSent.current + SAVE_SPACING_MS - Date.now()),
      );
      pending.current = { timer, value };
    },
    [mutate],
  );
  const choose = (value: boolean) => {
    setChosen(value);
    schedule(value);
  };

  // A choice the last load of this tab could not send yet is saved now.
  useEffect(() => {
    storage((store) => store.removeItem(UNSENT_KEY));
    if (unsent !== null) schedule(unsent);
  }, [schedule, unsent]);

  // A choice still waiting when the page goes away is handed to the next load of this tab.
  useEffect(() => {
    const stash = () => {
      const waiting = pending.current;
      if (waiting === null) return;
      clearTimeout(waiting.timer);
      pending.current = null;
      storage((store) => store.setItem(UNSENT_KEY, String(waiting.value)));
    };
    addEventListener("pagehide", stash);
    return () => {
      removeEventListener("pagehide", stash);
      stash();
    };
  }, []);

  // A read that failed for a passing reason is retried once Kossé is live again (or another
  // instance answers); a file Kossé cannot use stays reported until it is fixed and the page
  // reloaded.
  const readFailure = saved.isError ? failureOf(saved.error) : null;
  const retryRead =
    readFailure !== null &&
    !(readFailure.kind === "refused" && readFailure.code === "configUnavailable");
  const { refetch } = saved;
  useEffect(() => {
    if (retryRead && recovered.startsWith("live")) void refetch();
  }, [retryRead, recovered, refetch]);

  let problem: string | null = null;
  if (save.isError)
    problem = `“All listeners” applies to this page only; it was not saved: ${failureOf(save.error).message}.`;
  else if (readFailure !== null && chosen === null && !readNoticed)
    problem = `Saved preferences could not be read: ${readFailure.message}.`;

  return {
    allListeners: chosen ?? saved.data?.allListeners ?? false,
    choose,
    problem,
    dismiss: () => {
      save.reset();
      setReadNoticed(true);
    },
  };
}

/** sessionStorage can be missing or throw (private windows, blocked site data). */
function storage<T>(use: (store: Storage) => T): T | null {
  try {
    return use(sessionStorage);
  } catch {
    return null;
  }
}

function readUnsent(): boolean | null {
  const value = storage((store) => store.getItem(UNSENT_KEY));
  return value === "true" || value === "false" ? value === "true" : null;
}

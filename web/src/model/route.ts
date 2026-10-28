// Addressable views without a router library. The route contract is shared with the tray and
// window (S08): `/` is the dashboard, `/?inspect=<pid>.<startedAtSecs>` opens that service's
// Inspect sheet and `/?stop=<pid>.<startedAtSecs>` its Stop confirmation (never confirmed by the
// link itself).
import { useEffect, useState } from "react";

export type Route =
  | { view: "dashboard" }
  | { view: "inspect"; identity: string }
  | { view: "stop"; identity: string }
  | { view: "not-found" };

const IDENTITY = /^\d+\.(\d+|unknown)$/;

export function parseRoute(pathname: string, search: string): Route {
  if (pathname !== "/" && pathname !== "/index.html") return { view: "not-found" };
  const params = new URLSearchParams(search);
  const inspect = params.get("inspect");
  if (inspect !== null && IDENTITY.test(inspect)) return { view: "inspect", identity: inspect };
  const stop = params.get("stop");
  if (stop !== null && IDENTITY.test(stop)) return { view: "stop", identity: stop };
  return { view: "dashboard" };
}

export function routeHref(route: Route): string {
  switch (route.view) {
    case "inspect":
      return `/?inspect=${route.identity}`;
    case "stop":
      return `/?stop=${route.identity}`;
    case "dashboard":
    case "not-found":
      return "/";
  }
}

const CHANGE = "kosse:navigate";
/** Marks history entries this page pushed, so closing a view can go back instead of piling up. */
const PUSHED = "kosse";

export function navigate(route: Route): void {
  window.history.pushState(PUSHED, "", routeHref(route));
  window.dispatchEvent(new Event(CHANGE));
}

/** Leaves an Inspect or Stop view: back to the entry it was opened from, else to `/`. */
export function closeView(): void {
  if (window.history.state === PUSHED) {
    window.history.back();
  } else {
    window.history.replaceState(null, "", "/");
    window.dispatchEvent(new Event(CHANGE));
  }
}

function current(): Route {
  return parseRoute(window.location.pathname, window.location.search);
}

export function useRoute(): Route {
  const [route, setRoute] = useState(current);
  useEffect(() => {
    const update = () => setRoute(current());
    window.addEventListener("popstate", update);
    window.addEventListener(CHANGE, update);
    return () => {
      window.removeEventListener("popstate", update);
      window.removeEventListener(CHANGE, update);
    };
  }, []);
  return route;
}

# UI pass: what to open, what is secondary, what is not web

Recorded 2026-10-30 on macOS, Apple silicon, on branch `dashboard-hierarchy` (rebased onto `main` after S21 merged). Dashboard-only: no Rust, contract or tray change.

## The problem, from the user's goal

The owner: "I'm having a hard time seeing what is the main url to check vs less important vs database and things like that." The dashboard's second most frequent job (blueprint/product.md, primary job 3) is to open the service you are working on without remembering its port. Before this pass every endpoint line had the same weight: a marker, `:port`, the HTTP result with the page title, then an underlined `Open` after the title, so the link landed at a different x on every line, after text that truncates. A service with three answering ports offered three equal Opens, a declared local link to the same URL offered a fourth, the eye went to the one red icon (an internal port) and to the bordered Inspect / Stop buttons and ink metrics on every row, and a database line looked like a web line that had not been checked.

## Hierarchy

Per service row, three tiers, all from observed facts and declared display tiers only:

1. **The open target.** Among the service's summary endpoints that answered HTTP (the launch targets of `Service::launch_endpoints`, the same set the tray and `kosse url` choose from), one is drawn first and strongest: the accent `Open` label with the full `localhost:<port>` in ink mono and the arrow, at the start of the endpoint column, so it sits at the same x on every row. When several answered (automatically or on Try HTTP, HTTP or HTTPS alike), a status below 400 with a page title comes first, then a status below 400, then a title, then any response; the lowest port breaks ties. A titled 404 page never beats an untitled 200. When nothing better answered, the target is a 4xx or 5xx and its status stays on the line. This is ordering and emphasis only: every answering summary endpoint keeps its own Open, nothing is labelled "main", and the tray and CLI keep their meaning (several answering endpoints remain several Open items, and `kosse url` stays ambiguous).
2. **Other summary endpoints.** One line each, below the target: marker, `:port`, name, HTTP status without the page title (the title is on the target and in Inspect), a quiet `Open ↗` when it answered (no host; its accessible name is `Open localhost:<port>`), Try HTTP when allowed. A failure on a summary endpoint keeps the failed marker: it is a launch target.
3. **Details endpoints** stay on the one muted `details` line (S21), never with a marker, an Open or the failed marker; endpoints are separated by commas, since `·` separates a status from its title.

The kind cue is a quiet `TCP` tag, shown only after a probe got an answer that is not HTTP (automatic or on Try HTTP). A known non-HTTP port or process name is a convention, not an observation: it keeps its "not probed: non-HTTP port/server" text and gets no tag. Every endpoint is TCP (`Endpoint.protocol`), so the tag is a fact that reads as "not web" without naming a guess such as "database". Unchecked or failed endpoints get no tag (unknown stays unknown). Try HTTP is a quiet underlined text button, so an offer never outweighs what was observed.

A declared link whose URL equals the URL of an endpoint that already offers Open is not repeated on the row; Inspect lists every declared link with its provenance. Declared links to other URLs stay as before.

Metrics and actions recede: metric values in `ink-muted`, Inspect, Stop and Customize as muted, borderless text at rest (hover brings back the border; hover and keyboard focus bring back Stop's `danger` text). The accent on the row is then only the open target.

No project-level "main URL": no observed fact makes one service of a project its entry point, and the tray has no such item. When two services serve the same app (an unconfigured Wrangler and its workerd both answer), the fix is a profile (`otherEndpoints: details`, `match.process`), which the prompt-first Customize window helps set up.

Trade-off: at rest Stop looks like Inspect, so it never outweighs the open target. The confirmation dialog, not the row's colour, is the safety cue; hover and keyboard focus restore Stop's red.

Unchanged: the Shape Rule markers, honesty wording (any status is reachable HTTP; failures say what happened), keyboard row focus, the narrow stacked layout, light and dark, reduced motion, `safeLink` for every href (`OutLink`), no inline style.

## What changed

- `web/src/model/endpoints.ts`: `openTarget` (the ranking above). `web/src/model/probe.ts`: `brief` (the result without the page title) and `notHttp` (HTTP ruled out). `web/src/model/links.ts`: `sameLink`.
- `web/src/components/project-card.tsx`: the target line first, other summary lines quieter, the duplicate declared link folded, `TCP` tag, muted metrics; `link.tsx`: `target` and `quiet` tones; `ui.ts`: borderless `rowAction` / `rowDanger` (also Customize in the card header).
- `e2e/support/world.ts`: the marketing site's central profile declares `Local site` at the Site endpoint's own URL, so the fold is exercised by Playwright, the window and the screenshots.
- `web/DESIGN.md`: the row order, the open link tones, the row actions and the fold.

## Tests (each guards a rule)

- `model/endpoints.test.ts`: a 404 on a lower port never outranks the page; a 200 outranks a 500; between equals the lowest port; no target when no summary endpoint answered. `TCP` only where HTTP is ruled out (pending, closed, unattributed and answering endpoints never get it). The brief line drops the page title only.
- `model/links.test.ts`: `sameLink` folds `http://localhost:3000` into `…:3000/`, never a different path, host spelling (`127.0.0.1`) or a non-HTTP URL.
- Playwright: of the API's two 404 ports, exactly one `[data-target]`, the lowest, drawn first, and both keep an Open; postgres carries `TCP`, caddy (TLS failure) does not; the site row has one link although its profile declares `Local site` at the same URL, and Inspect still lists `Local site`.

## Commands run

| Command                                                      | Result                                                                                                        |
| ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------- |
| `bun run app -- start/eval/shot/resize/theme/click/key/stop` | see below                                                                                                     |
| `bun run --cwd web test`                                     | 52 passed                                                                                                     |
| `bun run check`                                              | exit 0: fmt, oxlint, clippy, tsc, Vitest 52/52, budget `53.02 kB of 55.00 kB`, nextest 274/274, release build |
| `bun run test:e2e`                                           | 32 passed, 2 skipped (screenshot spec on Chromium)                                                            |
| `bun run test:window`                                        | 69 passed, 0 failed (screen unlocked)                                                                         |
| `bun run screenshots`                                        | regenerated `docs/screenshots/` (fixture data only), looked at all five                                       |
| `impeccable detect` on the changed components                | no findings                                                                                                   |

## Real window (`bun run app`)

Before and after screenshots were taken at 1180x780 (whole fixture world, and filtered to `marketing-site`, light and dark) and at 560x780 (light and dark); they are not committed because the unfiltered view shows the machine's own listeners. After: each service with an answering summary endpoint leads with the accent `Open localhost:<port>`; the API's second port reads `:3001 HTTP 404 Open localhost` in muted text; the site row shows no separate `Local site` line; postgres and the queue worker carry `TCP`; Inspect / Stop / Customize are muted text until hovered. At 560 px: no horizontal scroll (`scrollWidth 560 = innerWidth 560`), the row stacks and the actions sit on the column edge (a fix made after the first narrow screenshot showed them inset by the borderless padding).

Real AppKit clicks: Open on the marketing site's target handed `http://localhost:<site port>/` to the captured opener and the window stayed on `/`; Inspect opened the sheet with both endpoints and their tiers; Stop on postgres opened the confirmation naming PID and `:5432`, Cancel closed it and the row stayed; Customize opened the dialog, Escape closed it. Tab from Inspect to Stop: Stop is `:focus-visible` with the solid focus ring and its `danger` colour.

## Unverified

- Hover colours of the row actions were read from the classes, not seen: `bun run app -- click` does not hover.
- The owner's own Wrangler stack (not a fixture) was not probed by the debug app (`outsideTestScope`), so its after-state is inferred from the fixture equivalents: a page on one port outranks a 404 or 500 on another.
- The tray was not touched and not re-checked on the desktop; its Open items are unchanged in code.

## Review round 1

Fixes after two reviews, as new commits on the rebased branch:

- Ranking: status before title (above). Unit cases added: a titled 404 vs an untitled 200 (the 200 wins; the old ranking picked the 404), two titled 200s (the lowest port), a Try HTTP response that becomes the target over a skipped port, and HTTPS vs HTTP at equal rank (the lowest port; the scheme does not count).
- `TCP` only after a non-HTTP answer: `knownNonHttpPort` / `knownNonHttpProcess` no longer get it (unit test); Playwright asserts postgres has no tag until Try HTTP gets `not HTTP (on request)`, then has it. Try HTTP is a quiet text button.
- Quiet Open: `Open ↗` with a `line` underline at rest; Playwright asserts the accessible name `Open localhost:<port>` and its href. Real window: the quiet Open on the API's second port reads `Open localhost:3001` and a real click handed `http://localhost:3001/` to the captured opener.
- Details line: comma-separated (Playwright updated).
- Playwright `the open target carries the page title; other summary lines do not repeat it`: with both of the site's ports in the summary (the central profile edited for the test and restored), the target shows `HTTP 200 · <title>` and the other summary line shows `HTTP 200` without it.
- `web/DESIGN.md`: Layout list nesting fixed; 4xx targets, the Stop trade-off and the no-project-lead rationale recorded. `CHANGELOG.md`: a `Changed` bullet.

| Command               | Result                                                                              |
| --------------------- | ----------------------------------------------------------------------------------- |
| `bun run check`       | exit 0: Vitest 53/53, budget `52.98 kB of 55.00 kB`, nextest 274/274, release build |
| `bun run test:e2e`    | 34 passed, 2 skipped (screenshot spec on Chromium)                                  |
| `bun run test:window` | 69 passed, 0 failed (screen unlocked)                                               |
| `bun run screenshots` | regenerated; looked at all five                                                     |

Real window (`bun run app`): 1180x780 and 560x780, light and dark, filtered to the fixture world; no horizontal scroll at 560 (`scrollWidth 560 = innerWidth 560`). postgres reads `:5432 not probed: non-HTTP port Try HTTP` with no tag; a real click on Try HTTP turned it into `TCP not HTTP (on request)`; the queue worker (probed, not HTTP) carries `TCP`; workerd's details line reads `:<p1> HTTP 500, :<p2> HTTP 500`.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Plus, Trash2 } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";

import { failureOf, NO_REVISION, saveProfile } from "@/api/client";
import type {
  ConfiguredService,
  DeclaredProtocol,
  LinkKind,
  ProjectMetadata,
  ProjectProfile,
  Service,
  ServiceProfile,
} from "@/generated/contracts";
import { draftView, type Based } from "@/model/draft";
import { safeLink } from "@/model/links";

import { AgentPrompt } from "./customize-prompt";
import { Dialog } from "./dialog";
import { button, input, label, mono, primary, quiet } from "./ui";

type Base = Based<ProjectMetadata | null>;

/** `kind` round-trips an existing link's kind (the snapshot shows the effective one). */
type LinkDraft = { uid: number; label: string; url: string; kind?: LinkKind };
type ServiceDraft = {
  /** Stable key for the form row while its ID is being edited. */
  uid: number;
  id: string;
  name: string;
  cwd: string;
  port: string;
  protocol: DeclaredProtocol | "";
  probePath: string;
  links: LinkDraft[];
  /** The central profile sets `links` (an empty list then hides kosse.json's links). */
  linksSet: boolean;
  /** `disabled: true`: suppresses this ID, including a kosse.json entry with it. */
  disabled: boolean;
  /** The effective entry this draft edits, for showing what kosse.json sets. */
  effective: ConfiguredService | null;
};

/**
 * "Customize this project": first the prompt for the coding agent working in the repository, then
 * the fields. `metadata` is the live snapshot's; the form edits a draft of the central profile
 * at the revision it started from. Another writer's change (an agent's `kosse profile set`)
 * replaces an untouched draft, and is shown with a reload over an edited one, never overwritten:
 * saves carry that revision as `expectedRevision`.
 */
export function CustomizeDialog({
  root,
  title,
  metadata,
  services,
  onClose,
}: {
  root: string;
  title: string;
  metadata: ProjectMetadata | null;
  services: Service[];
  onClose: () => void;
}) {
  const live: Base = { revision: metadata?.centralRevision ?? NO_REVISION, profile: metadata };
  const [opened, setOpened] = useState(live.revision);
  const [pinned, setPinned] = useState<{ base: Base; title: string } | null>(null);
  const [conflicted, setConflicted] = useState(false);
  const [waited, setWaited] = useState(false);
  const [generation, setGeneration] = useState(0);
  const view = draftView({ live, pinned: pinned?.base ?? null, conflicted, waited });
  const pin = (base: Base | null) =>
    setPinned((current) => (base === null ? null : (current ?? { base, title })));
  const reload = () => {
    setPinned(null);
    setConflicted(false);
    setWaited(false);
    setOpened(live.revision);
    setGeneration((current) => current + 1);
  };
  // A refused save can arrive before the snapshot that shows the other writer's profile. That
  // takes one scan plus the file check (scheduler.rs, watch.rs); past it, Reload is offered anyway.
  useEffect(() => {
    if (!conflicted || waited) return;
    const timer = setTimeout(() => setWaited(true), RELOAD_BOUND_MS);
    return () => clearTimeout(timer);
  }, [conflicted, waited]);

  const notice =
    view.changedOutside !== null ? (
      <div
        role="alert"
        className="flex flex-wrap items-center gap-x-3 gap-y-2 border-t border-stale bg-stale-wash px-4 py-2.5"
      >
        <p className="min-w-0 flex-1 basis-60">
          An agent or another window changed this profile. Your edits aren't saved. Reload to see
          the new profile; your edits will be discarded.
        </p>
        <button
          type="button"
          className={button}
          aria-disabled={!view.changedOutside.reloadable}
          onClick={() => view.changedOutside?.reloadable === true && reload()}
        >
          {view.changedOutside.reloadable ? "Reload profile" : "Waiting for the change…"}
        </button>
      </div>
    ) : pinned === null && live.revision !== opened ? (
      <output className="block border-t border-hairline px-4 py-2 text-small text-ink-muted">
        Updated: the profile was changed outside this window.
      </output>
    ) : null;

  return (
    <Dialog
      title={`Customize ${pinned?.title ?? title}`}
      labelledBy="customize-title"
      onClose={onClose}
    >
      <ProfileForm
        root={root}
        title={title}
        metadata={view.shown.profile}
        services={services}
        expectedRevision={view.shown.revision}
        generation={generation}
        blocked={view.changedOutside !== null}
        lead={<AgentPrompt root={root} />}
        notice={notice}
        onPin={pin}
        onConflict={(base) => {
          pin(base);
          setConflicted(true);
        }}
        onClose={onClose}
      />
    </Dialog>
  );
}

const RELOAD_BOUND_MS = 5000;

/**
 * The fields: only those the central profile sets are editable here; values that come from the
 * repository's kosse.json are shown beside them and stay untouched. A new base (another
 * revision, or a reload) resets the draft in place, so focus stays where it was.
 */
function ProfileForm({
  root,
  title,
  metadata,
  services,
  expectedRevision,
  generation,
  blocked,
  lead,
  notice,
  onPin,
  onConflict,
  onClose,
}: {
  root: string;
  title: string;
  metadata: ProjectMetadata | null;
  services: Service[];
  expectedRevision: string;
  generation: number;
  /** The profile changed outside: saving would be refused, so Save is off. */
  blocked: boolean;
  lead: ReactNode;
  notice: ReactNode;
  /** The draft's base once it is edited (`null` while untouched), so it is kept. */
  onPin: (base: Base | null) => void;
  /** A save was refused because the profile changed since `expectedRevision`. */
  onConflict: (base: Base) => void;
  onClose: () => void;
}) {
  const client = useQueryClient();
  const hasCentral = metadata?.sources.includes("central") ?? false;
  const [start, setStart] = useState(() =>
    startOf(metadata, expectedRevision, generation, undefined),
  );
  const [name, setName] = useState(start.name);
  const [drafts, setDrafts] = useState<ServiceDraft[]>(start.drafts);
  const [problem, setProblem] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState(false);
  if (start.base.revision !== expectedRevision || start.generation !== generation) {
    const next = startOf(metadata, expectedRevision, generation, drafts);
    setStart(next);
    setName(next.name);
    setDrafts(next.drafts);
    setProblem(null);
    setConfirmRemove(false);
  }
  const { base } = start;
  const edited = name !== start.name || drafts !== start.drafts;
  useEffect(() => onPin(edited ? base : null), [edited, base, onPin]);

  const save = useMutation({
    mutationFn: (profile: ProjectProfile | null) =>
      saveProfile(client, { root, profile, expectedRevision }),
    onSuccess: onClose,
    onError: (error) => {
      const failure = failureOf(error);
      if (failure.kind === "refused" && failure.code === "revisionConflict") {
        setProblem(null);
        onConflict(base);
      } else {
        setProblem(failure.message);
      }
    },
  });

  const update = (index: number, change: Partial<ServiceDraft>) =>
    setDrafts((current) =>
      current.map((draft, at) => (at === index ? { ...draft, ...change } : draft)),
    );
  const unnamed = services.filter(
    (service) => service.profile === null && service.endpoints.length > 0,
  );
  // kosse.json entries without central fields: a central entry with the same ID overrides them.
  const repositoryOnly = (metadata?.services ?? []).filter(
    (entry) =>
      !Object.values(entry.sources).includes("central") &&
      !drafts.some((draft) => draft.id === entry.id),
  );

  const submit = () => {
    if (save.isPending || blocked) return;
    const built = buildProfile(name, drafts);
    if (typeof built === "string") {
      setProblem(built);
      return;
    }
    setProblem(null);
    save.mutate(built);
  };

  return (
    <>
      <form
        className="flex min-h-0 flex-auto flex-col"
        noValidate
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <div className="min-h-0 flex-auto space-y-4 overflow-y-auto px-4 py-4">
          {lead}
          <h3 className="border-t border-hairline pt-4 font-semibold tracking-tight">
            Or edit fields
          </h3>
          <p className="text-ink-muted">
            Saved in Kossé's settings on this Mac. Nothing is written to the project or its Git
            state.
          </p>
          {problem === null ? null : (
            <p
              role="alert"
              className="rounded-control border border-failed bg-failed-wash px-3 py-2"
            >
              Not saved: {problem}
            </p>
          )}
          <Field
            label="Project name"
            hint={
              metadata?.nameSource === "repository" ? `kosse.json: ${metadata.name ?? ""}` : null
            }
          >
            <input
              className={input}
              value={name}
              placeholder={title}
              onInput={(event) => setName(event.currentTarget.value)}
            />
          </Field>

          {drafts.map((draft, index) => (
            <fieldset
              key={draft.uid}
              className="space-y-2 rounded-control border border-hairline px-3 pt-2 pb-3"
            >
              <legend className="px-1 font-medium">
                Service <span className={mono}>{draft.id || "(new)"}</span>
              </legend>
              <div className="grid grid-cols-2 gap-2">
                <Field label="ID">
                  <input
                    className={`${input} ${mono}`}
                    value={draft.id}
                    onInput={(event) => update(index, { id: event.currentTarget.value })}
                  />
                </Field>
                <Field label="Name" hint={repoHint(draft, "name", draft.effective?.name)}>
                  <input
                    className={input}
                    value={draft.name}
                    onInput={(event) => update(index, { name: event.currentTarget.value })}
                  />
                </Field>
                <Field
                  label="Directory (relative)"
                  hint={repoHint(draft, "match", draft.effective?.match?.cwd)}
                >
                  <input
                    className={`${input} ${mono}`}
                    value={draft.cwd}
                    placeholder="."
                    onInput={(event) => update(index, { cwd: event.currentTarget.value })}
                  />
                </Field>
                <Field label="Port">
                  <input
                    className={`${input} ${mono}`}
                    value={draft.port}
                    inputMode="numeric"
                    onInput={(event) => update(index, { port: event.currentTarget.value })}
                  />
                </Field>
                <Field
                  label="Protocol (hint)"
                  hint={repoHint(draft, "protocol", draft.effective?.protocol)}
                >
                  <select
                    className={input}
                    value={draft.protocol}
                    onChange={(event) =>
                      update(index, { protocol: protocolOf(event.currentTarget.value) })
                    }
                  >
                    <option value="">not set</option>
                    <option value="http">http</option>
                    <option value="https">https</option>
                    <option value="tcp">tcp (not HTTP)</option>
                  </select>
                </Field>
                <Field
                  label="Probe path"
                  hint={repoHint(draft, "probePath", draft.effective?.probePath)}
                >
                  <input
                    className={`${input} ${mono}`}
                    value={draft.probePath}
                    placeholder="/"
                    onInput={(event) => update(index, { probePath: event.currentTarget.value })}
                  />
                </Field>
              </div>
              <div className="space-y-1.5">
                <span className={label}>Links</span>
                {draft.links.map((link, at) => (
                  <div key={link.uid} className="flex gap-2">
                    <input
                      className={input}
                      aria-label="Link label"
                      placeholder="Label"
                      value={link.label}
                      onInput={(event) =>
                        update(index, {
                          links: replaceAt(draft.links, at, {
                            ...link,
                            label: event.currentTarget.value,
                          }),
                        })
                      }
                    />
                    <input
                      className={`${input} ${mono}`}
                      aria-label="Link URL"
                      placeholder="https://app.localhost"
                      value={link.url}
                      onInput={(event) =>
                        update(index, {
                          links: replaceAt(draft.links, at, {
                            ...link,
                            url: event.currentTarget.value,
                          }),
                        })
                      }
                    />
                    <button
                      type="button"
                      className={quiet}
                      aria-label="Remove link"
                      onClick={() =>
                        update(index, {
                          links: draft.links.filter((_, i) => i !== at),
                          linksSet: true,
                        })
                      }
                    >
                      <Trash2 className="size-3.5" aria-hidden />
                    </button>
                  </div>
                ))}
                <div className="flex justify-between">
                  <button
                    type="button"
                    className={quiet}
                    onClick={() =>
                      update(index, {
                        links: [...draft.links, { uid: nextUid(), label: "", url: "" }],
                        linksSet: true,
                      })
                    }
                  >
                    <Plus className="size-3.5" aria-hidden /> Add link
                  </button>
                  <button
                    type="button"
                    className={`${quiet} text-ink-muted`}
                    onClick={() => setDrafts((current) => current.filter((_, i) => i !== index))}
                  >
                    Remove service
                  </button>
                </div>
                <p className="text-small text-ink-muted">
                  Links are shown as declared; Kossé never checks or probes them.
                </p>
                <label className="flex items-center gap-2">
                  <input
                    type="checkbox"
                    className="size-3.5"
                    checked={draft.disabled}
                    onChange={(event) => update(index, { disabled: event.currentTarget.checked })}
                  />
                  Turn this service off (also hides a kosse.json entry with this ID)
                </label>
              </div>
            </fieldset>
          ))}

          <div className="flex flex-wrap gap-2">
            {unnamed.map((service) => (
              <button
                key={`${service.process.identity.pid}`}
                type="button"
                className={button}
                onClick={() => setDrafts((current) => [...current, draftFor(service, current)])}
              >
                <Plus className="size-3.5" aria-hidden />
                Name <span className={mono}>:{service.endpoints[0]?.port}</span> (
                {service.process.name ?? "unknown"})
              </button>
            ))}
            {repositoryOnly.map((entry) => (
              <button
                key={entry.id}
                type="button"
                className={button}
                onClick={() => setDrafts((current) => [...current, overrideDraft(entry)])}
              >
                <Plus className="size-3.5" aria-hidden />
                Override <span className={mono}>{entry.id}</span> (kosse.json)
              </button>
            ))}
            <button
              type="button"
              className={button}
              onClick={() => setDrafts((current) => [...current, emptyDraft(current.length)])}
            >
              <Plus className="size-3.5" aria-hidden /> Add a service
            </button>
          </div>
        </div>
        {notice}
        <div className="flex flex-wrap items-center justify-end gap-2 border-t border-hairline px-4 py-3">
          {!hasCentral ? null : confirmRemove ? (
            <span className="mr-auto flex items-center gap-2">
              Remove every saved setting for this project?
              <button
                type="button"
                className={button}
                onClick={() => !save.isPending && save.mutate(null)}
              >
                Remove
              </button>
            </span>
          ) : (
            <button
              type="button"
              className={`${quiet} mr-auto text-ink-muted`}
              onClick={() => setConfirmRemove(true)}
            >
              Remove saved profile
            </button>
          )}
          <button type="button" className={button} onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className={primary} aria-disabled={save.isPending || blocked}>
            {save.isPending ? "Saving…" : "Save"}
          </button>
        </div>
      </form>
    </>
  );
}

function Field({
  label: text,
  hint = null,
  children,
}: {
  label: string;
  hint?: string | null;
  children: ReactNode;
}) {
  return (
    <label className="block min-w-0 space-y-1">
      <span className={label}>{text}</span>
      {children}
      {hint === null ? null : (
        <span className="block truncate text-small text-ink-muted">{hint}</span>
      )}
    </label>
  );
}

function repoHint(
  draft: ServiceDraft,
  field: "name" | "match" | "protocol" | "probePath",
  value: string | null | undefined,
): string | null {
  return draft.effective?.sources[field] === "repository" && value != null
    ? `kosse.json: ${value}`
    : null;
}

function protocolOf(value: string): DeclaredProtocol | "" {
  return value === "http" || value === "https" || value === "tcp" ? value : "";
}

function replaceAt<T>(list: T[], index: number, value: T): T[] {
  return list.map((item, at) => (at === index ? value : item));
}

/** The draft for one base; rows keep their keys (`uid`) by ID, so a reset keeps their elements. */
function startOf(
  metadata: ProjectMetadata | null,
  revision: string,
  generation: number,
  previous: ServiceDraft[] | undefined,
) {
  const drafts = initialDrafts(metadata);
  for (const draft of drafts) {
    draft.uid = previous?.find((old) => old.id === draft.id)?.uid ?? draft.uid;
  }
  return {
    base: { revision, profile: metadata } satisfies Base,
    generation,
    name: metadata?.nameSource === "central" ? (metadata.name ?? "") : "",
    drafts,
  };
}

export function initialDrafts(metadata: ProjectMetadata | null): ServiceDraft[] {
  if (metadata === null) return [];
  return metadata.services
    .filter((entry) => Object.values(entry.sources).includes("central"))
    .map((entry) => {
      const central = (field: keyof ConfiguredService["sources"]) =>
        entry.sources[field] === "central";
      return {
        uid: nextUid(),
        id: entry.id,
        name: central("name") ? (entry.name ?? "") : "",
        cwd: central("match") ? (entry.match?.cwd ?? "") : "",
        port: central("match") && entry.match?.port !== undefined ? String(entry.match.port) : "",
        protocol: central("protocol") ? (entry.protocol ?? "") : "",
        probePath: central("probePath") ? (entry.probePath ?? "") : "",
        links: central("links")
          ? entry.links.map((link) => ({
              uid: nextUid(),
              label: link.label,
              url: link.url,
              kind: link.kind,
            }))
          : [],
        linksSet: central("links"),
        disabled: central("disabled") && entry.disabled,
        effective: entry,
      };
    });
}

let uids = 0;
function nextUid(): number {
  uids += 1;
  return uids;
}

function uniqueId(base: string, taken: ServiceDraft[]): string {
  const slug =
    base
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-|-$/g, "") || "service";
  let id = slug;
  for (let n = 2; taken.some((draft) => draft.id === id); n++) id = `${slug}-${n}`;
  return id;
}

function draftFor(service: Service, taken: ServiceDraft[]): ServiceDraft {
  const base = service.package?.name ?? service.process.name ?? "service";
  return {
    uid: nextUid(),
    id: uniqueId(base, taken),
    name: "",
    cwd: service.package?.path || ".",
    port: String(service.endpoints[0]?.port ?? ""),
    protocol: "",
    probePath: "",
    links: [],
    linksSet: false,
    disabled: false,
    effective: null,
  };
}

function overrideDraft(entry: ConfiguredService): ServiceDraft {
  return { ...emptyDraft(0), id: entry.id, effective: entry };
}

function emptyDraft(count: number): ServiceDraft {
  return {
    uid: nextUid(),
    id: `service-${count + 1}`,
    name: "",
    cwd: "",
    port: "",
    protocol: "",
    probePath: "",
    links: [],
    linksSet: false,
    disabled: false,
    effective: null,
  };
}

/** The central profile the form describes, or a message naming the first invalid field. */
export function buildProfile(name: string, drafts: ServiceDraft[]): ProjectProfile | null | string {
  const services: Record<string, ServiceProfile> = {};
  for (const draft of drafts) {
    const id = draft.id.trim();
    if (id === "") return "every service needs an ID";
    if (id in services) return `the service ID "${id}" is used twice`;
    const entry: ServiceProfile = {};
    if (draft.name.trim() !== "") entry.name = draft.name.trim();
    const cwd = draft.cwd.trim();
    const portText = draft.port.trim();
    if (portText !== "" && !/^\d+$/.test(portText)) return `${id}: the port must be a number`;
    const port = portText === "" ? undefined : Number(portText);
    if (port !== undefined && (port < 1 || port > 65535))
      return `${id}: the port must be between 1 and 65535`;
    if (cwd !== "" || port !== undefined)
      entry.match = { ...(cwd === "" ? {} : { cwd }), ...(port === undefined ? {} : { port }) };
    if (draft.protocol !== "") entry.protocol = draft.protocol;
    const probePath = draft.probePath.trim();
    if (probePath !== "") {
      if (!probePath.startsWith("/")) return `${id}: the probe path must start with /`;
      entry.probePath = probePath;
    }
    const links = draft.links.filter((link) => link.label.trim() !== "" || link.url.trim() !== "");
    for (const link of links) {
      if (link.label.trim() === "") return `${id}: every link needs a label`;
      if (safeLink(link.url.trim()) === null)
        return `${id}: "${link.url}" is not an http or https URL`;
    }
    if (links.length > 0 || draft.linksSet) {
      entry.links = links.map((link) =>
        link.kind === undefined
          ? { label: link.label.trim(), url: link.url.trim() }
          : { label: link.label.trim(), url: link.url.trim(), kind: link.kind },
      );
    }
    if (draft.disabled) entry.disabled = true;
    services[id] = entry;
  }
  const trimmed = name.trim();
  if (trimmed === "" && Object.keys(services).length === 0) return null;
  return { version: 1, ...(trimmed === "" ? {} : { name: trimmed }), services };
}

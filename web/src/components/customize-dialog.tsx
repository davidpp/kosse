import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Plus, Trash2 } from "lucide-react";
import { useState, type ReactNode } from "react";

import { failureOf, saveProfile } from "@/api/client";
import type {
  ConfiguredService,
  DeclaredProtocol,
  EndpointProfile,
  EndpointShow,
  LinkKind,
  ProjectMetadata,
  ProjectProfile,
  Service,
  ServiceProfile,
} from "@/generated/contracts";
import { safeLink } from "@/model/links";

import { Dialog } from "./dialog";
import { button, input, label, mono, primary, quiet } from "./ui";

/** `kind` round-trips an existing link's kind (the snapshot shows the effective one). */
type LinkDraft = { uid: number; label: string; url: string; kind?: LinkKind };
/** `show: ""` leaves `show` unset: a listed endpoint is then in the summary. */
type EndpointDraft = {
  uid: number;
  port: string;
  name: string;
  show: EndpointShow | "";
  probePath: string;
};
type ServiceDraft = {
  /** Stable key for the form row while its ID is being edited. */
  uid: number;
  id: string;
  name: string;
  cwd: string;
  port: string;
  process: string;
  protocol: DeclaredProtocol | "";
  probePath: string;
  endpoints: EndpointDraft[];
  /** The central profile sets `endpoints` (an empty object then hides kosse.json's). */
  endpointsSet: boolean;
  otherEndpoints: EndpointShow | "";
  links: LinkDraft[];
  /** The central profile sets `links` (an empty list then hides kosse.json's links). */
  linksSet: boolean;
  /** `disabled: true`: suppresses this ID, including a kosse.json entry with it. */
  disabled: boolean;
  /** The effective entry this draft edits, for showing what kosse.json sets. */
  effective: ConfiguredService | null;
};

/**
 * "Customize this project": edits the project's central profile (Kossé's own configuration
 * file). Only fields the central profile sets are editable here; values that come from the
 * repository's kosse.json are shown beside them and stay untouched.
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
  const client = useQueryClient();
  const hasCentral = metadata?.sources.includes("central") ?? false;
  const [name, setName] = useState(metadata?.nameSource === "central" ? (metadata.name ?? "") : "");
  const [drafts, setDrafts] = useState<ServiceDraft[]>(() => initialDrafts(metadata));
  const [problem, setProblem] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState(false);

  const save = useMutation({
    mutationFn: (profile: ProjectProfile | null) => saveProfile(client, { root, profile }),
    onSuccess: onClose,
    onError: (error) => setProblem(failureOf(error).message),
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
    if (save.isPending) return;
    const built = buildProfile(name, drafts);
    if (typeof built === "string") {
      setProblem(built);
      return;
    }
    setProblem(null);
    save.mutate(built);
  };

  return (
    <Dialog title={`Customize ${title}`} labelledBy="customize-title" onClose={onClose}>
      <form
        className="flex min-h-0 flex-auto flex-col"
        noValidate
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <div className="min-h-0 flex-auto space-y-4 overflow-y-auto px-4 py-4">
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
              data-autofocus
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
                <Field label="Process name">
                  <input
                    className={`${input} ${mono}`}
                    value={draft.process}
                    placeholder="any"
                    onInput={(event) => update(index, { process: event.currentTarget.value })}
                  />
                </Field>
                <Field label="Other endpoints">
                  <ShowSelect
                    value={draft.otherEndpoints}
                    unset="not set"
                    onChange={(otherEndpoints) => update(index, { otherEndpoints })}
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
                  label="Probe path (summary endpoints)"
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
              <EndpointsEditor
                draft={draft}
                onChange={(endpoints) => update(index, { endpoints, endpointsSet: true })}
              />
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
          <button type="submit" className={primary} aria-disabled={save.isPending}>
            {save.isPending ? "Saving…" : "Save"}
          </button>
        </div>
      </form>
    </Dialog>
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

/** Port, name, tier and remove; the health path takes the line below in the same group. */
const endpointGrid = "grid grid-cols-[4.5rem_minmax(0,1fr)_7rem_2.25rem] items-center gap-2";

/** One group per listed endpoint; a listed endpoint is in the summary unless set to details. */
function EndpointsEditor({
  draft,
  onChange,
}: {
  draft: ServiceDraft;
  onChange: (endpoints: EndpointDraft[]) => void;
}) {
  return (
    <div className="space-y-1.5" data-endpoints-editor>
      <span className={`${label} block`}>Endpoints</span>
      {draft.endpoints.length === 0 ? null : (
        <div className={`${endpointGrid} px-2.5 ${label}`} aria-hidden>
          <span>Port</span>
          <span>Name</span>
          <span>Shown</span>
        </div>
      )}
      {draft.endpoints.map((endpoint, at) => {
        const set = (change: Partial<EndpointDraft>) =>
          onChange(replaceAt(draft.endpoints, at, { ...endpoint, ...change }));
        return (
          <div
            key={endpoint.uid}
            className="space-y-2 rounded-control border border-hairline bg-canvas/50 p-2"
            data-endpoint-row
          >
            <div className={endpointGrid}>
              <input
                className={`${input} ${mono}`}
                aria-label="Endpoint port"
                placeholder="3000"
                inputMode="numeric"
                value={endpoint.port}
                onInput={(event) => set({ port: event.currentTarget.value })}
              />
              <input
                className={input}
                aria-label="Endpoint name"
                placeholder="Site"
                value={endpoint.name}
                onInput={(event) => set({ name: event.currentTarget.value })}
              />
              <ShowSelect
                value={endpoint.show === "details" ? "details" : ""}
                unset="summary"
                onChange={(show) => set({ show })}
              />
              <button
                type="button"
                className={quiet}
                aria-label="Remove endpoint"
                onClick={() => onChange(draft.endpoints.filter((_, i) => i !== at))}
              >
                <Trash2 className="size-3.5" aria-hidden />
              </button>
            </div>
            <input
              className={`${input} ${mono}`}
              aria-label="Endpoint health path"
              placeholder="Health path for this port"
              value={endpoint.probePath}
              onInput={(event) => set({ probePath: event.currentTarget.value })}
            />
          </div>
        );
      })}
      <button
        type="button"
        className={quiet}
        onClick={() =>
          onChange([
            ...draft.endpoints,
            { uid: nextUid(), port: "", name: "", show: "", probePath: "" },
          ])
        }
      >
        <Plus className="size-3.5" aria-hidden /> Add endpoint
      </button>
      <p className="text-small text-ink-muted">
        Summary: what you open. Details: inspectors and internal ports, never opened.
      </p>
    </div>
  );
}

/** `unset` labels the empty choice: "not set", or "summary" where that is what unset means. */
function ShowSelect({
  value,
  unset,
  onChange,
}: {
  value: EndpointShow | "";
  unset: string;
  onChange: (show: EndpointShow | "") => void;
}) {
  return (
    <select
      className={input}
      aria-label="Shown in"
      value={value}
      onChange={(event) => {
        const next = event.currentTarget.value;
        onChange(next === "summary" || next === "details" ? next : "");
      }}
    >
      <option value="">{unset}</option>
      {unset === "summary" ? null : <option value="summary">summary</option>}
      <option value="details">details</option>
    </select>
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
        process: central("match") ? (entry.match?.process ?? "") : "",
        endpoints: central("endpoints")
          ? entry.endpoints.map((listed) => ({
              uid: nextUid(),
              port: String(listed.port),
              name: listed.name ?? "",
              show: listed.show ?? "",
              probePath: listed.probePath ?? "",
            }))
          : [],
        endpointsSet: central("endpoints"),
        otherEndpoints: central("otherEndpoints") ? (entry.otherEndpoints ?? "") : "",
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
    ...emptyDraft(0),
    id: uniqueId(base, taken),
    cwd: service.package?.path || ".",
    port: String(service.endpoints[0]?.port ?? ""),
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
    process: "",
    protocol: "",
    probePath: "",
    endpoints: [],
    endpointsSet: false,
    otherEndpoints: "",
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
    const process = draft.process.trim();
    if (cwd !== "" || port !== undefined || process !== "")
      entry.match = {
        ...(cwd === "" ? {} : { cwd }),
        ...(port === undefined ? {} : { port }),
        ...(process === "" ? {} : { process }),
      };
    if (draft.protocol !== "") entry.protocol = draft.protocol;
    const probePath = draft.probePath.trim();
    if (probePath !== "") {
      if (!probePath.startsWith("/")) return `${id}: the probe path must start with /`;
      entry.probePath = probePath;
    }
    const endpoints: Record<string, EndpointProfile> = {};
    for (const endpoint of draft.endpoints) {
      const key = endpoint.port.trim();
      const title = endpoint.name.trim();
      const path = endpoint.probePath.trim();
      if (key === "" && title === "" && path === "" && endpoint.show === "") continue;
      // The server checks ports and paths; a repeated port would silently drop a row.
      if (key in endpoints) return `${id}: endpoint :${key} is listed twice`;
      const listed: EndpointProfile = {};
      if (title !== "") listed.name = title;
      if (endpoint.show !== "") listed.show = endpoint.show;
      if (path !== "") listed.probePath = path;
      // Listing a port alone says "in the summary", which is what an unset `show` means.
      endpoints[key] = Object.keys(listed).length === 0 ? { show: "summary" } : listed;
    }
    if (Object.keys(endpoints).length > 0 || draft.endpointsSet) entry.endpoints = endpoints;
    if (draft.otherEndpoints !== "") entry.otherEndpoints = { show: draft.otherEndpoints };
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

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { LoaderCircle } from "lucide-react";
import { useEffect, useState } from "react";

import { act, failureOf } from "@/api/client";
import type { ActionRequest, PortOwner, Snapshot } from "@/generated/contracts";
import { formatClock, identityKey, sameIdentity } from "@/model/format";
import { describeFailure, describeOwner, describeResult, type Outcome } from "@/model/results";

import { Dialog } from "./dialog";
import { button, danger, mono } from "./ui";

/** Everything the confirmation showed, frozen when it opened: this is what the user confirms. */
export type StopTarget = {
  request: ActionRequest;
  name: string;
  project: string | null;
  /** Other processes on the same port numbers (another address family, say); not affected. */
  coOwners: PortOwner[];
};

type Step =
  | { step: "confirm" }
  | { step: "result"; outcome: Outcome; signal: "stop" | "force" }
  | { step: "confirm-force"; untilMs: number };

/**
 * The Stop confirmation for `identity` (`pid.start`), built from the snapshot it opened on and
 * frozen there: the instance, ports and names shown are what the request sends, even when a
 * newer snapshot arrives (Kossé then answers `stale`). A deep link to an identity that is not
 * running says so instead.
 */
export function StopDialog({
  identity,
  snapshot,
  onClose,
}: {
  identity: string;
  snapshot: Snapshot;
  onClose: () => void;
}) {
  const [target] = useState(() => stopTargetOf(identity, snapshot));
  if (target === null) {
    const [pid, started] = identity.split(".");
    return (
      <Dialog title="Process not running" labelledBy="stop-title" onClose={onClose}>
        <p className="px-4 py-4">
          No running process matches PID {pid}
          {started === undefined || started === "unknown"
            ? ""
            : ` started ${formatClock(Number(started) * 1000)}`}
          . It may have exited or been restarted under a new PID. Nothing was sent.
        </p>
        <div className="flex justify-end border-t border-hairline px-4 py-3">
          <button
            type="button"
            className={button}
            data-autofocus
            ref={focusOnMount}
            onClick={onClose}
          >
            Close
          </button>
        </div>
      </Dialog>
    );
  }
  return <Confirmation target={target} onClose={onClose} />;
}

function Confirmation({ target, onClose }: { target: StopTarget; onClose: () => void }) {
  const client = useQueryClient();
  const [step, setStep] = useState<Step>({ step: "confirm" });
  const { request, name } = target;
  const pid = request.identity.pid;
  const ports = portList(request.ports);

  const signal = useMutation({
    mutationFn: (kind: "stop" | "force") => act(client, kind, request),
    onSuccess: (result, kind) =>
      setStep({ step: "result", outcome: describeResult(result, kind), signal: kind }),
    onError: (error, kind) =>
      setStep({ step: "result", outcome: describeFailure(failureOf(error), kind), signal: kind }),
  });
  const busy = signal.isPending;

  const send = (kind: "stop" | "force") => {
    if (!busy) signal.mutate(kind);
  };

  const title =
    step.step === "confirm-force"
      ? `Force stop ${name} (PID ${pid})?`
      : step.step === "result"
        ? `${name} (PID ${pid})`
        : `Stop ${name} (PID ${pid})?`;

  return (
    <Dialog title={title} labelledBy="stop-title" onClose={onClose} locked={busy}>
      <div className="space-y-3 overflow-y-auto px-4 py-4" aria-live="polite">
        {step.step === "confirm" ? (
          request.identity.startedAtSecs === null ? (
            <p>
              Its start time is unknown, so Kossé cannot confirm which process this PID names. It
              will not stop it.
            </p>
          ) : (
            <>
              <p>
                {name}
                {target.project === null ? ", outside any project," : ` in ${target.project}`}{" "}
                listens on <span className={mono}>{portList(request.ports)}</span>. Stop sends
                SIGTERM; {affected(request.ports.length)}.
              </p>
              <p className="text-ink-muted">
                Kossé checks again that PID {pid} is the same process before sending anything, then
                watches its ports. Its parent is never signalled.
              </p>
              {target.coOwners.length === 0 ? null : (
                <p className="text-ink-muted">
                  Also on these port numbers, and not affected:{" "}
                  {target.coOwners.map(describeOwner).join(", ")}.
                </p>
              )}
            </>
          )
        ) : null}
        {step.step === "result" ? <OutcomeView outcome={step.outcome} /> : null}
        {step.step === "confirm-force" ? (
          <>
            <p>
              This kills the process immediately (SIGKILL): it cannot clean up or save anything.
              Affects <span className={mono}>{ports}</span>.
            </p>
            <p className="text-ink-muted">
              Offered until {formatClock(step.untilMs)}. Kossé checks the identity again first.
            </p>
          </>
        ) : null}
        {busy ? (
          <p className="flex items-center gap-2 text-ink-muted">
            <LoaderCircle className="size-3.5 animate-spin" aria-hidden />
            {signal.variables === "force"
              ? "Sending SIGKILL…"
              : "Sent the stop request; watching the process (up to 5 s)…"}
          </p>
        ) : null}
      </div>
      {/* Keyed by step: each step's buttons mount fresh, so focus moves to its safe default. */}
      <div
        key={step.step}
        className="flex flex-wrap justify-end gap-2 border-t border-hairline px-4 py-3"
      >
        <Actions
          step={step}
          busy={busy}
          onClose={onClose}
          send={send}
          setStep={setStep}
          eligible={request.identity.startedAtSecs !== null}
        />
      </div>
    </Dialog>
  );
}

function Actions({
  step,
  busy,
  eligible,
  onClose,
  send,
  setStep,
}: {
  step: Step;
  busy: boolean;
  eligible: boolean;
  onClose: () => void;
  send: (kind: "stop" | "force") => void;
  setStep: (step: Step) => void;
}) {
  const now = useNow(
    step.step === "result" && step.outcome.forceUntilMs !== null ? step.outcome.forceUntilMs : null,
  );
  switch (step.step) {
    case "confirm":
      return (
        <>
          {/* Once a signal is on its way, nothing here can take it back: no Cancel. */}
          {busy ? null : (
            <button
              type="button"
              className={button}
              data-autofocus
              ref={focusOnMount}
              onClick={onClose}
            >
              Cancel
            </button>
          )}
          {eligible ? (
            <button
              type="button"
              className={danger}
              aria-disabled={busy}
              onClick={() => send("stop")}
            >
              {busy ? "Stopping…" : "Stop process"}
            </button>
          ) : null}
        </>
      );
    case "confirm-force":
      return (
        <>
          {/* Once a signal is on its way, nothing here can take it back: no Cancel. */}
          {busy ? null : (
            <button
              type="button"
              className={button}
              data-autofocus
              ref={focusOnMount}
              onClick={onClose}
            >
              Cancel
            </button>
          )}
          <button
            type="button"
            className={danger}
            aria-disabled={busy}
            onClick={() => send("force")}
          >
            {busy ? "Forcing…" : "Force stop"}
          </button>
        </>
      );
    case "result": {
      const until = step.outcome.forceUntilMs;
      const offered = until !== null && now < until;
      return (
        <>
          {until === null ? null : offered ? (
            <button
              type="button"
              className={button}
              onClick={() => setStep({ step: "confirm-force", untilMs: until })}
            >
              Force stop…
            </button>
          ) : (
            <span className="mr-auto self-center text-ink-muted">
              The force offer expired. Stop it again to get a new one.
            </span>
          )}
          <button
            type="button"
            className={button}
            data-autofocus
            ref={focusOnMount}
            onClick={onClose}
          >
            {step.outcome.tone === "done" ? "Done" : "Close"}
          </button>
        </>
      );
    }
  }
}

function OutcomeView({ outcome }: { outcome: Outcome }) {
  return (
    <div role={outcome.tone === "done" ? "status" : "alert"} className="space-y-1.5">
      <p className="font-semibold">{outcome.title}</p>
      {outcome.detail.map((line) => (
        <p key={line} className="text-ink-muted">
          {line}
        </p>
      ))}
      {outcome.forceUntilMs === null ? null : (
        <p className="text-ink-muted">
          Force stop (SIGKILL) is offered until {formatClock(outcome.forceUntilMs)}. It needs its
          own confirmation.
        </p>
      )}
    </div>
  );
}

/** `:3000`, `:3000 and :3001`, `:3000, :3001 and :3002`. */
function portList(ports: number[]): string {
  const named = ports.map((port) => `:${port}`);
  const last = named.pop();
  return named.length === 0 ? (last ?? "") : `${named.join(", ")} and ${last}`;
}

function affected(count: number): string {
  if (count === 1) return "that port is affected";
  return count === 2 ? "both ports are affected" : `all ${count} ports are affected`;
}

/** Cancel, Done or Close: never a signal button. */
function focusOnMount(element: HTMLElement | null): void {
  element?.focus();
}

/** The current time, re-read once when `deadlineMs` passes (no ticking clock). */
function useNow(deadlineMs: number | null): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (deadlineMs === null) return;
    const timer = setTimeout(() => setNow(Date.now()), Math.max(0, deadlineMs - Date.now()) + 50);
    return () => clearTimeout(timer);
  }, [deadlineMs]);
  return now;
}

export function stopTargetOf(key: string, snapshot: Snapshot): StopTarget | null {
  const observation = snapshot.observation;
  const service = observation?.services.find(
    (candidate) => identityKey(candidate.process.identity) === key,
  );
  if (observation === null || service === undefined) return null;
  const ports = service.endpoints.map((endpoint) => endpoint.port);
  const project =
    observation.projects.find((candidate) => candidate.root === service.project) ?? null;
  const metadata = observation.profiles.find((candidate) => candidate.root === service.project);
  const match = service.profile;
  const configured =
    match?.kind === "matched"
      ? metadata?.services.find((entry) => entry.id === match.service)
      : undefined;
  return {
    request: { instance: snapshot.instance, identity: service.process.identity, ports },
    name: configured?.name ?? service.process.name ?? "unknown process",
    project: metadata?.name ?? project?.name ?? null,
    coOwners: observation.services
      .filter((other) => !sameIdentity(other.process.identity, service.process.identity))
      .flatMap((other) =>
        other.endpoints
          .filter((endpoint) => ports.includes(endpoint.port))
          .map((endpoint) => ({
            port: endpoint.port,
            identity: other.process.identity,
            name: other.process.name,
            parentPid: other.process.parentPid,
          })),
      ),
  };
}

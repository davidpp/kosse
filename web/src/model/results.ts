// Plain-language outcomes of stop and force. Each message says what was observed, never more:
// only `exited` is success, and nothing here equates a failed check with a stopped process.
import type { ActionResult, Ineligible, PortOwner, StaleReason } from "@/generated/contracts";

import type { ApiFailure } from "@/api/client";

export type Tone = "done" | "attention" | "failed";

export type Outcome = {
  tone: Tone;
  title: string;
  detail: string[];
  /** Present on `stillRunning`: force may be confirmed until this time (Unix ms). */
  forceUntilMs: number | null;
};

const STALE: Record<StaleReason, string> = {
  instanceChanged: "Kossé restarted after this view was loaded.",
  exited: "The process had already exited.",
  identityChanged: "That PID now belongs to a different process.",
  notInSnapshot: "The process is no longer in the latest scan.",
  portsChanged: "Its ports changed since the confirmation was shown.",
};

const REFUSED: Record<Ineligible, string> = {
  systemProcess: "It is a system process (PID 1 or lower).",
  kosseItself: "It is Kossé itself. Quit Kossé from its own menu instead.",
  noStartTime: "Its start time is unknown, so its identity cannot be confirmed.",
  otherUser: "It belongs to another user.",
  ownerUnknown: "Its owner could not be read.",
  unreadable: "The process could not be read.",
  outsideTestScope: "It is outside the test scope this server was started with (--act-only).",
};

export function describeOwner(owner: PortOwner): string {
  return `${owner.name ?? "unknown process"} (PID ${owner.identity.pid}) on :${owner.port}`;
}

function sharedNote(shared: PortOwner[]): string[] {
  return shared.length === 0
    ? []
    : [
        `Already listening on the same port numbers before the stop, and not affected: ${shared.map(describeOwner).join(", ")}.`,
      ];
}

function ports(list: number[]): string {
  return list.map((port) => `:${port}`).join(", ");
}

export function describeResult(result: ActionResult, signal: "stop" | "force"): Outcome {
  const sent = signal === "stop" ? "graceful stop request (SIGTERM)" : "force stop (SIGKILL)";
  const none = { forceUntilMs: null };
  switch (result.outcome) {
    case "exited":
      return {
        ...none,
        tone: "done",
        title: "The process exited.",
        detail: [
          `PID ${result.target.pid} is gone and ${ports(result.ports)} ${result.ports.length === 1 ? "is" : "are"} free.`,
          ...sharedNote(result.shared),
        ],
      };
    case "stillRunning":
      return {
        tone: "attention",
        title: "Still running.",
        detail: [
          `PID ${result.target.pid} did not exit after the ${sent} and still listens on ${ports(result.ports)}.`,
          ...(result.forceUntilMs === null
            ? [
                "Force stop is not offered: the process changed while it was observed. Review it and stop it again.",
              ]
            : []),
        ],
        forceUntilMs: result.forceUntilMs,
      };
    case "respawned":
      return {
        ...none,
        tone: "attention",
        title: "It exited, and a new process took its place.",
        detail: [
          `A process with the same parent (PID ${result.parentPid}) listens again: ${result.owners.map(describeOwner).join(", ")}.`,
          "Something is restarting it. Kossé never stops the parent; stop it where it was started.",
          ...sharedNote(result.shared),
        ],
      };
    case "endpointTaken":
      return {
        ...none,
        tone: "attention",
        title: "It exited, and another process now listens on its port.",
        detail: [
          `Now listening: ${result.owners.map(describeOwner).join(", ")}.`,
          ...sharedNote(result.shared),
        ],
      };
    case "observationFailed":
      return {
        ...none,
        tone: "failed",
        title: result.processExited
          ? "It exited, but its ports could not be checked."
          : "The result could not be observed.",
        detail: [`The ${sent} was sent. ${result.reason}`, "The dashboard shows the next scan."],
      };
    case "stale":
      return {
        ...none,
        tone: "failed",
        title: "Nothing was sent.",
        detail: [STALE[result.reason], "Review the current scan and try again."],
      };
    case "forceNotOffered":
      return {
        ...none,
        tone: "failed",
        title: "Force stop is no longer offered.",
        detail: ["The offer expired or was used. Nothing was sent; stop the process again first."],
      };
    case "refused":
      return {
        ...none,
        tone: "failed",
        title: "Kossé will not stop this process.",
        detail: [REFUSED[result.reason], "Nothing was sent."],
      };
    case "permissionDenied":
      return {
        ...none,
        tone: "failed",
        title: "macOS refused the signal.",
        detail: [`Permission denied for PID ${result.target.pid}. Nothing changed.`],
      };
    case "signalFailed":
      return {
        ...none,
        tone: "failed",
        title: "The signal could not be sent.",
        detail: [result.reason, "Nothing was sent."],
      };
    case "internal":
      return {
        ...none,
        tone: "failed",
        title: "Kossé hit an internal error.",
        detail: [result.reason],
      };
  }
}

/** A request that got no `ActionResult` back. */
export function describeFailure(failure: ApiFailure, signal: "stop" | "force"): Outcome {
  const none = { forceUntilMs: null, tone: "failed" as const };
  switch (failure.kind) {
    case "unreachable":
      return {
        ...none,
        title: "Kossé did not answer.",
        detail: [
          `The ${signal} request may or may not have reached it. Check the process once the dashboard reconnects.`,
        ],
      };
    case "refused":
      return {
        ...none,
        // Kossé's own message says which limit applied: one stop per second, or another stop
        // still being observed.
        title: failure.code === "rateLimited" ? "Not sent yet." : "The request was refused.",
        detail: [
          `${sentence(failure.message)} Nothing was sent${failure.code === "rateLimited" ? "; try again in a moment" : ""}.`,
        ],
      };
    case "unexpected":
      return { ...none, title: "Unexpected answer.", detail: [failure.message] };
  }
}

function sentence(text: string): string {
  const trimmed = text.trim().replace(/\.$/, "");
  return `${trimmed.charAt(0).toUpperCase()}${trimmed.slice(1)}.`;
}

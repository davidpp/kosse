import { useQuery } from "@tanstack/react-query";
import { Copy } from "lucide-react";
import { useRef, useState } from "react";

import { failureOf, promptQuery } from "@/api/client";

import { button, mono } from "./ui";

/**
 * "Customize with your agent": core's prompt for `root` (the text `kosse profile prompt` prints),
 * read-only, with Copy. The agent previews with `--dry-run` and applies with `--expect`; the
 * snapshot then carries the new profile to the fields below.
 */
export function AgentPrompt({ root }: { root: string }) {
  const prompt = useQuery(promptQuery(root));
  const text = useRef<HTMLTextAreaElement>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const failure = prompt.isError ? failureOf(prompt.error) : null;

  const copy = async () => {
    if (prompt.data === undefined) return;
    try {
      await navigator.clipboard.writeText(prompt.data.prompt);
      setCopied("Copied to the clipboard.");
    } catch {
      // No clipboard access: select the prompt for ⌘C instead.
      text.current?.select();
      setCopied("The prompt is selected; press ⌘C to copy it.");
    }
  };

  return (
    <section aria-labelledby="customize-agent" className="space-y-2">
      <div className="flex items-center gap-3">
        <h3 id="customize-agent" className="flex-1 font-semibold tracking-tight">
          Customize with your agent
        </h3>
        {failure === null ? (
          <button
            type="button"
            className={button}
            aria-disabled={prompt.data === undefined}
            onClick={() => void copy()}
            data-autofocus
          >
            <Copy className="size-3.5" aria-hidden />
            Copy prompt
          </button>
        ) : null}
      </div>
      {failure === null ? (
        <>
          <p className="text-ink-muted">
            Paste it into the coding agent working in this repository. It previews the profile with{" "}
            <code className={mono}>--dry-run</code>, then applies it with{" "}
            <code className={mono}>--expect</code>; the fields below follow.
          </p>
          <textarea
            ref={text}
            readOnly
            rows={9}
            aria-label="Prompt for your coding agent"
            className="block w-full resize-y rounded-control border border-hairline bg-canvas px-3 py-2 font-mono text-small text-ink"
            value={prompt.data?.prompt ?? "Loading…"}
          />
          <output className="block text-small text-ink-muted">{copied}</output>
        </>
      ) : (
        <p className="rounded-control border border-stale bg-stale-wash px-3 py-2">
          No prompt for this project: {failure.message}. Edit the fields below instead.
        </p>
      )}
    </section>
  );
}

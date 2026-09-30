"use client";
import * as React from "react";
import { toast } from "sonner";
import { Icon } from "@/components/icons";
import { ApiError } from "@/lib/api-client";

/**
 * One place that turns an API error into a toast, keyed by the server's stable
 * error `code`. Every mutation hook routes its onError through `toastError`, so
 * a new friendly message benefits status changes, reorders, edits, creates,
 * closes and gates at once rather than being bolted onto one route.
 */

const SKEW_BACKUP = "BR_IGNORE_SCHEMA_SKEW=1 br export --all -o beads-backup.jsonl";

function CommandLine({ cmd, note }: { cmd: string; note: string }) {
  const [copied, setCopied] = React.useState(false);
  return (
    <div className="flex items-center gap-2">
      <code className="min-w-0 flex-1 truncate rounded-md border border-border bg-[var(--surface-2)] px-[7px] py-[3px] font-mono text-[11px] text-[var(--text-2)]">
        {cmd}
      </code>
      <span className="flex-shrink-0 text-[10.5px] text-[var(--text-3)]">{note}</span>
      <button
        type="button"
        onClick={() => {
          void navigator.clipboard?.writeText(cmd).then(
            () => {
              setCopied(true);
              setTimeout(() => setCopied(false), 1500);
            },
            () => setCopied(false),
          );
        }}
        title="Copy command"
        aria-label={`Copy command: ${cmd}`}
        className="flex h-[22px] w-[22px] flex-shrink-0 items-center justify-center rounded-md border border-border text-[var(--text-3)] hover:text-[var(--text)]"
      >
        <Icon name={copied ? "check" : "link"} size={11} />
      </button>
    </div>
  );
}

/**
 * br refuses to open the database when its schema is ahead of the binary
 * (BeadsError::SchemaSkewForward). Nothing is written and nothing is lost — the
 * raw stderr just reads like data loss, so lead with "your data is safe". There
 * is no `br migrate` command: the fix is a binary that knows this schema, so
 * the copy button carries the safety export and nothing else.
 */
function SchemaMigrationMessage({ detail }: { detail?: string }) {
  const [open, setOpen] = React.useState(false);
  return (
    <div className="flex w-full flex-col gap-[7px]">
      <div className="text-[13px] font-[650]">Your beads database needs a one-time upgrade</div>
      <p className="m-0 text-[12px] leading-[1.5] text-[var(--text-2)]">
        This database was written by a newer <span className="font-mono">br</span> than the one
        you&rsquo;re running, so it refuses to open it at all.{" "}
        <strong>Your data is safe and nothing has been lost.</strong> In a terminal, from this
        project folder, take a safety copy:
      </p>
      <CommandLine cmd={SKEW_BACKUP} note="safety copy" />
      <p className="m-0 text-[11.5px] leading-[1.45] text-[var(--text-3)]">
        <span className="font-mono">BR_IGNORE_SCHEMA_SKEW=1</span> is what lets the read-only
        export run past that check; it changes nothing on disk. Then upgrade br to the version that
        wrote this database — there is no migrate command — and reload. Anyone else working in
        this workspace needs the same br version.
      </p>
      {detail && (
        <>
          <button
            type="button"
            onClick={() => setOpen((v) => !v)}
            className="self-start text-[11.5px] font-[550] text-[var(--text-3)] underline underline-offset-2 hover:text-[var(--text-2)]"
          >
            {open ? "Hide technical details" : "Show technical details"}
          </button>
          {open && (
            <pre className="m-0 max-h-[180px] overflow-auto whitespace-pre-wrap rounded-md border border-border bg-[var(--surface-2)] p-2 font-mono text-[10.5px] leading-[1.45] text-[var(--text-3)]">
              {detail}
            </pre>
          )}
        </>
      )}
    </div>
  );
}

/** Show an error as a toast, using a friendly form when we recognise its code. */
export function toastError(err: unknown) {
  if (err instanceof ApiError && err.code === "schema_migration_required") {
    toast.error(<SchemaMigrationMessage detail={err.detail} />, {
      duration: Infinity,
      closeButton: true,
      className: "w-[420px]",
    });
    return;
  }
  toast.error(err instanceof Error ? err.message : "Something went wrong");
}

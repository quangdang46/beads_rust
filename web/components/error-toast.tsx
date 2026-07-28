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

const MIGRATE_BACKUP = "bd export --all -o beads-backup.jsonl";
const MIGRATE_CMD = "BD_ALLOW_REMOTE_MIGRATE=1 bd migrate";

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
 * bd refuses ALL writes when the binary's schema is ahead of the local database
 * (gastownhall/beads#4259). Reads still work, so the board renders fine and only
 * mutations fail — which makes the raw stderr especially alarming and easy to
 * misread as data loss. Lead with "your data is safe".
 */
function SchemaMigrationMessage({ detail }: { detail?: string }) {
  const [open, setOpen] = React.useState(false);
  return (
    <div className="flex w-full flex-col gap-[7px]">
      <div className="text-[13px] font-[650]">Your beads database needs a one-time upgrade</div>
      <p className="m-0 text-[12px] leading-[1.5] text-[var(--text-2)]">
        Your <span className="font-mono">bd</span> tool is newer than this project&rsquo;s database,
        so writes are paused until they&rsquo;re reconciled.{" "}
        <strong>Your data is safe and nothing has been lost.</strong> In a terminal, from this
        project folder:
      </p>
      <CommandLine cmd={MIGRATE_BACKUP} note="safety copy" />
      <CommandLine cmd={MIGRATE_CMD} note="one-time" />
      <p className="m-0 text-[11.5px] leading-[1.45] text-[var(--text-3)]">
        Then reload. If this database is shared, run the migrate on{" "}
        <strong>one machine only</strong> — coordinate with anyone else using it first.
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

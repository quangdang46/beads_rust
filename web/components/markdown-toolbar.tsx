"use client";
import * as React from "react";
import { Icon } from "@/components/icons";
import {
  bold,
  italic,
  inlineCode,
  heading,
  bullet,
  checklist,
  quote,
  link,
  codeBlock,
  type MarkdownTransform,
} from "@/lib/markdown-edit";

/**
 * Apply a markdown transform to a textarea at its current selection, push the
 * rewritten text through `onChange`, then restore the cursor/selection once the
 * controlled re-render has flushed the new value into the DOM (bead 5ov).
 */
export function applyTransform(
  ta: HTMLTextAreaElement | null,
  value: string,
  onChange: (next: string) => void,
  fn: MarkdownTransform,
): void {
  if (!ta) return;
  const next = fn({ value, start: ta.selectionStart, end: ta.selectionEnd });
  onChange(next.value);
  requestAnimationFrame(() => {
    ta.focus();
    ta.setSelectionRange(next.start, next.end);
  });
}

type Item =
  | { kind: "sep" }
  | { kind: "btn"; title: string; fn: MarkdownTransform; icon?: string; glyph?: string; glyphClass?: string };

const ITEMS: Item[] = [
  { kind: "btn", title: "Bold (⌘B)", fn: bold, glyph: "B", glyphClass: "font-bold" },
  { kind: "btn", title: "Italic (⌘I)", fn: italic, glyph: "I", glyphClass: "italic font-serif" },
  { kind: "btn", title: "Inline code", fn: inlineCode, glyph: "</>", glyphClass: "font-mono text-[10px]" },
  { kind: "sep" },
  { kind: "btn", title: "Heading", fn: heading, glyph: "H", glyphClass: "font-semibold" },
  { kind: "btn", title: "Bullet list", fn: bullet, icon: "list" },
  { kind: "btn", title: "Checklist", fn: checklist, glyph: "☑", glyphClass: "text-[13px]" },
  { kind: "btn", title: "Quote", fn: quote, glyph: "❝", glyphClass: "text-[13px]" },
  { kind: "sep" },
  { kind: "btn", title: "Link (⌘K)", fn: link, icon: "link" },
  { kind: "btn", title: "Code block", fn: codeBlock, glyph: "{ }", glyphClass: "font-mono text-[10px]" },
];

export function MarkdownToolbar({
  textareaRef,
  value,
  onChange,
}: {
  textareaRef: React.RefObject<HTMLTextAreaElement | null>;
  value: string;
  onChange: (next: string) => void;
}) {
  return (
    <div className="mb-[6px] flex flex-wrap items-center gap-[3px]">
      {ITEMS.map((item, i) =>
        item.kind === "sep" ? (
          <span key={i} className="mx-[3px] h-[16px] w-px bg-border" />
        ) : (
          <button
            key={i}
            type="button"
            title={item.title}
            // Keep textarea focus/selection through the click so the transform
            // applies to what the user had selected, not a collapsed cursor.
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => applyTransform(textareaRef.current, value, onChange, item.fn)}
            className="flex h-7 min-w-7 items-center justify-center rounded-md border border-border bg-[var(--surface-2)] px-[6px] text-[12px] text-[var(--text-2)] hover:bg-[var(--surface-3)] hover:text-[var(--text)]"
          >
            {item.icon ? (
              <Icon name={item.icon} size={14} />
            ) : (
              <span className={item.glyphClass}>{item.glyph}</span>
            )}
          </button>
        ),
      )}
    </div>
  );
}

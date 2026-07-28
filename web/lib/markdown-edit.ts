/**
 * Pure helpers backing the description editor's formatting toolbar (bead 5ov).
 *
 * Each transform takes the current textarea selection and returns the rewritten
 * text plus the new selection range, so the caller can restore the cursor. They
 * keep markdown as the stored source of truth (no WYSIWYG) — buttons and the
 * Cmd/Ctrl shortcuts just insert or wrap markdown at the selection.
 */

/** A textarea selection: full text plus the [start, end) selected range. */
export type Sel = { value: string; start: number; end: number };
export type MarkdownTransform = (sel: Sel) => Sel;

/**
 * Wrap (or unwrap) the selection with an inline marker — bold (`**`),
 * italic (`*`), inline code (`` ` ``). Re-applying with the markers already
 * present unwraps them; with no selection it inserts the pair and parks the
 * cursor between them.
 */
function wrapInline(marker: string): MarkdownTransform {
  return ({ value, start, end }) => {
    const selected = value.slice(start, end);

    // Markers sit just outside the selection → unwrap them.
    if (
      value.slice(start - marker.length, start) === marker &&
      value.slice(end, end + marker.length) === marker
    ) {
      return {
        value: value.slice(0, start - marker.length) + selected + value.slice(end + marker.length),
        start: start - marker.length,
        end: end - marker.length,
      };
    }

    // Markers are inside the selection → unwrap them.
    if (
      selected.length >= 2 * marker.length &&
      selected.startsWith(marker) &&
      selected.endsWith(marker)
    ) {
      const inner = selected.slice(marker.length, selected.length - marker.length);
      return { value: value.slice(0, start) + inner + value.slice(end), start, end: start + inner.length };
    }

    if (start === end) {
      const cursor = start + marker.length;
      return { value: value.slice(0, start) + marker + marker + value.slice(end), start: cursor, end: cursor };
    }

    return {
      value: value.slice(0, start) + marker + selected + marker + value.slice(end),
      start: start + marker.length,
      end: end + marker.length,
    };
  };
}

/**
 * Toggle a line prefix — heading (`## `), bullet (`- `), checklist (`- [ ] `),
 * blockquote (`> `) — across every line touched by the selection. If all
 * non-empty lines already carry the prefix it is removed; otherwise it is added.
 */
function toggleLinePrefix(prefix: string): MarkdownTransform {
  return ({ value, start, end }) => {
    const lineStart = value.lastIndexOf("\n", start - 1) + 1;
    let lineEnd = value.indexOf("\n", end);
    if (lineEnd === -1) lineEnd = value.length;

    const lines = value.slice(lineStart, lineEnd).split("\n");
    const meaningful = lines.filter((l) => l.trim() !== "");
    const allPrefixed = meaningful.length > 0 && meaningful.every((l) => l.startsWith(prefix));

    const next = allPrefixed
      ? lines.map((l) => (l.startsWith(prefix) ? l.slice(prefix.length) : l))
      : lines.map((l) => prefix + l);

    const block = next.join("\n");
    return {
      value: value.slice(0, lineStart) + block + value.slice(lineEnd),
      start: lineStart,
      end: lineStart + block.length,
    };
  };
}

/**
 * Wrap the selection as a link `[text](url)`, parking the cursor over the `url`
 * placeholder. With no selection, inserts `[](url)` and parks the cursor inside
 * the empty link text.
 */
export const link: MarkdownTransform = ({ value, start, end }) => {
  const selected = value.slice(start, end);
  if (selected) {
    const inserted = `[${selected}](url)`;
    const urlStart = start + selected.length + 3; // past `[selected](`
    return { value: value.slice(0, start) + inserted + value.slice(end), start: urlStart, end: urlStart + 3 };
  }
  const inserted = "[](url)";
  return { value: value.slice(0, start) + inserted + value.slice(end), start: start + 1, end: start + 1 };
};

/**
 * Wrap the selection in a fenced code block on its own lines, keeping the cursor
 * (or the re-selected body) inside the fence.
 */
export const codeBlock: MarkdownTransform = ({ value, start, end }) => {
  const selected = value.slice(start, end);
  const pad = start > 0 && value[start - 1] !== "\n" ? "\n" : "";
  const open = "```\n";
  const body = pad + open + selected + "\n```";
  const cursor = start + pad.length + open.length;
  return {
    value: value.slice(0, start) + body + value.slice(end),
    start: cursor,
    end: cursor + selected.length,
  };
};

export const bold = wrapInline("**");
export const italic = wrapInline("*");
export const inlineCode = wrapInline("`");
export const heading = toggleLinePrefix("## ");
export const bullet = toggleLinePrefix("- ");
export const checklist = toggleLinePrefix("- [ ] ");
export const quote = toggleLinePrefix("> ");

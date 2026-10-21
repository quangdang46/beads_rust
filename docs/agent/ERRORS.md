# Errors

Most commands return non-zero exit codes on failure and may emit a structured error envelope.

Where the envelope goes depends on the output mode:

- **JSON mode** (`--json`, `--robot`, `--format json`): the structured error is written to
  **stdout**, so a robot caller reads exactly one parseable document on the same stream that
  carries success payloads. stderr holds only log lines.
- **Human mode**: the error goes to **stderr**, so stdout stays clean for the command's
  normal output and for pipelines.

Parse stdout for the error envelope after a non-zero exit in JSON mode; parse stderr in human
mode. Either way, check the exit code first.

Example (JSON mode, captured with stdout redirection):

```bash
br show br-NOTEXIST --json >err.json || true
cat err.json | jq .
```

Minimal regression check:

```bash
set +e
br show br-NOTEXIST --json >out.json 2>err.json
status=$?
set -e
test "$status" -eq 3
jq -e '.error.code == "ISSUE_NOT_FOUND"' out.json >/dev/null
```

Human mode keeps the error on stderr instead:

```bash
set +e
br show br-NOTEXIST >out.txt 2>err.txt
status=$?
set -e
test "$status" -eq 3
test ! -s out.txt
grep -q 'Issue not found' err.txt
```

Shape:

```json
{
  "error": {
    "code": "ISSUE_NOT_FOUND",
    "message": "Issue not found: br-NOTEXIST",
    "hint": "Run 'br list' to see available issues.",
    "retryable": false,
    "context": { "searched_id": "br-NOTEXIST" }
  }
}
```

## Codes

`ISSUE_NOT_FOUND` covers lookups by issue ID. Entities that are looked up by name rather than
by ID — a merge slot, a federation peer, a worktree — report `NOT_FOUND` (also exit code 3)
with the entity kind in `context.kind`:

```json
{
  "error": {
    "code": "NOT_FOUND",
    "message": "merge slot not found: br-demo-merge-slot",
    "hint": "Run 'br merge-slot create' to create the merge slot.",
    "retryable": false,
    "context": { "kind": "merge slot", "id": "br-demo-merge-slot" }
  }
}
```

The full code-to-exit-code table is in [../TROUBLESHOOTING.md](../TROUBLESHOOTING.md#error-code-reference).

Machine-readable schema:

```bash
br schema error --format json
```

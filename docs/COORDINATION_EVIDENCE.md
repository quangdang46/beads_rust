# Coordination Evidence Contract

Status: design and helper contract for future coordination CLI, MCP, scheduler,
and audit work. There is no user-facing coordination command yet.

## Purpose

Large agent swarms can leave work hidden in `in_progress` when a process dies or
an operator loses track of panes. `br ready` correctly hides active claims, but
operators still need a read-only way to decide whether a hidden claim is fresh,
stale, ambiguous, or protected by an active Agent Mail reservation.

The durable contract is `br.coordination.v1`, implemented as pure data types and
classification helpers in `src/coordination.rs`.

## Non-Goals

- No automatic claim stealing.
- No live Agent Mail calls from `br`.
- No git operations.
- No background daemon.
- No command that mutates issue status or assignee.

## Evidence Inputs

The classifier uses only caller-provided evidence:

- Issue metadata: status, assignee, and `updated_at`.
- Owner kind: `swarm_agent`, `human`, or `unknown`.
- Optional Agent Mail snapshot evidence supplied by a caller or fixture.

Missing Agent Mail data is explicit evidence, not proof of abandonment.

## Policy Matrix

| Condition | Classification | Recommended action |
| --- | --- | --- |
| Empty or whitespace assignee | `unassigned` | `observe` |
| Age below owner threshold | `fresh` | `observe` |
| Active matching reservation after stale threshold | `blocked_by_active_reservation` | `leave_active` |
| Stale age but no Agent Mail snapshot | `no_mail_snapshot` | `inspect_mail` |
| Stale age with invalid snapshot | `ambiguous` | `inspect_mail` |
| Stale swarm-agent age with no active reservation | `stale_candidate` | `reclaim_candidate` |
| Abandoned-likely swarm-agent age with no active reservation | `abandoned_likely` | `reclaim_candidate` |
| Stale human or unknown owner with no active reservation | `stale_candidate` | `ask_owner` |
| Abandoned-likely human or unknown owner with no active reservation | `abandoned_likely` | `ask_owner` |

Default thresholds:

- Swarm-agent stale candidate: 120 minutes.
- Swarm-agent abandoned-likely marker: 480 minutes.
- Human or unknown stale candidate: 1440 minutes.
- Human or unknown abandoned-likely marker: 4320 minutes.

These thresholds match the existing AGENTS.md guidance that swarm-agent claims
are stale candidates after two quiet hours, while human or unclear claims use a
one-business-day rule of thumb. The abandoned-likely marker is deliberately more
conservative and remains advisory.

## Output Shape

The top-level machine-readable envelope is:

```text
CoordinationStatusOutput {
  schema_version: "br.coordination.v1",
  generated_at: DateTime<Utc>,
  summary: CoordinationSummary,
  claims: [ClaimAssessment]
}
```

Each claim assessment includes:

- assignee after trimming whitespace,
- owner kind,
- updated timestamp and computed age in minutes,
- stale and abandoned thresholds,
- reservation evidence,
- classification,
- recommended action,
- evidence source list.

Future CLI and MCP surfaces should expose this shape directly in JSON mode and
may convert it to TOON using the normal output layer. Human text output should be
a projection of the same fields, not a separate policy.

## Agent Mail Boundary

Agent Mail remains the collision-avoidance layer. `br` must not depend on a live
MCP service. Future commands may accept explicit snapshot files or stdin payloads
that describe reservations and agent liveness, but absence of that snapshot must
be reported as `no_mail_snapshot` or `ambiguous`, never as proof that reclaiming
is safe.

## Scheduler Boundary

The scheduler may reuse the same age and owner-kind thresholds. It should not
claim that a stale issue is abandoned unless it also receives trustworthy
reservation evidence. When in doubt, scheduler output should point agents to the
coordination status surface for deeper diagnosis.

## Reclaim Boundary

`reclaim_candidate` is advisory. The documented safe sequence still requires an
audit comment before any claim update:

```bash
br comments add <id> --author "$AGENT_NAME" \
  --message "reclaim: previous in_progress claim appears abandoned; evidence: updated_at=<timestamp>, assignee=<name>, no active reservation or pane" \
  --json
br update <id> --claim --json
```

Human or unknown ownership keeps the safer `ask_owner` recommendation even after
the stale threshold.

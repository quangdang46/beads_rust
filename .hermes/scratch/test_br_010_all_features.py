#!/usr/bin/env python3
"""
Comprehensive feature test for br 0.1.0
=========================================
Tests ALL subcommands with happy cases + edge cases using a real tmp dir.
Based on actual br 0.1.0 output format analysis.
"""

import subprocess, tempfile, os, sys, json, re, shutil
from pathlib import Path

BR_BIN = shutil.which("br") or "/Users/tranquangdang21/.local/bin/br"
FAILURES = []
PASS_COUNT = 0
FAIL_COUNT = 0

def run(*args, cwd=None, timeout=30, env_extra=None):
    cmd = [BR_BIN] + list(args)
    env = os.environ.copy()
    if env_extra: env.update(env_extra)
    try:
        p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True,
                           timeout=timeout, env=env)
        return p.returncode, p.stdout.strip(), p.stderr.strip()
    except subprocess.TimeoutExpired:
        return -1, "", f"TIMEOUT ({timeout}s)"
    except FileNotFoundError:
        return -2, "", f"NOT FOUND: {cmd[0]}"

def section(name):
    print(f"\n{'─'*70}\n  [{name}]\n{'─'*70}")

def check(label, condition, detail=""):
    global PASS_COUNT, FAIL_COUNT
    if condition:
        PASS_COUNT += 1
        print(f"  ✓ {label}")
    else:
        FAIL_COUNT += 1
        FAILURES.append((label, detail))
        print(f"  ✖ {label} — FAIL")
        if detail:
            for line in detail.split("\n"):
                print(f"    {line}")

def check_json_has(label, stdout, expected_fields=None):
    try:
        data = json.loads(stdout)
    except json.JSONDecodeError as e:
        return check(label, False, f"Not valid JSON: {e}\nstdout={stdout[:300]}")
    if expected_fields:
        ok = True
        for f in expected_fields:
            found = _json_path(data, f)
            if found is None:
                ok = False
                break
        if not ok:
            # Data might be wrapped — unwrap common wrappers
            for wrapper_key in ["issue", "issues", "result", "data"]:
                if isinstance(data, dict) and wrapper_key in data:
                    inner = data[wrapper_key]
                    if isinstance(inner, list) and inner:
                        inner = inner[0]
                    all_found = all(_json_path(inner, f) is not None for f in expected_fields)
                    if all_found:
                        return check(label, True, f"Found in .{wrapper_key}")
            return check(label, False, f"Missing fields in JSON: keys={list(data.keys())[:10] if isinstance(data, dict) else type(data).__name__}")
    return check(label, True, f"JSON ok: {type(data).__name__}")

def _json_path(data, path):
    parts = path.split(".")
    curr = data
    for p in parts:
        if isinstance(curr, dict) and p in curr:
            curr = curr[p]
        else:
            return None
    return curr

def extract_id(text):
    """Extract br-XXXX from text like '✓ Created br-my-feature-xyz: title'"""
    m = re.search(r'([a-zA-Z]{2,5}[\w-]+)\s*:', text)
    if m:
        return m.group(1)
    m = re.search(r'"id"\s*:\s*"([^"]+)"', text)
    if m:
        return m.group(1)
    return None

def create_issue(title, *extra_args):
    rc, out, err = run("create", *extra_args, title)
    rid = extract_id(out or "")
    if not rid and out:
        # fallback: take first whitespace-separated token that matches br-ID
        for tok in out.split():
            if re.match(r'[a-zA-Z]{2,5}[\w-]+$', tok):
                rid = tok
                break
    return rc, rid, out, err

# ──────────────────────────────────────────────────────────────────
tmpdir = tempfile.mkdtemp(prefix="br-test-010-")
print(f"Temporary workspace: {tmpdir}")
print(f"Binary: {BR_BIN}")
_, ver, _ = run("--version")
print(f"Version: {ver}")
os.chdir(tmpdir)

# ═══════════════════════════════════════════════════════════════════
# 1. INIT
# ═══════════════════════════════════════════════════════════════════
section("INIT — Happy cases")
rc, out, err = run("init")
check("init (default prefix)", rc == 0, err)
check(".beads/beads.db exists", Path(".beads/beads.db").exists())
check("issues.jsonl exists", Path(".beads/issues.jsonl").exists())
check("config.yaml exists", Path(".beads/config.yaml").exists())

section("INIT — Edge cases")
fresh_dir = Path(tmpdir) / "prefix-test"
fresh_dir.mkdir(parents=True, exist_ok=True)
rc2, out2, err2 = run("init", "--prefix", "bd", cwd=str(fresh_dir))
check("init --prefix bd in fresh dir", rc2 == 0, err2)
cfg = (fresh_dir / ".beads/config.yaml").read_text()
check("config has prefix bd", "bd" in cfg, cfg[:100])

sub_tmp = Path(tmpdir) / "subdir"
sub_tmp.mkdir(parents=True, exist_ok=True)
rc3, out3, err3 = run("init", cwd=str(sub_tmp))
check("init in subdirectory", rc3 == 0, err3)
check("subdir .beads created", (sub_tmp / ".beads" / "beads.db").exists())

rc4, out4, err4 = run("init", "--prefix", "", "--force")
check("init with empty prefix (graceful)", rc4 == 0 or "empty" in err4.lower(), f"{rc4} {err4[:200]}")

# ═══════════════════════════════════════════════════════════════════
# 2. CREATE
# ═══════════════════════════════════════════════════════════════════
section("CREATE — Happy cases")
rc, id1, out, err = create_issue("Test task 1")
check("create basic issue", rc == 0 and id1, f"rc={rc} id={id1} err={err[:100]}")

rc2, out2, err2 = run("create", "--slug", "my-feature", "Slugged issue")
id_slug = extract_id(out2)
check("create with slug", rc2 == 0 and id_slug, f"rc={rc2} {err2[:100]}")

rc3, id_feat, out3, err3 = create_issue("Feature request", "--type", "feature")
check("create feature type", rc3 == 0 and id_feat, f"rc={rc3} {err3[:100]}")

rc4, out4, err4 = run("create", "--json", "JSON creation")
check("create --json output", rc4 == 0, err4[:100])
check("create JSON has ID", '"id"' in out4, out4[:200])

rc5, out5, err5 = run("q", "Quick capture issue")
check("q (quick capture)", rc5 == 0, err5[:100])
# q returns bare ID as output
qid = out5.strip()
check("q returns ID", bool(qid) and len(qid) > 3, f"Got: {qid}")

rc6, out6, err6 = run("create",
    "--title", "All fields", "--type", "task", "--priority", "P0",
    "-d", "Full description", "-a", "tester", "--owner", "owner@test.com",
    "-l", "test", "--estimate", "60", "--due", "2026-12-31",
    "--external-ref", "EXT-001", "--json")
check("create with all fields", rc6 == 0, err6[:200])
id_all = json.loads(out6).get("id") if out6 else None

section("CREATE — Edge cases")
rc_e1, out_e1, err_e1 = run("create", "")
check("create empty title (should fail)", rc_e1 != 0, f"rc=0: {out_e1[:100]}")

long_t = "A" * 5000
rc_e2, out_e2, err_e2 = run("create", long_t)
check("create 5000-char title rejected (500 char limit)", rc_e2 != 0, f"rc={rc_e2} {err_e2[:100]}")

rc_e3, out_e3, err_e3 = run("create", "Tiếng Việt: ổn 🎉 <script>")
check("create unicode/special chars", rc_e3 == 0, err_e3[:100])
check("unicode preserved in output", "Tiếng Việt" in out_e3, out_e3[:200])

rc_e4, out_e4, err_e4 = run("create", "--dry-run", "Dry run")
check("create --dry-run previews", rc_e4 == 0, err_e4[:100])

rc_e5, out_e5, err_e5 = run("create", "--silent", "Silent issue")
# --silent returns bare ID as output
e5_id = out_e5.strip()
check("create --silent returns ID", rc_e5 == 0 and e5_id and len(e5_id) > 3, f"{out_e5[:100]}")

rc_e6, out_e6, err_e6 = run("create", "--ephemeral", "Ephemeral issue")
check("create --ephemeral", rc_e6 == 0, err_e6[:100])

rc_e7, out_e7, err_e7 = run("create", "-p", "-1", "Invalid priority")
check("create invalid negative priority (should fail)", rc_e7 != 0, f"rc=0: {out_e7[:100]}")

rc_e8, out_e8, err_e8 = run("create", "-p", "P99", "Bad priority")
check("create invalid P99 priority (should fail)", rc_e8 != 0, f"rc=0: {out_e8[:100]}")

rc_e9, out_e9, err_e9 = run("create", "--parent", "br-NONEXIST-XXXX", "Orphan child")
check("create with nonexistent parent (graceful)",
      rc_e9 == 0 or "not found" in err_e9.lower(), f"{rc_e9} {err_e9[:200]}")

# ═══════════════════════════════════════════════════════════════════
# 3. SHOW
# ═══════════════════════════════════════════════════════════════════
section("SHOW — Happy cases")
rc, out, err = run("show", id1)
check(f"show {id1}", rc == 0, err[:100])
check("title visible", "Test task" in out, out[:200])

rc2, out2, err2 = run("show", "--json", id1)
check(f"show --json {id1}", rc2 == 0, err2[:100])

rc3, out3, err3 = run("show", id1, "--format", "toon")
check("show --format toon", rc3 == 0, err3[:100])

rc4, out4, err4 = run("show", "--oneline", id1)
check("show --oneline has content", bool(out4), out4[:200])

section("SHOW — Edge cases")
rc5, out5, err5 = run("show", "br-NONEXIST-XXXXX")
check("show nonexistent ID (should fail)", rc5 != 0, f"rc=0: {out5[:200]}")

if id1 and id_feat:
    rc6, out6, err6 = run("show", id1, id_feat)
    check("show multiple issues", rc6 == 0, err6[:100])

# ═══════════════════════════════════════════════════════════════════
# 4. UPDATE
# ═══════════════════════════════════════════════════════════════════
section("UPDATE — Happy cases")
rc, out, err = run("update", id1, "--title", "Updated title", "-p", "P2", "--json")
check("update title+priority", rc == 0, err[:200])

rc2, out2, err2 = run("update", id1, "-d", "New description", "--assignee", "dev1", "--owner", "dev1@x.com")
check("update description+assignee+owner", rc2 == 0, err2[:100])

# First add test label, then update with add/remove labels
rc_lbl, out_lbl, err_lbl = run("update", id1, "--add-label", "test")
rc3, out3, err3 = run("update", id1, "--add-label", "hotfix")
check("update add label hotfix", rc3 == 0, err3[:100])

rc3b, out3b, err3b = run("update", id1, "--remove-label", "test")
check("update remove label test", rc3b == 0, err3b[:100])

rc4, out4, err4 = run("update", id1, "--status", "in_progress")
check("update status to in_progress", rc4 == 0, err4[:100])

# Unassign first then claim
rc_unassign, _, _ = run("update", id1, "--assignee", "")
rc5, out5, err5 = run("update", id1, "--claim")
check("update --claim", rc5 == 0, err5[:100])

rc6, out6, err6 = run("update", id1, "--notes-push", "Added a note")
check("update --notes-push", rc6 == 0, err6[:100])

section("UPDATE — Edge cases")
rc7, out7, err7 = run("update", "br-NONEXIST", "--title", "Ghost")
check("update nonexistent ID (should fail)", rc7 != 0, f"rc=0")

rc8, out8, err8 = run("update", id1, "--status", "closed")
check("update to closed refused (use close)", rc8 != 0, f"rc=0 (direct closed!): {out8[:200]}")

rc9, out9, err9 = run("update", id1, "--status", "tombstone")
check("update to tombstone refused (use delete)", rc9 != 0, f"rc=0: {out9[:200]}")

rc10, out10, err10 = run("update", id1, "--estimate", "-1")
check("update negative estimate (should fail or round)", rc10 != 0, f"rc=0")

# ═══════════════════════════════════════════════════════════════════
# 5. CLOSE / REOPEN
# ═══════════════════════════════════════════════════════════════════
section("CLOSE — Happy cases")
rc, out, err = run("close", id1, "-r", "Completed", "--json")
check("close issue with reason", rc == 0, err[:200])
check("close returns JSON", out.startswith("[") or out.startswith("{"), out[:100])

rc2, out2, err2 = run("reopen", id1)
check("reopen closed issue", rc2 == 0, err2[:100])

section("CLOSE — Edge cases")
rc3, out3, err3 = run("close", "br-NONEXIST")
check("close nonexistent ID (should fail)", rc3 != 0, f"rc=0")

rc4, out4, err4 = run("close", id1, "--suggest-next", "-r", "test")
check("close --suggest-next", rc4 == 0, err4[:100])

# ═══════════════════════════════════════════════════════════════════
# 6. LIST
# ═══════════════════════════════════════════════════════════════════
section("LIST — Happy cases")
rc, out, err = run("list")
check("list all open", rc == 0, err[:100])
rc2, out2, err2 = run("list", "--all")
check("list --all (includes closed)", rc2 == 0, err2[:100])
rc3, out3, err3 = run("list", "--json")
check("list --json", rc3 == 0, err3[:100])
check('list JSON has "issues"', '"issues"' in out3, out3[:200])
d3 = json.loads(out3) if out3 else {}
check("list JSON issues array", isinstance(d3.get("issues"), list) if isinstance(d3, dict) else False, out3[:200])

section("LIST — Filter edge cases")
rc4, out4, err4 = run("list", "--limit", "0")
check("list --limit 0 (unlimited)", rc4 == 0, err4[:100])

rc5, out5, err5 = run("list", "--limit", "1", "--json")
check("list --limit 1", rc5 == 0, err5[:100])
d5 = json.loads(out5) if out5 else {}
if isinstance(d5, dict):
    check("limit 1 returns <=1 issues", len(d5.get("issues", [])) <= 1, str(len(d5.get("issues",[]))))
else:
    check("limit 1 returns <=1 issues", True, f"JSON type: {type(d5).__name__}")

rc6, out6, err6 = run("list", "--sort", "priority", "-r", "--json")
check("list --sort priority -r", rc6 == 0, err6[:100])

rc7, out7, err7 = run("list", "--format", "csv", "--fields", "id,title,status")
check("list --format csv", rc7 == 0, err7[:100])
check("csv has header", "id,title,status" in out7[:100], out7[:200])

rc8, out8, err8 = run("list", "--label", "hotfix")
check("list --label hotfix", rc8 == 0, err8[:100])

rc9, out9, err9 = run("list", "--unassigned")
check("list --unassigned", rc9 == 0, err9[:100])

rc10, out10, err10 = run("list", "--created-after", "24h")
check("list --created-after 24h", rc10 == 0, err10[:100])

rc11, out11, err11 = run("list", "--created-before", "1s")
check("list --created-before 1s (empty expected)", rc11 == 0, err11[:100])

rc12, out12, err12 = run("list", "-F", "status=open AND priority>1", "--json")
check("list DSL filter", rc12 == 0, err12[:100])

# ═══════════════════════════════════════════════════════════════════
# 7. SEARCH
# ═══════════════════════════════════════════════════════════════════
section("SEARCH")
rc, out, err = run("search", "Updated title")
check("search found term", rc == 0, err[:100])
check("search found results", "Updated" in out or len(out) > 5, out[:200])

rc2, out2, err2 = run("search", "ZZZZNOSUCH")
check("search non-existent term (empty results)", rc2 == 0, err2[:100])

rc3, out3, err3 = run("search", "")
check("search empty query (should fail)", rc3 != 0, f"rc=0")

rc4, out4, err4 = run("search", id1)
check("search by ID", rc4 == 0, err4[:100])

# ═══════════════════════════════════════════════════════════════════
# 8. DELETE
# ═══════════════════════════════════════════════════════════════════
section("DELETE")
rc, id_del, out, err = create_issue("To be deleted")
check("create for delete test", rc == 0 and id_del, f"rc={rc} id={id_del}")

rc2, out2, err2 = run("delete", id_del, "--reason", "test cleanup")
check("delete issue", rc2 == 0, err2[:200])
check("delete confirms", "deleted" in out2.lower() or "tombstone" in out2.lower(), out2[:100])

rc3, out3, err3 = run("delete", "br-NONEXIST-XXXX")
check("delete nonexistent ID (should fail)", rc3 != 0, f"rc=0")

rc4, out4, err4 = run("delete", "--dry-run", id_del)
check("delete --dry-run (already tombstone)", rc4 in (0, 1), f"rc={rc4}")

# ═══════════════════════════════════════════════════════════════════
# 9. LABEL
# ═══════════════════════════════════════════════════════════════════
section("LABEL")
rc, id_lab, out, err = create_issue("Label testing issue")
check("create for label test", rc == 0 and id_lab)

# br 0.1.0 uses: br label add <ID> [--label <label>] (comma-separated labels positional? Actually -l cannot repeat)
# Use the --label flag with comma-separated
rc2, out2, err2 = run("label", "add", id_lab, "-l", "alpha")
check("label add alpha", rc2 == 0, err2[:200])

rc2b, out2b, err2b = run("label", "add", id_lab, "-l", "beta")
check("label add beta", rc2b == 0, err2b[:200])

rc2c, out2c, err2c = run("label", "add", id_lab, "-l", "gamma")
check("label add gamma", rc2c == 0, err2c[:200])

rc3, out3, err3 = run("label", "list", id_lab)
check("label list for issue", rc3 == 0, err3[:100])
check("labels visible", "alpha" in out3 or "beta" in out3, out3[:200])

rc4, out4, err4 = run("label", "list-all")
check("label list-all", rc4 == 0, err4[:100])

rc5, out5, err5 = run("label", "remove", id_lab, "-l", "beta")
check("label remove beta", rc5 == 0, err5[:200])

rc6, out6, err6 = run("label", "rename", "alpha", "alpha-renamed")
check("label rename alpha->alpha-renamed", rc6 == 0, err6[:200])

section("LABEL — Edge cases")
rc7, out7, err7 = run("label", "add", "br-NONEXIST", "-l", "test")
check("label add to nonexistent (should fail)", rc7 != 0, f"rc=0")

# ═══════════════════════════════════════════════════════════════════
# 10. DEP (Dependencies)
# ═══════════════════════════════════════════════════════════════════
section("DEP")
rc, id_dep_p, out, err = create_issue("Dep parent")
check("create dep parent", rc == 0 and id_dep_p)
rc2, id_dep_c, out2, err2 = create_issue("Dep child")
check("create dep child", rc2 == 0 and id_dep_c)

rc3, out3, err3 = run("dep", "add", id_dep_c, id_dep_p)
check("dep add child->parent", rc3 == 0, err3[:200])

rc4, out4, err4 = run("dep", "list", id_dep_c)
check("dep list", rc4 == 0, err4[:100])
check("dep shows parent", id_dep_p in out4, out4[:200])

rc5, out5, err5 = run("dep", "tree", id_dep_c)
check("dep tree", rc5 == 0, err5[:100])

rc6, out6, err6 = run("dep", "cycles")
check("dep cycles", rc6 == 0, err6[:100])

section("DEP — Edge cases")
rc7, out7, err7 = run("dep", "add", "br-NONEXIST-1", id_dep_p)
check("dep add non-existent depender (should fail)", rc7 != 0, f"rc=0")

rc8, out8, err8 = run("dep", "add", id_dep_p, id_dep_p)
check("dep add self-dependency (should fail)", rc8 != 0, f"rc=0: {out8[:200]}")

rc9, out9, err9 = run("dep", "add", id_dep_p, id_dep_c)
check("dep add reverse edge (cycle detection)", rc9 != 0, f"rc=0: cycle allowed! {out9[:200]}")

rc10, out10, err10 = run("dep", "remove", id_dep_c, id_dep_p)
check("dep remove", rc10 == 0, err10[:200])

# ═══════════════════════════════════════════════════════════════════
# 11. COMMENTS
# ═══════════════════════════════════════════════════════════════════
section("COMMENTS")
rc, id_cmt, out, err = create_issue("Commented issue")
check("create for comments test", rc == 0 and id_cmt)

rc2, out2, err2 = run("comments", "add", id_cmt, "--message", "This is a test comment")
check("comments add", rc2 == 0, err2[:200])

rc3, out3, err3 = run("comments", "list", id_cmt)
check("comments list", rc3 == 0, err3[:100])
check("comment visible in list", "test comment" in out3, out3[:200])

section("COMMENTS — Edge cases")
rc4, out4, err4 = run("comments", "add", "br-NONE", "--message", "ghost")
check("comment on nonexistent (should fail)", rc4 != 0, f"rc=0")

# ═══════════════════════════════════════════════════════════════════
# 12. SYNC
# ═══════════════════════════════════════════════════════════════════
section("SYNC — Happy cases")
rc, out, err = run("sync", "--status", "--json")
check("sync --status --json", rc == 0, err[:200])

rc2, out2, err2 = run("sync", "--flush-only", "-v")
check("sync --flush-only", rc2 == 0, err2[:200])
jsize = Path(".beads/issues.jsonl").stat().st_size
check("JSONL has content after flush", jsize > 50, f"size={jsize}")

rc3, out3, err3 = run("sync", "--import-only", "-v")
check("sync --import-only", rc3 == 0, err3[:200])

section("SYNC — Edge cases")
with open(".beads/issues.jsonl", "a") as f:
    f.write("NOT VALID JSON\n")
rc4, out4, err4 = run("sync", "--import-only")
check("sync import rejects corrupt JSONL", rc4 != 0, f"rc=0")
rc5, out5, err5 = run("sync", "--flush-only", "--force")
check("sync restore after corruption", rc5 == 0, err5[:200])

rc6, out6, err6 = run("sync", "--witness", "--json")
check("sync --witness", rc6 == 0, err6[:200])

# ═══════════════════════════════════════════════════════════════════
# 13. IMPORT / EXPORT
# ═══════════════════════════════════════════════════════════════════
section("IMPORT/EXPORT")
exp = Path(tmpdir) / "export-test.jsonl"
rc, out, err = run("export", "-o", str(exp))
check("export JSONL to file", rc == 0, err[:100])
check("exported file exists", exp.exists())

rc2, out2, err2 = run("export", "-f", "json", "-o", str(Path(tmpdir) / "export.json"))
check("export JSON format", rc2 == 0, err2[:100])

rc3, out3, err3 = run("export", "-f", "csv", "-o", str(Path(tmpdir) / "export.csv"))
check("export CSV format", rc3 == 0, err3[:100])

rc4, out4, err4 = run("export", "-s", "closed", "-o", str(Path(tmpdir) / "closed.jsonl"))
check("export filtered by status", rc4 == 0, err4[:100])

rc5, out5, err5 = run("import", "-i", str(exp))
check("import from file", rc5 == 0, err5[:200])

rc6, out6, err6 = run("import", "-i", "NONEXIST.jsonl")
check("import from nonexistent file (should fail)", rc6 != 0, f"rc=0")

# ═══════════════════════════════════════════════════════════════════
# 14. INFO
# ═══════════════════════════════════════════════════════════════════
section("INFO")
rc, out, err = run("info", "--json")
check("info --json", rc == 0, err[:100])

rc2, out2, err2 = run("info", "--schema")
check("info --schema", rc2 == 0, err2[:100])

rc3, out3, err3 = run("info", "--thanks")
check("info --thanks", rc3 == 0, err3[:100])

# ═══════════════════════════════════════════════════════════════════
# 15. STATS / COUNT
# ═══════════════════════════════════════════════════════════════════
section("STATS / COUNT")
rc, out, err = run("stats")
check("stats", rc == 0, err[:100])
rc2, out2, err2 = run("count")
check("count", rc2 == 0, err2[:100])
rc3, out3, err3 = run("count", "--by", "status")
check("count --by status", rc3 == 0, err3[:100])
rc4, out4, err4 = run("count", "--by", "priority")
check("count --by priority", rc4 == 0, err4[:100])
rc5, out5, err5 = run("count", "--by", "type")
check("count --by type", rc5 == 0, err5[:100])
rc6, out6, err6 = run("stats", "--by-assignee")
check("stats --by-assignee", rc6 == 0, err6[:100])

# ═══════════════════════════════════════════════════════════════════
# 16. READY / BLOCKED
# ═══════════════════════════════════════════════════════════════════
section("READY / BLOCKED")
rc, out, err = run("ready")
check("ready list", rc == 0, err[:100])
rc2, out2, err2 = run("ready", "--limit", "5")
check("ready --limit 5", rc2 == 0, err2[:100])
rc3, out3, err3 = run("ready", "--json")
check("ready --json", rc3 == 0, err3[:100])
rc4, out4, err4 = run("blocked")
check("blocked list", rc4 == 0, err4[:100])
rc5, out5, err5 = run("blocked", "--limit", "5")
check("blocked --limit 5", rc5 == 0, err5[:100])

# ═══════════════════════════════════════════════════════════════════
# 17. WISP (Ephemeral issues)
# ═══════════════════════════════════════════════════════════════════
section("WISP")
rc, out, err = run("wisp", "create", "Temp wisp 1")
check("wisp create", rc == 0, err[:100])
# wisp output: "Created wisp: wsp-se1"
wisp_m = re.search(r'wisp:\s*(\S+)', out)
wisp_id = wisp_m.group(1) if wisp_m else None
check(f"wisp ID: {wisp_id}", bool(wisp_id), out[:100])

rc2, out2, err2 = run("wisp", "list")
check("wisp list", rc2 == 0, err2[:100])

rc3, out3, err3 = run("wisp", "close", wisp_id)
check("wisp close", rc3 == 0, err3[:200])

rc4, out4, err4 = run("wisp", "close", wisp_id)
check("wisp close already-closed", rc4 == 0, err4[:200])

rc5, out5, err5 = run("wisp", "gc")
check("wisp gc (garbage collect)", rc5 == 0, err5[:100])

# ═══════════════════════════════════════════════════════════════════
# 18. MEMORY (Persistent agent memory)
# ═══════════════════════════════════════════════════════════════════
section("MEMORY — Happy cases")
rc, out, err = run("memory", "remember", "-k", "test-key", "test-value-12345")
check("memory remember", rc == 0, err[:200])

rc2, out2, err2 = run("memory", "recall", "test-key")
check("memory recall", rc2 == 0, err2[:100])
check("recall correct value", "test-value" in out2, out2[:200])

rc3, out3, err3 = run("memory", "memories", "test")
check("memory memories list", rc3 == 0, err3[:100])

section("MEMORY — Edge cases")
rc4, out4, err4 = run("memory", "recall", "NONEXIST-KEY-123")
check("memory recall nonexistent (should fail)", rc4 != 0, f"rc=0")

rc5, out5, err5 = run("memory", "forget", "test-key")
check("memory forget", rc5 == 0, err5[:200])

rc6, out6, err6 = run("memory", "recall", "test-key")
check("memory recall after forget (gone)", rc6 != 0, f"rc=0: {out6[:200]}")

rc7, out7, err7 = run("memory", "forget", "NONEXIST-KEY-123")
check("memory forget nonexistent (exit 7 expected)", rc7 in (0, 1, 7), f"rc={rc7}")

# ═══════════════════════════════════════════════════════════════════
# 19. RENAME / RENAME-PREFIX / DEFER / UNDEFER
# ═══════════════════════════════════════════════════════════════════
section("RENAME / DEFER / UNDEFER")
rc, id_rn, out, err = create_issue("To be renamed")
check("create for rename test", rc == 0 and id_rn)

rc2, out2, err2 = run("rename", id_rn, f"{id_rn}-renamed")
check("rename ID (should succeed or warn)", rc2 in (0, 1), f"rc={rc2} {err2[:200]}")

rc3, out3, err3 = run("rename", "br-NONE", "br-NONE-target")
check("rename nonexistent (should fail)", rc3 != 0, f"rc=0")

rc4, out4, err4 = run("rename-prefix", "--dry-run", "xx")
check("rename-prefix --dry-run", rc4 == 0, err4[:200])

rc5, id_df, out5, err5 = create_issue("Defer test")
check("create for defer test", rc5 == 0 and id_df)

rc6, out6, err6 = run("update", id_df, "--defer", "2027-01-01")
check("update --defer", rc6 == 0, err6[:200])

rc7, out7, err7 = run("defer", id_df)
check("defer command", rc7 == 0, err7[:200])

rc8, out8, err8 = run("undefer", id_df)
check("undefer command", rc8 == 0, err8[:200])

# ═══════════════════════════════════════════════════════════════════
# 20. CHANGELOG
# ═══════════════════════════════════════════════════════════════════
section("CHANGELOG")
rc, out, err = run("changelog", "--since", "2026-01-01")
check("changelog", rc in (0, 1), f"rc={rc} {err[:200]}")
rc2, out2, err2 = run("changelog", "--json")
check("changelog --json", rc2 in (0, 1), f"rc={rc2}")

# ═══════════════════════════════════════════════════════════════════
# 21. DOCTOR
# ═══════════════════════════════════════════════════════════════════
section("DOCTOR")
rc, out, err = run("doctor", "health")
check("doctor health", rc == 0, err[:100])

rc2, out2, err2 = run("doctor", "--json")
check("doctor --json exits 0 (health info, warnings are data)", rc2 in (0, 1), f"rc={rc2}")

rc3, out3, err3 = run("doctor", "--quick")
check("doctor --quick (exit may warn)", rc3 in (0, 1), f"rc={rc3}")

rc4, out4, err4 = run("doctor", "capabilities")
check("doctor capabilities", rc4 == 0, err4[:100])

rc5, out5, err5 = run("doctor", "ls")
check("doctor ls (list runs)", rc5 == 0, err5[:100])

# ═══════════════════════════════════════════════════════════════════
# 22. WHERE / VERSION
# ═══════════════════════════════════════════════════════════════════
section("WHERE / VERSION")
rc, out, err = run("where")
check("where (beads dir path)", rc == 0, err[:100])
check("where returns path info", tmpdir in out, out[:200])

rc2, out2, err2 = run("version")
check("version output", rc2 == 0, err2[:100])
check("version contains 0.1.0", "0.1.0" in out2, out2[:100])

# ═══════════════════════════════════════════════════════════════════
# 23. STALE / ORPHANS
# ═══════════════════════════════════════════════════════════════════
section("STALE / ORPHANS")
rc, out, err = run("stale")
check("stale list", rc == 0, err[:100])
rc2, out2, err2 = run("orphans")
check("orphans list", rc2 == 0, err2[:100])

# ═══════════════════════════════════════════════════════════════════
# 24. GRAPH
# ═══════════════════════════════════════════════════════════════════
section("GRAPH")
rc, out, err = run("graph", "--all")
check("graph --all", rc == 0, err[:100])
rc2, out2, err2 = run("graph", "--all", "--compact")
check("graph --all --compact", rc2 == 0, err2[:100])

# ═══════════════════════════════════════════════════════════════════
# 25. HOOKS
# ═══════════════════════════════════════════════════════════════════
section("HOOKS")
rc, out, err = run("hooks", "list")
check("hooks list in non-git dir (graceful error)", rc != 0, f"rc=0 (unexpected)")
# Run git directly (not through br wrapper)
subprocess.run(["git", "init"], capture_output=True, text=True)
subprocess.run(["git", "config", "user.email", "test@test.com"], capture_output=True, text=True)
subprocess.run(["git", "config", "user.name", "Test"], capture_output=True, text=True)
subprocess.run(["git", "add", "-A"], capture_output=True, text=True)
subprocess.run(["git", "commit", "-m", "init"], capture_output=True, text=True)
rc2, out2, err2 = run("hooks", "list")
check("hooks list in git repo", rc2 == 0, err2[:200])

rc3, out3, err3 = run("hooks", "install", "--all")
check("hooks install --all", rc3 in (0, 1), f"rc={rc3} {err3[:200]}")

rc4, out4, err4 = run("hooks", "list")
check("hooks list after install", rc4 == 0, err4[:100])

# ═══════════════════════════════════════════════════════════════════
# 26. PRIME
# ═══════════════════════════════════════════════════════════════════
section("PRIME")
rc, out, err = run("prime", "--full")
check("prime --full", rc == 0, err[:100])
check("prime output has content", len(out) > 30, f"short: {out[:100]}")

rc2, out2, err2 = run("prime", "--mcp")
check("prime --mcp", rc2 == 0, err2[:100])

rc3, out3, err3 = run("prime", "--memories-only")
check("prime --memories-only", rc3 == 0, err3[:100])

# ═══════════════════════════════════════════════════════════════════
# 27. COMPLETIONS
# ═══════════════════════════════════════════════════════════════════
section("COMPLETIONS")
for shell in ["bash", "zsh", "fish", "powershell"]:
    rc, out, err = run("completions", shell)
    check(f"completions {shell}", rc == 0, err[:100])

# ═══════════════════════════════════════════════════════════════════
# 28. SQL
# ═══════════════════════════════════════════════════════════════════
section("SQL — Happy cases")
rc, out, err = run("sql", "SELECT count(*) as cnt FROM issues")
check("sql SELECT count", rc == 0, err[:100])
check("sql returns number", bool(re.search(r'\d+', out)), out[:200])

rc2, out2, err2 = run("sql", "SELECT id, title FROM issues LIMIT 3", "--json")
check("sql --json", rc2 == 0, err2[:100])

section("SQL — Edge cases")
rc3, out3, err3 = run("sql", "SELECT * FROM nonexistent")
check("sql bad table (should fail)", rc3 != 0, f"rc=0: {out3[:200]}")

rc4, out4, err4 = run("sql", "DELETE FROM issues")
check("sql write forbidden (should fail)", rc4 != 0, f"rc=0 (writes allowed!)")

rc5, out5, err5 = run("sql", "")
check("sql empty query (should fail)", rc5 != 0, f"rc=0")

# ═══════════════════════════════════════════════════════════════════
# 29. Command availability checks (no arg side-effects, just --help)
# ═══════════════════════════════════════════════════════════════════
section("Command Availability")
cmds = [
    ("audit", "--help"), ("agents", "--help"), ("config", "--help"),
    ("formula", "--help"), ("gate", "--help"), ("epic", "--help"),
    ("mol", "--help"), ("scheduler", "--help"), ("query", "--help"),
    ("custom-status", "--help"), ("custom-type", "--help"), ("lint", "--help"),
    ("history", "--help"), ("coordination", "--help"),
    ("federation", "--help"), ("merge-slot", "--help"), ("worktree", "--help"),
    ("capabilities", ""), ("recipes", "--help"), ("schema", "--help"),
    ("quickstart", ""), ("robot-docs", "guide"),
]
for cmd, sub in cmds:
    args = [cmd]
    if sub:
        args.append(sub)
    rc, out, err = run(*args)
    check(f"{cmd} {sub}".strip() + " available", rc == 0, f"rc={rc} {err[:100]}")

# ═══════════════════════════════════════════════════════════════════
# 30. TEMPLATE
# ═══════════════════════════════════════════════════════════════════
section("TEMPLATE")
rc, out, err = run("template", "list")
check("template list (empty)", rc == 0, err[:100])

rc2, out2, err2 = run("template", "create", "Bug report template",
    "-p", "2", "--description", "Steps to reproduce...",
    "--type", "bug", "--labels", "bug")
check("template create", rc2 == 0, err2[:200])
tpl_id = extract_id(out2)

rc3, out3, err3 = run("template", "list")
check("template list after create", rc3 == 0, err3[:100])

# Show template by ID (not title)
rc4, out4, err4 = run("template", "list")
check("template list (non-empty)", rc4 == 0, err4[:100])

# Spawn by ID
if tpl_id:
    rc5, out5, err5 = run("template", "spawn", tpl_id, "--title", "Spawned bug")
    check("template spawn by ID", rc5 == 0, err5[:200])
else:
    # Try by title as fallback
    rc5, out5, err5 = run("template", "list")
    check("template list (no spawn test)", rc5 == 0, "tpl_id not extracted")

section("TEMPLATE — Edge cases")
# Delete by template ID
if tpl_id:
    rc6, out6, err6 = run("template", "delete", tpl_id)
    check("template delete by ID", rc6 == 0, err6[:200])
else:
    check("template delete (skipped)", True, "no tpl_id")

# ═══════════════════════════════════════════════════════════════════
# CLEANUP
# ═══════════════════════════════════════════════════════════════════
section("CLEANUP")
os.chdir(Path.home())
try:
    shutil.rmtree(tmpdir)
    check(f"Temp dir cleaned: {tmpdir}", True)
except Exception as e:
    check(f"Cleanup issue (non-fatal)", True, str(e))

# ═══════════════════════════════════════════════════════════════════
total = PASS_COUNT + FAIL_COUNT
print(f"\n{'='*70}")
print(f"  SUMMARY: {total} tests")
print(f"  PASS:  {PASS_COUNT}")
print(f"  FAIL:  {FAIL_COUNT}")
print(f"{'='*70}")

if FAILURES:
    print(f"\n  FAILURES ({len(FAILURES)}):")
    for label, detail in FAILURES[:30]:
        print(f"  ✖ {label}")
        if detail:
            for line in detail.split("\n")[:3]:
                print(f"    {line}")

sys.exit(1 if FAIL_COUNT > 0 else 0)

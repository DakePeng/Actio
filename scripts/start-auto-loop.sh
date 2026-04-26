#!/usr/bin/env bash
# =============================================================================
# start-auto-loop.sh — autonomous improvement loop launcher for D:\Dev\Actio
# =============================================================================
#
# WHAT THIS DOES
#   Boots Claude Code into a recurring `/loop` that, on each tick, either:
#     (A) DISCOVERS issues/improvements/feature ideas and appends them to
#         ISSUES.md, or
#     (B) PICKS UP an open ISSUES.md item and FIXES/BUILDS it, gated by tests
#         and clippy/tsc. Larger items go through the superpowers chain
#         (brainstorming → systematic-debugging → writing-plans →
#          executing-plans → verification → finishing-a-development-branch).
#   All work happens on a sandbox branch (`auto/improvements`); the loop is
#   forbidden from pushing, merging to main, or touching CI.
#
# QUICK START
#   scripts/start-auto-loop.sh                 # 1h interval (best for overnight)
#   scripts/start-auto-loop.sh 30m             # tighter cadence (more quota burn)
#   scripts/start-auto-loop.sh 2h              # conservative; multi-day soak
#   scripts/start-auto-loop.sh --help          # print this header and exit
#
# OPTIONS (env vars)
#   DRY_RUN=1        Set up branch + ISSUES.md + prompt file, then print the
#                    command and exit. Does NOT launch claude.
#   SKIP_BRANCH=1    Don't switch branches; run on whatever is currently
#                    checked out. The loop's HARD RULES still forbid pushes,
#                    so this is safe but you lose the sandbox boundary.
#   BRANCH=<name>    Use a different sandbox branch name (default:
#                    auto/improvements).
#
# ARGUMENTS
#   $1  Interval for /loop. Anything /loop accepts: 30m, 45m, 1h, 90m, 2h, ...
#       Default: 1h. Don't go below the cold-build time of `cargo check`
#       (~10 min on this repo) or ticks will overlap.
#
# WHY INTERVAL-BASED SURVIVES USAGE LIMITS
#   `/loop <interval> <prompt>` is backed by an external cron entry. Each tick
#   is a fresh turn. If a tick aborts because the Anthropic usage limit was
#   hit, the cron entry is untouched — the next scheduled fire just runs the
#   same prompt again, which is exactly what you want for unattended runs.
#   The dynamic / self-paced form (`/loop` with no interval) does NOT survive
#   limits as cleanly, because it relies on the model calling ScheduleWakeup
#   before the turn ends.
#
# WHAT GETS CREATED ON FIRST RUN
#   - Branch `auto/improvements` (from current HEAD), checked out.
#   - `ISSUES.md` at the repo root with empty Open / Resolved sections.
#   - `.claude/auto-improve-loop.prompt.md` containing the full loop prompt
#     (rewritten on every launch so edits to this script propagate).
#   Each is committed to the sandbox branch.
#
# STOPPING THE LOOP
#   Inside the session:   /oh-my-claudecode:cancel
#   From any CC session:  use the CronList tool to find the entry, then
#                         CronDelete to remove it.
#   Killing the terminal: stops the *current* tick but the cron entry remains
#                         and will fire again. Always cancel via the tools
#                         above, not by closing the window.
#
# REVIEWING THE WORK
#   The loop never merges. After a run:
#     git log auto/improvements ^main --oneline   # what changed
#     git diff main..auto/improvements            # full diff
#     cat ISSUES.md                               # queue + resolved log
#   Cherry-pick or squash-merge whatever you want to keep; discard the rest.
#
# CUSTOMIZING THE PROMPT
#   Edit the heredoc inside this script (search for PROMPT_EOF). The next
#   launch will rewrite `.claude/auto-improve-loop.prompt.md`. Editing that
#   file directly works too, but will be overwritten the next time you run
#   this script.
#
# REQUIREMENTS
#   - `git` and `claude` on PATH.
#   - Clean working tree (the script refuses to start otherwise — it would be
#     dangerous to mix the loop's commits with in-progress local work).
#
# =============================================================================

set -euo pipefail

# Handle --help / -h before any side effects. Prints the leading comment block
# (everything between the shebang and the first non-comment line) with the
# leading "# " stripped.
case "${1:-}" in
  -h|--help|help)
    awk 'NR==1 { next } /^[^#]/ { exit } { sub(/^# ?/, ""); print }' "${BASH_SOURCE[0]}"
    exit 0
    ;;
esac

INTERVAL="${1:-1h}"
BRANCH="${BRANCH:-auto/improvements}"
PROMPT_FILE=".claude/auto-improve-loop.prompt.md"
ISSUES_FILE="ISSUES.md"
DRY_RUN="${DRY_RUN:-0}"
SKIP_BRANCH="${SKIP_BRANCH:-0}"

# Resolve repo root from script location, regardless of where it's invoked from.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_DIR"

err() { echo "ERROR: $*" >&2; exit 1; }
info() { echo "[start-auto-loop] $*"; }

# ---- Preflight ----------------------------------------------------------------
command -v git >/dev/null || err "git not on PATH"
command -v claude >/dev/null || err "claude CLI not on PATH"

git rev-parse --is-inside-work-tree >/dev/null 2>&1 || err "not inside a git repo: $REPO_DIR"

if [[ -n "$(git status --porcelain)" ]]; then
  echo "Working tree not clean. Commit or stash first." >&2
  git status --short >&2
  exit 1
fi

# ---- Branch setup -------------------------------------------------------------
if [[ "$SKIP_BRANCH" != "1" ]]; then
  CURRENT_BRANCH="$(git rev-parse --abbrev-ref HEAD)"
  if [[ "$CURRENT_BRANCH" != "$BRANCH" ]]; then
    if git show-ref --verify --quiet "refs/heads/$BRANCH"; then
      info "checking out existing branch $BRANCH"
      git checkout "$BRANCH"
    else
      info "creating branch $BRANCH from $CURRENT_BRANCH"
      git checkout -b "$BRANCH"
    fi
  else
    info "already on $BRANCH"
  fi
fi

# ---- Seed ISSUES.md if missing ------------------------------------------------
if [[ ! -f "$ISSUES_FILE" ]]; then
  info "seeding $ISSUES_FILE"
  cat > "$ISSUES_FILE" <<'ISSUES_EOF'
# ISSUES

Auto-improve queue for the autonomous loop. Schema lives in
`.claude/auto-improve-loop.prompt.md`. Edit by hand to seed/reprioritize items;
the loop will respect priority, type, and status fields.

## Open

_(empty)_

## Resolved

_(empty)_
ISSUES_EOF
  git add "$ISSUES_FILE"
  git commit -m "chore: seed ISSUES.md for auto-improve loop"
fi

# ---- Write the loop prompt (overwrite each launch so edits to this script propagate) ----
mkdir -p .claude
cat > "$PROMPT_FILE" <<'PROMPT_EOF'
Autonomous improvement loop for D:\Dev\Actio.

SETUP (every tick, fast):
- `git fetch && git status`. If dirty from an abandoned prior run, run `git restore .` and `git clean -fd` ONLY inside `auto/improvements`. Never touch tracked files on `main`.
- Ensure branch `auto/improvements` exists and is checked out (create from main if missing).
- Ensure ISSUES.md exists at repo root; create with empty "## Open" / "## Resolved" sections if not.

Then choose ONE phase:

==================================================
PHASE A — DISCOVER  (run if Open section has < 8 items)
==================================================
1. Rotate through discovery lanes — pick ONE this tick that wasn't the last one logged:

   QUALITY lanes (find bugs / smells / gaps):
   - backend clippy: `cd backend && cargo clippy --all-targets -- -D warnings`
   - backend tests:  `cd backend && cargo test -p actio-core --lib`
   - frontend types: `cd frontend && pnpm tsc --noEmit`
   - frontend tests: `cd frontend && pnpm test`
   - TODO/FIXME/XXX/HACK grep across the repo
   - i18n parity check (`frontend/src/i18n/__tests__/parity.test.ts` + manual scan of en.ts vs zh-CN.ts)
   - dead-code / unused-export sweep on a single subsystem
   - error-handling audit on a single subsystem (engine, api, store, components)

   PRODUCT lanes (find features / UX wins):
   - UI/UX audit: read 1–2 React components end-to-end and identify accessibility (aria, keyboard nav, focus mgmt), responsive, empty-state, error-state, or loading-state gaps
   - design-consistency: scan a feature surface (Board, People, Settings, Standby Tray) for inconsistent spacing, typography, color, or interaction patterns vs the rest of the app
   - missing-affordances: features the data model already supports but UI doesn't expose (fields in DB without an editor, endpoints without a caller)
   - workflow friction: trace a user journey (enroll a speaker, review a pending reminder, dismiss a candidate) and note dead clicks, missing confirmations, jarring transitions
   - feature gaps from CLAUDE.md / AGENTS.md hints (e.g. "the last unfinished migration step" — propose concrete next steps)
   - performance: cargo build with `--timings`, vite bundle size, or React DevTools-style "what re-renders unnecessarily" reasoning
   - docs drift: AGENTS.md / CLAUDE.md sections that no longer match code

2. Surface 1–3 concrete items. For each: locate file:line (or describe the user-facing surface), state the gap, propose direction, estimate scope (small / medium / large).

3. Append to ISSUES.md using the schema below. Commit: `chore(issues): triage <lane>`.

4. END TURN.

==================================================
PHASE B — FIX / BUILD  (run if Open section has ≥ 1 item)
==================================================
1. Pick highest-priority OPEN item (P0 > P1 > P2). Within same priority, prefer SMALL scope and BUG type so progress is visible. Mark IN-PROGRESS with timestamp.

2. Route by Type + Scope:

   BUG (Type = bug):
   - SMALL: fix directly, add regression test.
   - MEDIUM/LARGE: superpowers:systematic-debugging → superpowers:writing-plans → superpowers:executing-plans → superpowers:verification-before-completion → superpowers:finishing-a-development-branch.

   QUALITY (Type = refactor / cleanup / perf / a11y / i18n / docs):
   - SMALL: do it.
   - MEDIUM/LARGE: superpowers:writing-plans → superpowers:executing-plans → verification → finishing.

   FEATURE or UI (Type = feature / ui):
   - ALWAYS start with superpowers:brainstorming to lock requirements + design before code.
   - Then superpowers:writing-plans → superpowers:executing-plans (or subagent-driven-development if parallelizable) → verification → finishing.
   - For UI: write a story or a focused vitest first that pins the new behavior; do not declare done from a vibe-check.
   - Match existing visual language: read 1–2 sibling components first; reuse Tailwind tokens, existing class compositions, and the `settings-check` style helper where applicable. Do NOT invent a new design system.
   - Keys for any new copy land in BOTH `frontend/src/i18n/en.ts` AND `zh-CN.ts` (parity test enforces it).

3. VERIFICATION GATE — must print success before marking DONE:
   - Backend touched:  `cd backend && cargo fmt && cargo clippy -- -D warnings && cargo test -p actio-core --lib`
   - Frontend touched: `cd frontend && pnpm tsc --noEmit && pnpm test`
   - For UI items, ALSO run `cd frontend && pnpm build` to catch prod-only failures.

4. Move issue Open → Resolved with commit SHA. Conventional commit: `fix|feat|perf|refactor|style|docs|a11y(area): …`.

5. END TURN.

==================================================
ISSUES.md SCHEMA (per item)
==================================================
### [P0|P1|P2] <short title>  (id: ISS-NNN)
- Type: bug | refactor | perf | a11y | i18n | docs | feature | ui
- Status: OPEN | IN-PROGRESS | NEEDS-REVIEW | BLOCKED | DONE (sha: …)
- Area: backend/<path> | frontend/<path> | cross-cutting
- Scope: small | medium | large
- Found: YYYY-MM-DD HH:MM
- Symptom (bug/quality) OR User value (feature/ui): …
- Hypothesis (bug) OR Proposed direction (feature/ui): …
- Acceptance: concrete, testable definition of done
- Notes: file:line refs, screenshots-needed, prior attempts

==================================================
HARD RULES (never violate)
==================================================
- Stay on branch `auto/improvements`. NEVER push, force-push, rebase, merge to main, or touch CI.
- NEVER modify settings.json, .env, credentials, secrets, or delete history from ISSUES.md.
- NEVER skip hooks (`--no-verify`) or silence clippy/TS errors with allow/any to make CI pass.
- For FEATURE/UI items: brainstorming pass is mandatory before any code. Skipping it = revert.
- If a fix would touch > 10 files OR change a public API OR introduce a new dependency: write the plan to `.omc/plans/ISS-NNN.md`, mark issue NEEDS-REVIEW, end turn.
- If tests fail and root cause is unclear after 2 systematic-debugging passes: `git restore .`, mark issue BLOCKED with notes, end turn.
- One tick = one phase. Don't chain Discover → Fix in the same turn.
- Verification gate is the ONLY definition of "done" — no "looks good to me" claims.
- If Anthropic usage-limit error occurs mid-tick, end immediately; the next cron fire will retry.
- Don't add a brand-new top-level dependency without a NEEDS-REVIEW pause; it's a directional decision the user should weigh.
PROMPT_EOF

# Commit prompt file changes if any.
if [[ -n "$(git status --porcelain "$PROMPT_FILE")" ]]; then
  git add "$PROMPT_FILE"
  git commit -m "chore: update auto-improve loop prompt"
fi

# ---- Launch -------------------------------------------------------------------
PROMPT_BODY="$(cat "$PROMPT_FILE")"
LOOP_INPUT="/loop $INTERVAL $PROMPT_BODY"

info "branch:   $(git rev-parse --abbrev-ref HEAD)"
info "interval: $INTERVAL"
info "prompt:   $PROMPT_FILE  ($(wc -c < "$PROMPT_FILE") bytes)"
info "issues:   $ISSUES_FILE"
echo

if [[ "$DRY_RUN" == "1" ]]; then
  info "DRY_RUN=1 — not launching. Command would be:"
  echo
  echo "  claude --dangerously-skip-permissions \"\$(cat $PROMPT_FILE | sed 's/^/    /')\""
  echo
  info "To launch for real: scripts/start-auto-loop.sh $INTERVAL"
  exit 0
fi

info "launching claude... (Ctrl-C to abort before /loop is dispatched)"
info "stop later with: /oh-my-claudecode:cancel  inside the session"
echo
exec claude --dangerously-skip-permissions "$LOOP_INPUT"

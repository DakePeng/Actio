# =============================================================================
# start-auto-loop.ps1 - autonomous improvement loop launcher for D:\Dev\Actio
# =============================================================================
#
# Native PowerShell version of scripts/start-auto-loop.sh. Same behavior, no
# Git Bash dependency. Targets Windows PowerShell 5.1+ and PowerShell 7+.
#
# WHAT THIS DOES
#   Boots Claude Code into a recurring `/loop` that, on each tick, either:
#     (A) DISCOVERS issues/improvements/feature ideas and appends them to
#         ISSUES.md, or
#     (B) PICKS UP an open ISSUES.md item and FIXES/BUILDS it, gated by tests
#         and clippy/tsc. Larger items go through the superpowers chain
#         (brainstorming -> systematic-debugging -> writing-plans ->
#          executing-plans -> verification -> finishing-a-development-branch).
#   All work happens on a sandbox branch (`auto/improvements`); the loop is
#   forbidden from pushing, merging to main, or touching CI.
#
# QUICK START
#   .\scripts\start-auto-loop.ps1                       # 1h interval (overnight)
#   .\scripts\start-auto-loop.ps1 -Interval 30m         # tighter cadence
#   .\scripts\start-auto-loop.ps1 -Interval 2h          # conservative
#   .\scripts\start-auto-loop.ps1 -Help                 # print this header
#
# OPTIONS
#   -Interval <str>   Cron-style interval the /loop accepts: 30m, 1h, 90m, ...
#                     Default: 1h. Don't go below ~10m or ticks may overlap
#                     a cold `cargo check`.
#   -DryRun           Set up branch + ISSUES.md + prompt file and print the
#                     command, but do NOT launch claude.
#   -SkipBranch       Don't switch branches; run on the current one. The
#                     loop's HARD RULES still block pushes, but you lose the
#                     sandbox boundary.
#   -Branch <name>    Override the sandbox branch name. Default:
#                     auto/improvements.
#   -Help             Print this header and exit.
#
# WHY INTERVAL-BASED SURVIVES USAGE LIMITS
#   `/loop <interval> <prompt>` is backed by an external cron entry. Each tick
#   is a fresh turn. If a tick aborts because the Anthropic usage limit was
#   hit, the cron entry is untouched -- the next scheduled fire just runs the
#   same prompt again, which is exactly what you want unattended. The dynamic
#   / self-paced form does NOT survive limits as cleanly.
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
#   From any CC session:  CronList -> CronDelete the entry.
#   Closing the terminal stops the *current* tick but the cron entry remains
#   and will fire again. Always cancel via the tools above.
#
# REVIEWING THE WORK
#   git log auto/improvements ^main --oneline
#   git diff main..auto/improvements
#   Get-Content ISSUES.md
#
# REQUIREMENTS
#   - `git` and `claude` on PATH.
#   - Clean working tree (the script refuses to start otherwise).
#
# EXECUTION POLICY
#   If you get "running scripts is disabled on this system", either:
#     PowerShell -ExecutionPolicy Bypass -File .\scripts\start-auto-loop.ps1
#   or set per-user policy once:
#     Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
#
# =============================================================================

[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [string]$Interval = '1h',

    [string]$Branch = 'auto/improvements',

    [switch]$DryRun,

    [switch]$SkipBranch,

    [switch]$Help
)

$ErrorActionPreference = 'Stop'

function Write-Info { param([string]$Msg) Write-Host "[start-auto-loop] $Msg" }
function Fail { param([string]$Msg) Write-Error $Msg; exit 1 }

# ---- --help -------------------------------------------------------------------
if ($Help) {
    $scriptPath = $MyInvocation.MyCommand.Path
    Get-Content -LiteralPath $scriptPath |
        ForEach-Object {
            if ($_ -notmatch '^\s*#') { 'STOP' }
            elseif ($_ -match '^# ?(.*)$') { $matches[1] }
            else { $_ }
        } |
        ForEach-Object -Begin { $stop = $false } -Process {
            if ($stop) { return }
            if ($_ -eq 'STOP') { $stop = $true; return }
            $_
        }
    exit 0
}

# ---- Resolve repo root --------------------------------------------------------
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoDir   = Split-Path -Parent $ScriptDir
Set-Location -LiteralPath $RepoDir

$PromptFile = '.claude/auto-improve-loop.prompt.md'
$IssuesFile = 'ISSUES.md'

# ---- Preflight ----------------------------------------------------------------
if (-not (Get-Command git -ErrorAction SilentlyContinue))    { Fail 'git not on PATH' }
if (-not (Get-Command claude -ErrorAction SilentlyContinue)) { Fail 'claude CLI not on PATH' }

git rev-parse --is-inside-work-tree *> $null
if ($LASTEXITCODE -ne 0) { Fail "not inside a git repo: $RepoDir" }

$dirty = git status --porcelain
if ($dirty) {
    Write-Host 'Working tree not clean. Commit or stash first.' -ForegroundColor Red
    git status --short
    exit 1
}

# ---- Branch setup -------------------------------------------------------------
if (-not $SkipBranch) {
    $current = (git rev-parse --abbrev-ref HEAD).Trim()
    if ($current -ne $Branch) {
        git show-ref --verify --quiet "refs/heads/$Branch"
        if ($LASTEXITCODE -eq 0) {
            Write-Info "checking out existing branch $Branch"
            git checkout $Branch
        } else {
            Write-Info "creating branch $Branch from $current"
            git checkout -b $Branch
        }
        if ($LASTEXITCODE -ne 0) { Fail "git checkout failed" }
    } else {
        Write-Info "already on $Branch"
    }
}

# ---- Seed ISSUES.md if missing ------------------------------------------------
if (-not (Test-Path -LiteralPath $IssuesFile)) {
    Write-Info "seeding $IssuesFile"
    $issuesSeed = @'
# ISSUES

Auto-improve queue for the autonomous loop. Schema lives in
`.claude/auto-improve-loop.prompt.md`. Edit by hand to seed/reprioritize items;
the loop will respect priority, type, and status fields.

## Open

_(empty)_

## Resolved

_(empty)_
'@
    Set-Content -LiteralPath $IssuesFile -Value $issuesSeed -Encoding utf8 -NoNewline
    # Add a trailing newline (Set-Content -NoNewline omits it; we want one).
    Add-Content -LiteralPath $IssuesFile -Value '' -Encoding utf8
    git add -- $IssuesFile
    git commit -m 'chore: seed ISSUES.md for auto-improve loop'
}

# ---- Write the loop prompt (overwritten every launch) -------------------------
New-Item -ItemType Directory -Force -Path '.claude' | Out-Null

$prompt = @'
Autonomous improvement loop for D:\Dev\Actio.

SETUP (every tick, fast):
- `git fetch && git status`. If dirty from an abandoned prior run, run `git restore .` and `git clean -fd` ONLY inside `auto/improvements`. Never touch tracked files on `main`.
- Ensure branch `auto/improvements` exists and is checked out (create from main if missing).
- Ensure ISSUES.md exists at repo root; create with empty "## Open" / "## Resolved" sections if not.

Then choose ONE phase:

==================================================
PHASE A - DISCOVER  (run if Open section has < 8 items)
==================================================
1. Rotate through discovery lanes - pick ONE this tick that wasn't the last one logged:

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
   - UI/UX audit: read 1-2 React components end-to-end and identify accessibility (aria, keyboard nav, focus mgmt), responsive, empty-state, error-state, or loading-state gaps
   - design-consistency: scan a feature surface (Board, People, Settings, Standby Tray) for inconsistent spacing, typography, color, or interaction patterns vs the rest of the app
   - missing-affordances: features the data model already supports but UI doesn't expose (fields in DB without an editor, endpoints without a caller)
   - workflow friction: trace a user journey (enroll a speaker, review a pending reminder, dismiss a candidate) and note dead clicks, missing confirmations, jarring transitions
   - feature gaps from CLAUDE.md / AGENTS.md hints (e.g. "the last unfinished migration step" - propose concrete next steps)
   - performance: cargo build with `--timings`, vite bundle size, or React DevTools-style "what re-renders unnecessarily" reasoning
   - docs drift: AGENTS.md / CLAUDE.md sections that no longer match code

2. Surface 1-3 concrete items. For each: locate file:line (or describe the user-facing surface), state the gap, propose direction, estimate scope (small / medium / large).

3. Append to ISSUES.md using the schema below. Commit: `chore(issues): triage <lane>`.

4. END TURN.

==================================================
PHASE B - FIX / BUILD  (run if Open section has >= 1 item)
==================================================
1. Pick highest-priority OPEN item (P0 > P1 > P2). Within same priority, prefer SMALL scope and BUG type so progress is visible. Mark IN-PROGRESS with timestamp.

2. Route by Type + Scope:

   BUG (Type = bug):
   - SMALL: fix directly, add regression test.
   - MEDIUM/LARGE: superpowers:systematic-debugging -> superpowers:writing-plans -> superpowers:executing-plans -> superpowers:verification-before-completion -> superpowers:finishing-a-development-branch.

   QUALITY (Type = refactor / cleanup / perf / a11y / i18n / docs):
   - SMALL: do it.
   - MEDIUM/LARGE: superpowers:writing-plans -> superpowers:executing-plans -> verification -> finishing.

   FEATURE or UI (Type = feature / ui):
   - ALWAYS start with superpowers:brainstorming to lock requirements + design before code.
   - Then superpowers:writing-plans -> superpowers:executing-plans (or subagent-driven-development if parallelizable) -> verification -> finishing.
   - For UI: write a story or a focused vitest first that pins the new behavior; do not declare done from a vibe-check.
   - Match existing visual language: read 1-2 sibling components first; reuse Tailwind tokens, existing class compositions, and the `settings-check` style helper where applicable. Do NOT invent a new design system.
   - Keys for any new copy land in BOTH `frontend/src/i18n/en.ts` AND `zh-CN.ts` (parity test enforces it).

3. VERIFICATION GATE - must print success before marking DONE:
   - Backend touched:  `cd backend && cargo fmt && cargo clippy -- -D warnings && cargo test -p actio-core --lib`
   - Frontend touched: `cd frontend && pnpm tsc --noEmit && pnpm test`
   - For UI items, ALSO run `cd frontend && pnpm build` to catch prod-only failures.

4. Move issue Open -> Resolved with commit SHA. Conventional commit: `fix|feat|perf|refactor|style|docs|a11y(area): ...`.

5. END TURN.

==================================================
ISSUES.md SCHEMA (per item)
==================================================
### [P0|P1|P2] <short title>  (id: ISS-NNN)
- Type: bug | refactor | perf | a11y | i18n | docs | feature | ui
- Status: OPEN | IN-PROGRESS | NEEDS-REVIEW | BLOCKED | DONE (sha: ...)
- Area: backend/<path> | frontend/<path> | cross-cutting
- Scope: small | medium | large
- Found: YYYY-MM-DD HH:MM
- Symptom (bug/quality) OR User value (feature/ui): ...
- Hypothesis (bug) OR Proposed direction (feature/ui): ...
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
- One tick = one phase. Don't chain Discover -> Fix in the same turn.
- Verification gate is the ONLY definition of "done" - no "looks good to me" claims.
- If Anthropic usage-limit error occurs mid-tick, end immediately; the next cron fire will retry.
- Don't add a brand-new top-level dependency without a NEEDS-REVIEW pause; it's a directional decision the user should weigh.
'@

Set-Content -LiteralPath $PromptFile -Value $prompt -Encoding utf8

# Commit prompt file changes if any.
$promptStatus = git status --porcelain -- $PromptFile
if ($promptStatus) {
    git add -- $PromptFile
    git commit -m 'chore: update auto-improve loop prompt'
}

# ---- Launch -------------------------------------------------------------------
# Keep the /loop payload short. Passing the whole prompt as one CLI argument is
# fragile on Windows and can also hit slash-command input limits in Claude Code.
# Each scheduled tick reads the full prompt from disk instead.
$PromptPathForClaude = (Resolve-Path -LiteralPath $PromptFile).Path
$loopInstruction = "Read the full recurring-loop instructions from `"$PromptPathForClaude`" and follow that file exactly for this tick. Treat that file as authoritative; do not continue from this short dispatch text alone."
$loopInput  = "/loop $Interval $loopInstruction"
$promptBytes = (Get-Item -LiteralPath $PromptFile).Length
$dispatchChars = $loopInput.Length

Write-Info ("branch:   " + (git rev-parse --abbrev-ref HEAD).Trim())
Write-Info "interval: $Interval"
Write-Info "prompt:   $PromptFile  ($promptBytes bytes)"
Write-Info "dispatch: $dispatchChars chars"
Write-Info "issues:   $IssuesFile"
Write-Host ''

if ($DryRun) {
    Write-Info '-DryRun set -- not launching. Command would be:'
    Write-Host ''
    Write-Host "  claude --dangerously-skip-permissions `"$loopInput`""
    Write-Host ''
    Write-Info "To launch for real: .\scripts\start-auto-loop.ps1 -Interval $Interval"
    exit 0
}

Write-Info 'launching claude... (Ctrl-C to abort before /loop is dispatched)'
Write-Info 'stop later with: /oh-my-claudecode:cancel  inside the session'
Write-Host ''

& claude --dangerously-skip-permissions $loopInput
exit $LASTEXITCODE

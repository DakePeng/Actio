# Me-Centered Action-Item Extraction — Design

**Date:** 2026-04-26
**Status:** Draft (spec)
**Touches:** `2026-04-15-llm-todo-extraction-design.md`, `2026-04-25-batch-clip-processing-design.md` — refines the extraction prompt and gating used by both the legacy windowed extractor and the clip-driven extractor.

## Motivation

The always-on extractor produces reminders that are (a) often meaningless and (b) frequently not the device-owner's. The root cause is that the LLM has no notion of *whose* action items it is extracting. `WINDOW_SYSTEM_PROMPT` (`backend/actio-core/src/engine/llm_prompt.rs:51`) says only *"You are listening to a rolling window of conversation and extracting only the CERTAIN action items."* When Alice says "remind me to email Bob tomorrow," the model creates a reminder — and the only inbox the system has is the device-owner's board.

Two separate problems sit underneath that:

1. **Perspective.** The system has no concept of "this is me." `speakers.kind` distinguishes `enrolled | provisional` but does not flag the device-owner. The LLM therefore cannot tell when an item belongs to *the user* versus to someone else in the room.
2. **Quality.** The current confidence rule (`high → open`, `medium → pending`, `low → drop`) gates on the LLM's self-reported certainty about *its own extraction*, not about whether the underlying utterance was a real, concrete commitment. Idle muttering ("I should look into that sometime") slips through as `medium` or `high` because the model is biased toward extracting.

This design adds a small per-tenant identity profile, a "this is me" flag on speakers, and a stricter prompt that requires both *ownership* and *concreteness* before an item is returned.

## Goals

- The extraction prompt knows the user's name, aliases (multilingual), and a short user-written bio, and uses them to filter for items that belong to the user.
- A single speaker per tenant can be marked as the "self" speaker; the prompt tells the LLM that bracketed speaker tag is the user's voice.
- The LLM applies an explicit two-leg gate (ownership + concreteness) before returning an item.
- "Me-centered, but include things I should know" — items that *another* speaker promises *to* the user (e.g. "I'll send you the API spec by Friday") still land on the board.
- First-run users with no profile set keep the current behavior (legacy prompt) — the change is opt-in by virtue of filling out the profile.

## Non-goals

- No backfill: existing reminders on the board are left alone. Re-running the LLM over historical transcripts is expensive and the user can dismiss noise manually.
- No structured projects/contacts/role fields. The bio is a single free-form paragraph; richer structure can be added later if a need emerges.
- No "shared family board" mode where reminders are tagged per-speaker. The design is single-user-centered; multi-user routing is future work.
- No new model class or LLM provider. The same `LlmRouter` paths are used.
- No replacement for the existing confidence routing (`high → open`, `medium → pending`). The new gate runs *inside* the LLM via prompt rules; the backend's confidence handling is unchanged.

## Locked decisions (from brainstorming)

| Area | Decision |
|---|---|
| Ownership rule | "Me-centered, but include things I should know." Items belong to the user when the user commits, the user is asked or addressed by name/alias, OR another speaker promises a deliverable to the user. |
| Profile scope | Light: `display_name` + `aliases` (list of strings, multilingual) + free-form `bio` paragraph. No structured role/projects/contacts fields. |
| Noise floor | Explicit deliverable gate written into the prompt: an item must satisfy a verb-object pair AND at least one of (explicit time, named recipient, urgency keyword). Layered with the user-written bio for subjective filtering. |
| Profile location | New `tenant_profile` DB table, one row per `tenant_id`. Not in `settings.toml`. |
| "Self" designation | New `is_self BOOLEAN` column on `speakers`, with a partial unique index ensuring at most one `is_self=1` row per tenant. |
| Backfill | None. Existing reminders are left alone. |
| Fallback | If a tenant has no profile AND no `is_self` speaker, the legacy `WINDOW_SYSTEM_PROMPT` is used unchanged. |

## Architecture

The change is contained: one new table, one new column, one new prompt template, and a new optional argument threaded through the LLM router. No changes to the audio pipeline, the diarization pipeline, the windowing scheduler, or the confidence-gating logic in `gate_action_item`.

### Data model

**New migration** (next number after `006_reminders_source_window_soft_fk.sql`):

```sql
-- tenant_profile: per-tenant identity used to ground LLM extraction.
CREATE TABLE IF NOT EXISTS tenant_profile (
    tenant_id     TEXT PRIMARY KEY,
    display_name  TEXT,
    aliases       TEXT NOT NULL DEFAULT '[]'  -- JSON array of strings
                  CHECK (json_valid(aliases) AND json_type(aliases) = 'array'),
    bio           TEXT,
    updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- is_self flag on speakers — links a voice to the device-owner.
ALTER TABLE speakers ADD COLUMN is_self INTEGER NOT NULL DEFAULT 0
    CHECK (is_self IN (0, 1));

-- At most one self speaker per tenant.
CREATE UNIQUE INDEX IF NOT EXISTS idx_speakers_one_self_per_tenant
    ON speakers(tenant_id) WHERE is_self = 1;
```

**Why a separate table, not columns on `speakers`:** the bio and aliases are tenant-level identity. They survive re-enrollment of a different voice (e.g. user re-enrolls because mic placement changed). The `is_self` flag is the link between the two — `speakers` holds the voiceprint, `tenant_profile` holds the identity.

### New repository: `repository/tenant_profile.rs`

```rust
pub struct TenantProfile {
    pub tenant_id: Uuid,
    pub display_name: Option<String>,
    pub aliases: Vec<String>,
    pub bio: Option<String>,
}

pub async fn get_for_tenant(pool: &SqlitePool, tenant_id: Uuid)
    -> sqlx::Result<Option<TenantProfile>>;

pub async fn upsert(pool: &SqlitePool, profile: &TenantProfile)
    -> sqlx::Result<()>;
```

`upsert` writes the full row (display_name, aliases JSON, bio) in one statement.

### New repository function: `speaker::mark_as_self`

Transactional: clears any existing `is_self=1` rows for the same tenant, then sets the target row's `is_self=1`. Defense-in-depth backstop for the partial unique index.

### Prompt: `engine/llm_prompt.rs`

Keep `WINDOW_SYSTEM_PROMPT` as the legacy fallback (verbatim). Add `WINDOW_SYSTEM_PROMPT_PROFILED`:

```
You are extracting action items FOR {display_name}.
They may also be addressed as: {aliases_csv}.
About them:
{bio}

Their voice is tagged in the transcript as "{display_name}".
Other speakers are other people — friends, coworkers, voices on a podcast,
LLM TTS, anyone.

Extract an item ONLY when BOTH of these are true:

(1) OWNERSHIP — the item belongs to {display_name}. Qualifies if any of:
    a. {display_name} commits ("I'll send the doc", "let me check on that")
    b. {display_name} is asked or assigned by name or by direct address
       ("Hey Dake, can you…", "@DK could you…", "你能不能…")
    c. another speaker promises a deliverable TO {display_name}
       ("I'll send YOU the API spec by Friday")

(2) CONCRETENESS — at least one of:
    a. explicit time ("by Friday 3pm", "tomorrow morning", "EOD")
    b. named recipient or counterparty ("to Bob", "with the design team")
    c. urgency keyword ("ASAP", "today", "now", "before the demo")

If unsure who owns an item, drop it. If it's vague aspiration ("I should
look into that someday", "we ought to"), drop it.

confidence:
  high   — both legs unambiguous
  medium — both legs satisfied but phrasing leaves real doubt
  (no "low" — omit instead)

evidence_quote MUST be a verbatim substring. speaker_name copied from the
input bracket tag. Same JSON output schema as before:
{"items":[{"title":"...","description":"...","priority":"high|medium|low",
"due_time":"YYYY-MM-DDTHH:MM","labels":["..."],
"confidence":"high|medium","evidence_quote":"...","speaker_name":"..."}]}
```

`build_window_messages` gains an `Option<&TenantProfile>` argument. When `Some`, it interpolates the profiled template; when `None`, it returns the existing legacy template byte-for-byte. An empty/whitespace bio simply omits the "About them:\n{bio}" block (no dangling label).

### LLM router & remote client

`LlmRouter::generate_action_items_with_refs` and the `RemoteLlmClient` call site gain `profile: Option<&TenantProfile>` as a new trailing argument. The `Stub` variant (used in unit/integration tests) ignores it. The `Disabled` variant continues to short-circuit before any prompt construction.

### Pipeline changes — `engine/window_extractor.rs`

Both `process_window_with` and `extract_for_clip` add a single profile lookup right after the session/tenant lookup:

```rust
let profile = tenant_profile_repo::get_for_tenant(pool, session.tenant_id).await?;
// …
router.generate_action_items_with_refs(
    &attributed,
    &label_names,
    &window_local_date,
    profile.as_ref(),
).await
```

No changes to `gate_action_item`, no changes to the windowing scheduler, no changes to clip persistence. The "self" speaker plays its role purely in the prompt — `attributed` lines already carry the speaker's `display_name`, so the LLM sees `[HH:MM:SS • Dake Peng]: ...` exactly as before, and the prompt tells it that bracketed name is the user's voice.

### API surface

**New endpoints (`backend/actio-core/src/api/profile.rs`):**

- `GET /profile` — returns the tenant's profile (or `404` if not set yet).
- `PUT /profile` — body: `{ display_name?, aliases?, bio? }`; upserts.

**New endpoint (`backend/actio-core/src/api/speakers.rs`):**

- `POST /speakers/:id/mark-self` — flips `is_self=1` for the target speaker, clears the prior self-speaker for the tenant. Returns the updated row.

OpenAPI annotations follow the existing utoipa patterns in `api/`.

### Frontend changes

- **Settings → About me panel.** Three controls: name input, aliases chip-input (add/remove with case-insensitive dedupe per script), bio textarea. Save button calls `PUT /profile`. New i18n keys land in both `en.ts` and `zh-CN.ts` (parity test enforces).
- **People tab speaker card.** A "**This is me**" toggle on every speaker card, mutually exclusive across the tab. On flip, calls `POST /speakers/:id/mark-self` and re-fetches the speaker list.
- **Optional gentle nudge.** When `is_self` is set on a speaker whose `display_name` differs from the tenant's `tenant_profile.display_name`, the People-tab card surfaces a one-click "Sync name to profile" action. Cosmetic only — the LLM uses `tenant_profile.display_name`, not the speaker's display name, for prompt interpolation.

### Aliases & multilingual matching

Aliases are stored as a flat JSON array of strings — no language tagging. The LLM handles cross-script matching natively (a user named `Dake Peng` with aliases `["Dake", "DK", "彭大可"]` will be matched whether the transcript line is `"Hey Dake"`, `"DK 你看一下"`, or `"彭大可，麻烦你"`). The frontend chip input deduplicates case-insensitively per-script (so `"Dake"` and `"dake"` collapse, but `"DK"` and `"彭大可"` coexist).

## Testing strategy

### Unit — `llm_prompt.rs`

- `build_window_messages(profile=None, …)` returns a byte-for-byte match against the legacy `WINDOW_SYSTEM_PROMPT` (guards against fallback drift when the profiled template evolves).
- With profile present, the rendered prompt contains: `display_name`, every alias (joined CSV), the bio paragraph, both ownership and concreteness rules.
- Empty/whitespace `bio` omits the "About them:" block.
- Aliases with non-ASCII characters (`彭大可`) survive interpolation intact.

### Unit — `repository/tenant_profile.rs`

- `upsert` round-trips aliases as JSON (insert `["Dake", "彭大可"]`, fetch, assert order preserved).
- `aliases` CHECK constraint rejects non-array JSON values (insert `'"not array"'` directly via raw SQL → expect error).
- `upsert` is idempotent — second call with same `tenant_id` updates rather than failing.

### Unit — `repository/speaker.rs`

- `mark_as_self(speaker_a)` then `mark_as_self(speaker_b)` for the same tenant: A's flag flips to 0, B's to 1.
- Raw `INSERT … is_self=1` for a tenant that already has a self-speaker fails (partial unique index defense).

### Integration — `engine/window_extractor.rs`

- Existing tests keep passing — the `Stub` router ignores the new `profile` arg.
- New test `process_window_with_profile_passes_profile_to_router`: a capture-stub variant records the `profile` it received; assert each field propagates from DB → router.
- `process_window_with_no_profile_uses_legacy_prompt`: assert the request payload matches the legacy template (achieved by capturing the rendered messages).

### Integration — full extraction with stubbed LLM responses

These tests stub LLM responses to verify the *backend's* persistence and gating; they do not exercise the LLM-side rules (those live in the prompt and would need an evaluation harness, see Manual section below).

- Owner = me + concrete deadline → `open`.
- Owner = me + vague phrasing (LLM returns `confidence='low'` for vague items per the new rules) → dropped by the existing gate.
- Owner = other speaker, no addressing me, LLM correctly returns `[]` → no rows inserted.
- Other speaker promises deliverable TO me, LLM returns one `high` item → `open`.

### Frontend — `vitest`

- `AboutMeSection` renders existing values, edits propagate through the API client (mocked).
- Aliases chip input adds, removes, deduplicates case-insensitively per script (`"Dake"` + `"dake"` → one chip; `"DK"` + `"彭大可"` → two chips).
- People tab: toggling "This is me" on speaker B unsets it on speaker A in the same render cycle (calls `mark-self`, re-fetches list).
- `i18n/__tests__/parity.test.ts` automatically catches missing zh-CN keys.

### Manual / dogfood (not automated)

- Set up the profile, run for one full workday, count board items, eyeball precision.
- Compare against a recorded baseline session: same audio file → old prompt vs new prompt; manually score precision/recall on a 50-item sample.
- Sanity-check the multilingual alias path with a zh ↔ en mixed conversation.

## Migration & rollout

- Migration `007_tenant_profile_and_self_speaker.sql` adds the table, the column, and the partial unique index.
- No data migration: `tenant_profile` starts empty; `speakers.is_self` defaults to 0 for every existing row.
- The pipeline auto-falls back to the legacy prompt for any tenant with no profile, so the change is invisible to first-run users until they fill out the About-me panel.
- After release, the legacy `WINDOW_SYSTEM_PROMPT` constant stays in `llm_prompt.rs` indefinitely (it serves as the no-profile fallback). It is not deprecated.

## Open questions

None at spec time. Implementation may surface details around the chip-input UX or the precise CSS for the "This is me" toggle; those are UI questions to answer during build, not design questions.

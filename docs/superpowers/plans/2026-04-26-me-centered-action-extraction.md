# Me-Centered Action-Item Extraction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the always-on action-item extractor produce reminders that actually belong to the device-owner and pass an objective concreteness bar.

**Architecture:** Add a per-tenant identity profile (`tenant_profile`: display_name, multilingual aliases, free-form bio) plus a single `is_self` flag on `speakers`. Thread the profile through the LLM router into a new prompt template that requires both ownership and concreteness before returning an item. First-run users with no profile fall back to the legacy prompt — change is invisible until they fill out About-me.

**Tech Stack:** Rust (axum, sqlx, tokio), SQLite, React + Zustand + Tailwind, vitest. No new deps.

**Spec:** `docs/superpowers/specs/2026-04-26-me-centered-action-extraction-design.md`

---

## File Map

**Created:**
- `backend/actio-core/migrations/007_tenant_profile_and_self_speaker.sql`
- `backend/actio-core/src/repository/tenant_profile.rs`
- `backend/actio-core/src/api/profile.rs`
- `frontend/src/api/profile.ts`
- `frontend/src/components/settings/__tests__/AboutMeSection.test.tsx`

**Modified:**
- `backend/actio-core/src/repository/mod.rs` — register `tenant_profile` module
- `backend/actio-core/src/repository/speaker.rs` — add `mark_as_self`
- `backend/actio-core/src/engine/llm_prompt.rs` — add `WINDOW_SYSTEM_PROMPT_PROFILED`, extend `build_window_messages`
- `backend/actio-core/src/engine/llm_router.rs` — thread `profile` through `generate_action_items_with_refs`
- `backend/actio-core/src/engine/remote_llm_client.rs` — same threading
- `backend/actio-core/src/engine/window_extractor.rs` — fetch profile in `process_window_with` and `extract_for_clip`
- `backend/actio-core/src/api/mod.rs` — register `profile` module + new routes
- `backend/actio-core/src/api/segment.rs` (or wherever speakers live) — add `mark-self` route
- `backend/actio-core/src/domain/types.rs` — add `TenantProfile` struct
- `frontend/src/store/use-store.ts` — extend profile slice with `aliases`, `bio`, server sync
- `frontend/src/components/settings/ProfileSection.tsx` → renamed/extended to `AboutMeSection.tsx` (or kept as-is and extended)
- `frontend/src/i18n/en.ts`, `frontend/src/i18n/zh-CN.ts` — new keys
- `frontend/src/components/voice/PeopleTab.tsx` (or wherever speaker cards render) — "This is me" toggle

---

## Pre-flight (assumed environment)

- `cd backend && cargo check -p actio-core --tests` succeeds on `main` head before starting.
- `cd frontend && pnpm install && pnpm test` succeeds on `main` head before starting.
- Working tree clean; on a feature branch, e.g. `feat/me-centered-extraction`.

---

### Task 1: Database migration

**Files:**
- Create: `backend/actio-core/migrations/007_tenant_profile_and_self_speaker.sql`

- [ ] **Step 1: Write the migration**

```sql
-- 007_tenant_profile_and_self_speaker.sql
-- Per-tenant identity (display name, multilingual aliases, free-form bio)
-- and a self-speaker flag used to ground the action-item extraction prompt.

CREATE TABLE IF NOT EXISTS tenant_profile (
    tenant_id     TEXT PRIMARY KEY,
    display_name  TEXT,
    aliases       TEXT NOT NULL DEFAULT '[]'
                  CHECK (json_valid(aliases) AND json_type(aliases) = 'array'),
    bio           TEXT,
    updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

ALTER TABLE speakers ADD COLUMN is_self INTEGER NOT NULL DEFAULT 0
    CHECK (is_self IN (0, 1));

-- Partial unique index: at most one is_self=1 row per tenant.
CREATE UNIQUE INDEX IF NOT EXISTS idx_speakers_one_self_per_tenant
    ON speakers(tenant_id) WHERE is_self = 1;
```

- [ ] **Step 2: Verify it loads**

Run: `cd backend && cargo test -p actio-core --lib repository::db::run_migrations -- --nocapture`

(If no such test exists, run any test that opens an in-memory DB — e.g. `cargo test -p actio-core --lib schedule_windows_emits_rows_once_per_step`. The shared `fresh_pool()` helper applies all migrations, so any passing test confirms the new SQL parses.)

Expected: PASS (migration applies cleanly to an empty DB).

- [ ] **Step 3: Commit**

```bash
git add backend/actio-core/migrations/007_tenant_profile_and_self_speaker.sql
git commit -m "feat(db): add tenant_profile table and speakers.is_self column"
```

---

### Task 2: TenantProfile struct in domain types

**Files:**
- Modify: `backend/actio-core/src/domain/types.rs`

- [ ] **Step 1: Add the struct**

Append to `domain/types.rs` (preserve alphabetical/grouping conventions of the file):

```rust
/// Per-tenant identity used to ground the action-item extraction prompt.
/// Populated via `PUT /profile`. Stored in the `tenant_profile` table.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct TenantProfile {
    pub tenant_id: uuid::Uuid,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub bio: Option<String>,
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cd backend && cargo check -p actio-core`

Expected: PASS, no warnings related to `TenantProfile`.

- [ ] **Step 3: Commit**

```bash
git add backend/actio-core/src/domain/types.rs
git commit -m "feat(types): add TenantProfile domain type"
```

---

### Task 3: tenant_profile repository — round-trip test

**Files:**
- Create: `backend/actio-core/src/repository/tenant_profile.rs`
- Modify: `backend/actio-core/src/repository/mod.rs`

- [ ] **Step 1: Register the module**

Add to `backend/actio-core/src/repository/mod.rs` (alphabetical with the others):

```rust
pub mod tenant_profile;
```

- [ ] **Step 2: Create the repo file with a failing test**

Create `backend/actio-core/src/repository/tenant_profile.rs`:

```rust
//! tenant_profile repository: per-tenant identity (display_name, aliases, bio).
//!
//! Aliases are stored as a JSON array in a TEXT column; the table CHECK
//! ensures shape. The repo handles JSON ↔ Vec<String> conversion.

use sqlx::SqlitePool;
use uuid::Uuid;

use crate::domain::types::TenantProfile;

pub async fn get_for_tenant(
    pool: &SqlitePool,
    tenant_id: Uuid,
) -> sqlx::Result<Option<TenantProfile>> {
    let row: Option<(String, Option<String>, String, Option<String>)> = sqlx::query_as(
        "SELECT tenant_id, display_name, aliases, bio FROM tenant_profile WHERE tenant_id = ?1",
    )
    .bind(tenant_id.to_string())
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(tid, name, aliases_json, bio)| TenantProfile {
        tenant_id: Uuid::parse_str(&tid).unwrap_or(tenant_id),
        display_name: name,
        aliases: serde_json::from_str(&aliases_json).unwrap_or_default(),
        bio,
    }))
}

pub async fn upsert(pool: &SqlitePool, profile: &TenantProfile) -> sqlx::Result<()> {
    let aliases_json =
        serde_json::to_string(&profile.aliases).unwrap_or_else(|_| "[]".to_string());
    sqlx::query(
        r#"INSERT INTO tenant_profile (tenant_id, display_name, aliases, bio, updated_at)
           VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
           ON CONFLICT(tenant_id) DO UPDATE SET
             display_name = excluded.display_name,
             aliases      = excluded.aliases,
             bio          = excluded.bio,
             updated_at   = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')"#,
    )
    .bind(profile.tenant_id.to_string())
    .bind(&profile.display_name)
    .bind(aliases_json)
    .bind(&profile.bio)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::db::run_migrations;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn fresh_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn upsert_then_get_round_trips_unicode_aliases() {
        let pool = fresh_pool().await;
        let tenant_id = Uuid::new_v4();
        let profile = TenantProfile {
            tenant_id,
            display_name: Some("Dake Peng".into()),
            aliases: vec!["Dake".into(), "DK".into(), "彭大可".into()],
            bio: Some("Solo dev building Actio.".into()),
        };

        upsert(&pool, &profile).await.unwrap();

        let got = get_for_tenant(&pool, tenant_id).await.unwrap().unwrap();
        assert_eq!(got.display_name.as_deref(), Some("Dake Peng"));
        assert_eq!(got.aliases, vec!["Dake", "DK", "彭大可"]);
        assert_eq!(got.bio.as_deref(), Some("Solo dev building Actio."));
    }
}
```

- [ ] **Step 3: Run test — should pass on first try (TDD-as-spec — repo + test in one task)**

Run: `cd backend && cargo test -p actio-core --lib tenant_profile::tests::upsert_then_get_round_trips_unicode_aliases -- --nocapture`

Expected: PASS.

- [ ] **Step 4: Add idempotency test (failing → fix not needed, just verifies)**

Append to the `tests` module:

```rust
#[tokio::test]
async fn upsert_is_idempotent_and_overwrites() {
    let pool = fresh_pool().await;
    let tenant_id = Uuid::new_v4();
    let p1 = TenantProfile {
        tenant_id,
        display_name: Some("Old".into()),
        aliases: vec!["a".into()],
        bio: None,
    };
    let p2 = TenantProfile {
        tenant_id,
        display_name: Some("New".into()),
        aliases: vec!["b".into(), "c".into()],
        bio: Some("now with bio".into()),
    };
    upsert(&pool, &p1).await.unwrap();
    upsert(&pool, &p2).await.unwrap();

    let got = get_for_tenant(&pool, tenant_id).await.unwrap().unwrap();
    assert_eq!(got.display_name.as_deref(), Some("New"));
    assert_eq!(got.aliases, vec!["b", "c"]);
    assert_eq!(got.bio.as_deref(), Some("now with bio"));

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tenant_profile")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(count.0, 1, "upsert must not insert duplicate rows");
}
```

- [ ] **Step 5: Run all tenant_profile tests**

Run: `cd backend && cargo test -p actio-core --lib tenant_profile -- --nocapture`

Expected: 2 passed, 0 failed.

- [ ] **Step 6: Add CHECK-constraint defense test**

Append:

```rust
#[tokio::test]
async fn aliases_check_rejects_non_array_json() {
    let pool = fresh_pool().await;
    let tenant_id = Uuid::new_v4();
    let result = sqlx::query(
        "INSERT INTO tenant_profile (tenant_id, aliases) VALUES (?1, ?2)",
    )
    .bind(tenant_id.to_string())
    .bind(r#""not an array""#)
    .execute(&pool)
    .await;
    assert!(result.is_err(), "expected CHECK constraint violation");
}
```

- [ ] **Step 7: Run all tenant_profile tests again**

Run: `cd backend && cargo test -p actio-core --lib tenant_profile -- --nocapture`

Expected: 3 passed, 0 failed.

- [ ] **Step 8: Commit**

```bash
git add backend/actio-core/src/repository/tenant_profile.rs backend/actio-core/src/repository/mod.rs
git commit -m "feat(repo): add tenant_profile repository (get_for_tenant, upsert)"
```

---

### Task 4: speaker::mark_as_self — transactional self-flag

**Files:**
- Modify: `backend/actio-core/src/repository/speaker.rs`

- [ ] **Step 1: Add the failing test**

Append to the existing `tests` module in `speaker.rs` (use that file's existing `fresh_pool` / `mk_speaker` helpers — if they don't exist there, mirror the helpers from `repository/tenant_profile.rs` Task 3):

```rust
#[tokio::test]
async fn mark_as_self_clears_prior_self_for_same_tenant() {
    use crate::repository::db::run_migrations;
    use sqlx::sqlite::SqlitePoolOptions;
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:").await.unwrap();
    sqlx::query("PRAGMA foreign_keys = ON").execute(&pool).await.unwrap();
    run_migrations(&pool).await.unwrap();

    let tenant_id = Uuid::new_v4();
    let make = |name: &str| {
        let n = name.to_string();
        let p = pool.clone();
        let tid = tenant_id;
        async move {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO speakers (id, tenant_id, display_name) VALUES (?1, ?2, ?3)",
            )
            .bind(id.to_string()).bind(tid.to_string()).bind(&n)
            .execute(&p).await.unwrap();
            id
        }
    };
    let alice = make("Alice").await;
    let bob = make("Bob").await;

    super::mark_as_self(&pool, alice).await.unwrap();
    super::mark_as_self(&pool, bob).await.unwrap();

    let alice_flag: (i64,) = sqlx::query_as(
        "SELECT is_self FROM speakers WHERE id = ?1"
    ).bind(alice.to_string()).fetch_one(&pool).await.unwrap();
    let bob_flag: (i64,) = sqlx::query_as(
        "SELECT is_self FROM speakers WHERE id = ?1"
    ).bind(bob.to_string()).fetch_one(&pool).await.unwrap();
    assert_eq!(alice_flag.0, 0);
    assert_eq!(bob_flag.0, 1);
}
```

- [ ] **Step 2: Run — should fail (no `mark_as_self` yet)**

Run: `cd backend && cargo test -p actio-core --lib speaker::tests::mark_as_self_clears_prior_self_for_same_tenant`

Expected: FAIL — `mark_as_self` is not defined.

- [ ] **Step 3: Implement `mark_as_self`**

Add to `repository/speaker.rs` (above the `#[cfg(test)]` block):

```rust
/// Atomically flip `is_self=1` on the target speaker, clearing any prior
/// self-flag for the same tenant. Defense in depth alongside the partial
/// unique index `idx_speakers_one_self_per_tenant`.
pub async fn mark_as_self(pool: &SqlitePool, speaker_id: Uuid) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;

    // Look up tenant_id of the target speaker.
    let row: (String,) = sqlx::query_as(
        "SELECT tenant_id FROM speakers WHERE id = ?1",
    )
    .bind(speaker_id.to_string())
    .fetch_one(&mut *tx)
    .await?;
    let tenant_id = row.0;

    // Clear any prior self-flag for this tenant.
    sqlx::query(
        "UPDATE speakers SET is_self = 0 WHERE tenant_id = ?1 AND is_self = 1",
    )
    .bind(&tenant_id)
    .execute(&mut *tx)
    .await?;

    // Set the target speaker's flag.
    sqlx::query("UPDATE speakers SET is_self = 1 WHERE id = ?1")
        .bind(speaker_id.to_string())
        .execute(&mut *tx)
        .await?;

    tx.commit().await
}
```

- [ ] **Step 4: Run — should pass**

Run: `cd backend && cargo test -p actio-core --lib speaker::tests::mark_as_self_clears_prior_self_for_same_tenant`

Expected: PASS.

- [ ] **Step 5: Add raw-INSERT defense-in-depth test**

Append:

```rust
#[tokio::test]
async fn partial_unique_index_blocks_two_self_speakers_per_tenant() {
    use crate::repository::db::run_migrations;
    use sqlx::sqlite::SqlitePoolOptions;
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:").await.unwrap();
    run_migrations(&pool).await.unwrap();

    let tenant_id = Uuid::new_v4();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO speakers (id, tenant_id, display_name, is_self) VALUES (?1, ?2, 'A', 1)",
    ).bind(a.to_string()).bind(tenant_id.to_string())
    .execute(&pool).await.unwrap();

    let result = sqlx::query(
        "INSERT INTO speakers (id, tenant_id, display_name, is_self) VALUES (?1, ?2, 'B', 1)",
    ).bind(b.to_string()).bind(tenant_id.to_string())
    .execute(&pool).await;
    assert!(result.is_err(), "expected unique index violation");
}
```

- [ ] **Step 6: Run all speaker tests**

Run: `cd backend && cargo test -p actio-core --lib speaker::`

Expected: all pre-existing + 2 new pass.

- [ ] **Step 7: Commit**

```bash
git add backend/actio-core/src/repository/speaker.rs
git commit -m "feat(repo): add speaker::mark_as_self with transactional clear-and-set"
```

---

### Task 5: Profiled prompt template + builder fallback

**Files:**
- Modify: `backend/actio-core/src/engine/llm_prompt.rs`

- [ ] **Step 1: Add the failing tests first**

Append to the `tests` module in `llm_prompt.rs`:

```rust
use crate::domain::types::TenantProfile;
use uuid::Uuid;

fn fixture_profile() -> TenantProfile {
    TenantProfile {
        tenant_id: Uuid::new_v4(),
        display_name: Some("Dake Peng".into()),
        aliases: vec!["Dake".into(), "DK".into(), "彭大可".into()],
        bio: Some("Solo dev building Actio.".into()),
    }
}

#[test]
fn build_window_messages_no_profile_matches_legacy_byte_for_byte() {
    let labels = vec!["Work".into()];
    let with_none = build_window_messages("hi", &labels, "2026-04-26 Sunday", None);
    // Compare against the same call into the (currently unchanged) legacy path
    // by calling the same function with profile=None — the assertion is that
    // the rendered system content does NOT contain any profiled-prompt markers.
    let sys = &with_none[0].content;
    assert!(sys.contains(WINDOW_SYSTEM_PROMPT));
    assert!(!sys.contains("Extracting action items FOR"));
    assert!(!sys.contains("They may also be addressed as"));
}

#[test]
fn build_window_messages_with_profile_includes_all_fields() {
    let labels: Vec<String> = vec![];
    let profile = fixture_profile();
    let msgs = build_window_messages("hello", &labels, "2026-04-26 Sunday", Some(&profile));
    let sys = &msgs[0].content;
    assert!(sys.contains("Dake Peng"), "missing display_name");
    assert!(sys.contains("Dake"), "missing alias 1");
    assert!(sys.contains("DK"), "missing alias 2");
    assert!(sys.contains("彭大可"), "missing CJK alias");
    assert!(sys.contains("Solo dev building Actio."), "missing bio");
    assert!(sys.contains("OWNERSHIP"), "missing ownership rule");
    assert!(sys.contains("CONCRETENESS"), "missing concreteness rule");
}

#[test]
fn build_window_messages_with_profile_omits_about_when_bio_blank() {
    let mut p = fixture_profile();
    p.bio = Some("   ".into());
    let msgs = build_window_messages("hi", &[], "2026-04-26 Sunday", Some(&p));
    let sys = &msgs[0].content;
    assert!(!sys.contains("About them:"), "should not render the About them: header for blank bio");
}

#[test]
fn build_window_messages_with_profile_omits_aliases_line_when_empty() {
    let mut p = fixture_profile();
    p.aliases.clear();
    let msgs = build_window_messages("hi", &[], "2026-04-26 Sunday", Some(&p));
    let sys = &msgs[0].content;
    assert!(!sys.contains("They may also be addressed as"), "no alias line when list empty");
}
```

- [ ] **Step 2: Run — should fail (signature mismatch)**

Run: `cd backend && cargo test -p actio-core --lib llm_prompt::tests`

Expected: FAIL — `build_window_messages` takes 3 args, not 4. Compile error.

- [ ] **Step 3: Add the new prompt constant + extend the builder**

Replace the existing `pub const WINDOW_SYSTEM_PROMPT` block and `build_window_messages` function in `llm_prompt.rs` with:

```rust
pub const WINDOW_SYSTEM_PROMPT: &str = "\
You are listening to a rolling window of conversation and extracting only the CERTAIN action items.\n\
Be conservative: most idle talk is NOT an action item. Missing items is better than inventing them.\n\
\n\
Return ONLY a raw JSON object — no markdown, no fences, no explanation:\n\
{\"items\":[{\"title\":\"...\",\"description\":\"...\",\"priority\":\"high|medium|low\",\"due_time\":\"YYYY-MM-DDTHH:MM\",\"labels\":[\"...\"],\"confidence\":\"high|medium|low\",\"evidence_quote\":\"verbatim span from input\",\"speaker_name\":\"name as printed, or null\"}]}\n\
\n\
Rules:\n\
- If nothing in this window is a real action item, return {\"items\":[]}.\n\
- confidence=\"high\": explicit commitment or ask, unambiguous. Example: \\\"Remind me to email Bob tomorrow at 9.\\\"\n\
- confidence=\"medium\": plausibly an action but phrasing is ambiguous (\\\"maybe we should …\\\", \\\"someone could …\\\"). Use sparingly.\n\
- confidence=\"low\": do NOT return these — omit them entirely.\n\
- evidence_quote MUST be a verbatim substring from the input, trimmed. If you can't pick one, the item is not real — omit it.\n\
- speaker_name is copied from the bracketed speaker tag in the input line containing the evidence_quote, or null if Unknown.\n\
- title under 60 chars, same language as input. description expands context naturally. due_time only if an explicit time reference exists in this window.\n\
- labels: pick 0–3 from the provided list. Empty array if none fit.";

/// Profiled variant — used when a `TenantProfile` is available. Adds an
/// ownership rule (item must belong to the user) and a concreteness rule
/// (verb-object plus deadline / recipient / urgency). The `{display_name}`,
/// `{aliases_csv}`, and optional `{bio_block}` placeholders are filled by
/// `build_window_messages`.
pub const WINDOW_SYSTEM_PROMPT_PROFILED_TEMPLATE: &str = "\
You are extracting action items FOR {display_name}.\n\
{aliases_line}\
{bio_block}\
\n\
Their voice is tagged in the transcript as \"{display_name}\".\n\
Other speakers are other people — friends, coworkers, voices on a podcast, LLM TTS, anyone.\n\
\n\
Extract an item ONLY when BOTH of these are true:\n\
\n\
(1) OWNERSHIP — the item belongs to {display_name}. Qualifies if any of:\n\
    a. {display_name} commits (\"I'll send the doc\", \"let me check on that\").\n\
    b. {display_name} is asked or assigned by name or by direct address (\"Hey Dake, can you…\", \"@DK could you…\", \"你能不能…\").\n\
    c. another speaker promises a deliverable TO {display_name} (\"I'll send YOU the API spec by Friday\").\n\
\n\
(2) CONCRETENESS — at least one of:\n\
    a. explicit time (\"by Friday 3pm\", \"tomorrow morning\", \"EOD\").\n\
    b. named recipient or counterparty (\"to Bob\", \"with the design team\").\n\
    c. urgency keyword (\"ASAP\", \"today\", \"now\", \"before the demo\").\n\
\n\
If unsure who owns an item, drop it. If it's vague aspiration (\"I should look into that someday\", \"we ought to\"), drop it.\n\
\n\
Return ONLY a raw JSON object — no markdown, no fences, no explanation:\n\
{\"items\":[{\"title\":\"...\",\"description\":\"...\",\"priority\":\"high|medium|low\",\"due_time\":\"YYYY-MM-DDTHH:MM\",\"labels\":[\"...\"],\"confidence\":\"high|medium\",\"evidence_quote\":\"verbatim substring from input\",\"speaker_name\":\"name as printed, or null\"}]}\n\
\n\
confidence=\"high\": both legs unambiguous.\n\
confidence=\"medium\": both legs satisfied but phrasing leaves real doubt.\n\
Do not emit \"low\" — omit the item instead.\n\
evidence_quote MUST be a verbatim substring. title under 60 chars, same language as input. labels: pick 0–3 from the provided list.";

pub fn build_window_messages(
    attributed_transcript: &str,
    label_names: &[String],
    window_local_date: &str,
    profile: Option<&crate::domain::types::TenantProfile>,
) -> Vec<ChatMessage> {
    let labels_str = if label_names.is_empty() {
        "none".to_string()
    } else {
        label_names.join(", ")
    };

    let body = match profile {
        None => WINDOW_SYSTEM_PROMPT.to_string(),
        Some(p) => render_profiled_prompt(p),
    };

    let system = format!(
        "Window date (local): {window_local_date}\nLabels: [{labels_str}]\n\n{body}"
    );
    vec![
        ChatMessage { role: "system".into(), content: system },
        ChatMessage { role: "user".into(),   content: attributed_transcript.to_string() },
    ]
}

fn render_profiled_prompt(profile: &crate::domain::types::TenantProfile) -> String {
    let display = profile.display_name.as_deref().unwrap_or("the user");
    let aliases_line = if profile.aliases.is_empty() {
        String::new()
    } else {
        format!("They may also be addressed as: {}.\n", profile.aliases.join(", "))
    };
    let bio_block = match profile.bio.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(b) => format!("About them:\n{b}\n"),
        None => String::new(),
    };
    WINDOW_SYSTEM_PROMPT_PROFILED_TEMPLATE
        .replace("{display_name}", display)
        .replace("{aliases_line}", &aliases_line)
        .replace("{bio_block}", &bio_block)
}
```

- [ ] **Step 4: Update existing call sites in this file's tests**

The pre-existing test `build_todo_messages_has_system_then_user` is unaffected. There is no other call to `build_window_messages` inside this file. (Other callers are updated in Tasks 6–8.)

- [ ] **Step 5: Run llm_prompt tests**

Run: `cd backend && cargo test -p actio-core --lib llm_prompt::`

Expected: 4 new tests pass; pre-existing tests still pass. The crate as a whole will not yet compile because Tasks 6–8 haven't updated the callers — that's fine, run only this module.

- [ ] **Step 6: Commit**

```bash
git add backend/actio-core/src/engine/llm_prompt.rs
git commit -m "feat(prompt): add WINDOW_SYSTEM_PROMPT_PROFILED with ownership+concreteness gate"
```

---

### Task 6: Thread `profile` through LlmRouter

**Files:**
- Modify: `backend/actio-core/src/engine/llm_router.rs`

- [ ] **Step 1: Find the existing signature**

Run: `cd backend && grep -n "generate_action_items_with_refs" src/engine/llm_router.rs`

Note the current signature — typically:

```rust
pub async fn generate_action_items_with_refs(
    &self,
    transcript: &str,
    label_names: &[String],
    window_local_date: &str,
) -> Result<Vec<LlmActionItem>, LlmRouterError>
```

- [ ] **Step 2: Add the `profile` parameter**

Edit the method signature on `LlmRouter` (and any matching impl on `LlmRouter::Stub`, `LlmRouter::Disabled`, `LlmRouter::Local`, `LlmRouter::Remote`):

```rust
pub async fn generate_action_items_with_refs(
    &self,
    transcript: &str,
    label_names: &[String],
    window_local_date: &str,
    profile: Option<&crate::domain::types::TenantProfile>,
) -> Result<Vec<LlmActionItem>, LlmRouterError> {
    match self {
        LlmRouter::Disabled => Err(LlmRouterError::Disabled),
        LlmRouter::Stub(items) => Ok(items.clone()),
        LlmRouter::Local { slot, model_id } => {
            // forward to local-model path with profile
            local_generate(slot, model_id, transcript, label_names, window_local_date, profile).await
        }
        LlmRouter::Remote(client) => {
            client.generate_action_items_with_refs(
                transcript, label_names, window_local_date, profile,
            ).await
        }
    }
}
```

The `Stub` variant **ignores** `profile` — preserves all existing integration tests.

- [ ] **Step 3: If a free function `local_generate` exists, give it the profile arg**

If the local-LLM path constructs messages itself, find where it calls `build_window_messages` and pass `profile` in. Otherwise add the parameter and thread it.

Run: `cd backend && grep -n "build_window_messages" src/engine/`

Update every call site to pass the new fourth argument (`profile` for `process_window`, `None` if a non-window path uses it).

- [ ] **Step 4: Verify the crate compiles**

Run: `cd backend && cargo check -p actio-core`

Expected: compile errors only at the call sites in Tasks 7–8 (window_extractor) — those will be fixed shortly. Errors inside `engine/` itself must be resolved here. If a test stub uses the old 3-arg signature, fix it.

- [ ] **Step 5: Commit**

```bash
git add backend/actio-core/src/engine/llm_router.rs
git commit -m "refactor(llm): thread profile through LlmRouter::generate_action_items_with_refs"
```

---

### Task 7: Thread `profile` through RemoteLlmClient

**Files:**
- Modify: `backend/actio-core/src/engine/remote_llm_client.rs`

- [ ] **Step 1: Find the method**

Run: `cd backend && grep -n "fn generate_action_items_with_refs" src/engine/remote_llm_client.rs`

- [ ] **Step 2: Add the `profile` parameter and pass to `build_window_messages`**

```rust
pub async fn generate_action_items_with_refs(
    &self,
    transcript: &str,
    label_names: &[String],
    window_local_date: &str,
    profile: Option<&crate::domain::types::TenantProfile>,
) -> Result<Vec<LlmActionItem>, LlmRouterError> {
    let messages = crate::engine::llm_prompt::build_window_messages(
        transcript, label_names, window_local_date, profile,
    );
    // ... rest of the existing method (HTTP POST, JSON parse) unchanged.
}
```

- [ ] **Step 3: Verify**

Run: `cd backend && cargo check -p actio-core`

Expected: compile errors limited to `window_extractor.rs` callers (next two tasks).

- [ ] **Step 4: Commit**

```bash
git add backend/actio-core/src/engine/remote_llm_client.rs
git commit -m "refactor(llm): thread profile through RemoteLlmClient"
```

---

### Task 8: Profile lookup in `process_window_with`

**Files:**
- Modify: `backend/actio-core/src/engine/window_extractor.rs`

- [ ] **Step 1: Add the failing test**

Append to the existing `tests` module in `window_extractor.rs`:

```rust
#[tokio::test]
async fn process_window_with_uses_profile_when_set() {
    use crate::domain::types::TenantProfile;

    let pool = fresh_pool().await;
    let sid = mk_session(&pool).await;
    let alice = mk_speaker(&pool, "Alice").await;
    let seg = mk_segment(&pool, sid, alice, 1_000, 30_000).await;
    mk_final_transcript_for_segment(
        &pool, sid, seg,
        "Please draft the design review summary by Thursday morning so the team can read it.",
        1_000, 30_000,
    ).await;

    // Look up the tenant for this session and seed its profile.
    let tenant_row: (String,) = sqlx::query_as(
        "SELECT tenant_id FROM audio_sessions WHERE id = ?1"
    ).bind(sid.to_string()).fetch_one(&pool).await.unwrap();
    let tenant_id = Uuid::parse_str(&tenant_row.0).unwrap();
    crate::repository::tenant_profile::upsert(&pool, &TenantProfile {
        tenant_id,
        display_name: Some("Alice".into()),
        aliases: vec!["A".into()],
        bio: Some("Engineer.".into()),
    }).await.unwrap();

    let window = upsert_test_window(&pool, sid, 0, 300_000).await;
    // Stub returns a high-confidence item irrespective of profile contents.
    let router = LlmRouter::stub(vec![stub_item(
        "Draft summary", "high",
        "draft the design review summary by Thursday morning",
        Some("Alice"),
    )]);

    let outcome = process_window_with(&pool, &router, &window).await.unwrap();
    assert!(matches!(outcome, ProcessOutcome::Produced(1)));
}
```

- [ ] **Step 2: Run — should fail (compile error: 3-arg call site, plus missing profile fetch)**

Run: `cd backend && cargo test -p actio-core --lib window_extractor::tests::process_window_with_uses_profile_when_set`

Expected: FAIL — current code calls `generate_action_items_with_refs` with 3 args.

- [ ] **Step 3: Edit `process_window_with` to fetch the profile and pass it**

Locate `process_window_with` in `window_extractor.rs`. After `fetch_session_started_at` and before the call to `router.generate_action_items_with_refs`, add:

```rust
let profile = crate::repository::tenant_profile::get_for_tenant(pool, tenant_id)
    .await
    .map_err(|e| ProcessError::Permanent(format!("profile lookup: {e}")))?;
```

(Use whichever variable in the existing function holds `tenant_id` — typically derived from `session_started_at.tenant_id`.)

Then change the router call:

```rust
let items = match router
    .generate_action_items_with_refs(&attributed, &label_names, &window_local_date, profile.as_ref())
    .await
{
    // ... unchanged arms
};
```

- [ ] **Step 4: Run all window_extractor tests**

Run: `cd backend && cargo test -p actio-core --lib window_extractor::tests`

Expected: all pre-existing tests + the new one pass.

- [ ] **Step 5: Commit**

```bash
git add backend/actio-core/src/engine/window_extractor.rs
git commit -m "feat(extractor): pass tenant profile to LLM in process_window_with"
```

---

### Task 9: Profile lookup in `extract_for_clip`

**Files:**
- Modify: `backend/actio-core/src/engine/window_extractor.rs`

- [ ] **Step 1: Add the failing test**

Append:

```rust
#[tokio::test]
async fn extract_for_clip_uses_profile_when_set() {
    use crate::domain::types::TenantProfile;

    let pool = fresh_pool().await;
    let sid = mk_session(&pool).await;
    let alice = mk_speaker(&pool, "Alice").await;
    let clip_id = mk_clip(&pool, sid, 0, 300_000).await;
    let seg = mk_clip_segment(&pool, sid, clip_id, alice, 1_000, 30_000).await;
    mk_final_transcript_for_segment(
        &pool, sid, seg,
        "Please draft the design review summary by Thursday morning so the team can read it.",
        1_000, 30_000,
    ).await;

    let tenant_row: (String,) = sqlx::query_as(
        "SELECT tenant_id FROM audio_sessions WHERE id = ?1"
    ).bind(sid.to_string()).fetch_one(&pool).await.unwrap();
    let tenant_id = Uuid::parse_str(&tenant_row.0).unwrap();
    crate::repository::tenant_profile::upsert(&pool, &TenantProfile {
        tenant_id,
        display_name: Some("Alice".into()),
        aliases: vec![],
        bio: None,
    }).await.unwrap();

    let router = LlmRouter::stub(vec![stub_item(
        "Draft summary", "high",
        "draft the design review summary by Thursday morning",
        Some("Alice"),
    )]);

    extract_for_clip(&pool, &router, clip_id).await.unwrap();

    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM reminders WHERE session_id = ?1"
    ).bind(sid.to_string()).fetch_one(&pool).await.unwrap();
    assert_eq!(count.0, 1);
}
```

- [ ] **Step 2: Run — should fail (compile error for now)**

Run: `cd backend && cargo test -p actio-core --lib window_extractor::tests::extract_for_clip_uses_profile_when_set`

Expected: FAIL.

- [ ] **Step 3: Update `extract_for_clip`**

Inside `extract_for_clip`, after the existing `session = fetch_session_started_at(...)` line, add:

```rust
let profile = crate::repository::tenant_profile::get_for_tenant(pool, session.tenant_id)
    .await
    .ok()
    .flatten();
```

(Use `.ok().flatten()` — clip extraction's contract is best-effort: a profile-fetch DB error must not block clip extraction. The legacy prompt path will run instead.)

Then change the router call:

```rust
let items = match router
    .generate_action_items_with_refs(&attributed, &label_names, &window_local_date, profile.as_ref())
    .await
{
    // ... unchanged arms
};
```

- [ ] **Step 4: Run window_extractor tests**

Run: `cd backend && cargo test -p actio-core --lib window_extractor::tests`

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add backend/actio-core/src/engine/window_extractor.rs
git commit -m "feat(extractor): pass tenant profile to LLM in extract_for_clip"
```

---

### Task 10: API endpoints for tenant_profile

**Files:**
- Create: `backend/actio-core/src/api/profile.rs`
- Modify: `backend/actio-core/src/api/mod.rs`

- [ ] **Step 1: Create the profile API module**

Create `backend/actio-core/src/api/profile.rs`:

```rust
//! GET / PUT /profile — tenant identity used by the action-item extractor.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::domain::types::TenantProfile;
use crate::repository::tenant_profile as repo;
use crate::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateProfileRequest {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub aliases: Option<Vec<String>>,
    #[serde(default)]
    pub bio: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ProfileResponse {
    pub tenant_id: uuid::Uuid,
    pub display_name: Option<String>,
    pub aliases: Vec<String>,
    pub bio: Option<String>,
}

impl From<TenantProfile> for ProfileResponse {
    fn from(p: TenantProfile) -> Self {
        Self {
            tenant_id: p.tenant_id,
            display_name: p.display_name,
            aliases: p.aliases,
            bio: p.bio,
        }
    }
}

#[utoipa::path(get, path = "/profile",
    responses(
        (status = 200, body = ProfileResponse),
        (status = 404, description = "No profile set"),
    ))]
pub async fn get_profile(State(state): State<AppState>) -> impl IntoResponse {
    let tenant_id = state.tenant_id();
    match repo::get_for_tenant(&state.pool, tenant_id).await {
        Ok(Some(p)) => Json(ProfileResponse::from(p)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::warn!(error = %e, "get_profile failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[utoipa::path(put, path = "/profile",
    request_body = UpdateProfileRequest,
    responses((status = 200, body = ProfileResponse)))]
pub async fn put_profile(
    State(state): State<AppState>,
    Json(req): Json<UpdateProfileRequest>,
) -> impl IntoResponse {
    let tenant_id = state.tenant_id();
    let existing = repo::get_for_tenant(&state.pool, tenant_id)
        .await
        .ok()
        .flatten();
    let merged = TenantProfile {
        tenant_id,
        display_name: req.display_name.or(existing.as_ref().and_then(|p| p.display_name.clone())),
        aliases: req.aliases.unwrap_or_else(|| existing.as_ref().map(|p| p.aliases.clone()).unwrap_or_default()),
        bio: req.bio.or(existing.and_then(|p| p.bio)),
    };
    if let Err(e) = repo::upsert(&state.pool, &merged).await {
        tracing::warn!(error = %e, "put_profile upsert failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    Json(ProfileResponse::from(merged)).into_response()
}
```

(If `AppState` does not currently expose a `tenant_id()` accessor, fall back to whatever pattern the existing `api/settings.rs` uses to derive a tenant — likely a single hard-coded `Uuid::nil()` or a value pulled from settings. Match that existing pattern.)

- [ ] **Step 2: Wire the routes**

Edit `backend/actio-core/src/api/mod.rs`:

```rust
pub mod profile;     // ← add alphabetical
```

Inside the `Router::new()` builder (search for `.route("/labels"` or similar to find the chain), add:

```rust
.route("/profile", get(profile::get_profile).put(profile::put_profile))
```

Add the new module to the `#[openapi(paths(...))]` block:

```rust
profile::get_profile,
profile::put_profile,
```

And to the `components(schemas(...))` block:

```rust
profile::ProfileResponse,
profile::UpdateProfileRequest,
```

- [ ] **Step 3: Verify build**

Run: `cd backend && cargo check -p actio-core`

Expected: compiles. If `tenant_id()` accessor was the issue, look at how `api/settings.rs` or `api/label.rs` handles tenant scoping today and copy the exact pattern.

- [ ] **Step 4: Smoke-test the endpoints**

Start the backend in another terminal: `cd backend && cargo run --bin actio-asr`

In a fresh shell:

```bash
curl -i http://localhost:3000/profile     # expect 404 first time
curl -i -X PUT http://localhost:3000/profile \
  -H 'content-type: application/json' \
  -d '{"display_name":"Dake","aliases":["DK","彭大可"],"bio":"Solo dev."}'
curl -s http://localhost:3000/profile | jq .
```

Expected: 404, then PUT returns the merged body, then GET returns it.

- [ ] **Step 5: Commit**

```bash
git add backend/actio-core/src/api/profile.rs backend/actio-core/src/api/mod.rs
git commit -m "feat(api): GET/PUT /profile for tenant identity"
```

---

### Task 11: API endpoint POST /speakers/:id/mark-self

**Files:**
- Modify: whichever file in `backend/actio-core/src/api/` registers speaker routes (search below)

- [ ] **Step 1: Locate the existing speaker routes**

Run: `cd backend && grep -rn "/speakers" src/api/ | head`

Likely in `api/segment.rs` (since `enroll_speaker`, `update_speaker` live in `api/mod.rs`'s OpenAPI block but the handlers may be elsewhere) or in `api/mod.rs` directly.

- [ ] **Step 2: Add the handler**

In the same file as the other speaker handlers, append:

```rust
#[utoipa::path(post, path = "/speakers/{id}/mark-self",
    params(("id" = String, Path)),
    responses(
        (status = 204, description = "Marked as self"),
        (status = 404, description = "Speaker not found"),
    ))]
pub async fn mark_speaker_as_self(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let speaker_id = match uuid::Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return axum::http::StatusCode::BAD_REQUEST.into_response(),
    };
    match crate::repository::speaker::mark_as_self(&state.pool, speaker_id).await {
        Ok(()) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(sqlx::Error::RowNotFound) => axum::http::StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::warn!(error = %e, "mark_as_self failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
```

- [ ] **Step 3: Register the route**

In `api/mod.rs`, in the router builder:

```rust
.route("/speakers/:id/mark-self", post(<module>::mark_speaker_as_self))
```

(Replace `<module>` with whatever path matches where you put the handler.)

Add the path to the OpenAPI `paths(...)` block.

- [ ] **Step 4: Verify build**

Run: `cd backend && cargo check -p actio-core`

Expected: PASS.

- [ ] **Step 5: Smoke test**

```bash
# Need an existing speaker — list first.
curl -s http://localhost:3000/speakers | jq '.[0].id'
# Mark one:
curl -i -X POST http://localhost:3000/speakers/<uuid>/mark-self
# Re-fetch to confirm is_self=1 (only visible if your speaker DTO surfaces it):
curl -s http://localhost:3000/speakers | jq '.[].is_self'
```

If the speaker DTO doesn't currently include `is_self`, add it (one line in the `Speaker` serialization struct + the SELECT). This is necessary for the frontend toggle to render correct state on load.

- [ ] **Step 6: Commit**

```bash
git add backend/actio-core/src/api/
git commit -m "feat(api): POST /speakers/:id/mark-self and surface is_self in DTO"
```

---

### Task 12: Frontend API client for /profile

**Files:**
- Create: `frontend/src/api/profile.ts`

- [ ] **Step 1: Create the client**

```typescript
import { getBackendUrl } from './backend-url';

export type ProfileResponse = {
  tenant_id: string;
  display_name: string | null;
  aliases: string[];
  bio: string | null;
};

export type UpdateProfileRequest = {
  display_name?: string | null;
  aliases?: string[];
  bio?: string | null;
};

export async function fetchProfile(): Promise<ProfileResponse | null> {
  const res = await fetch(`${getBackendUrl()}/profile`);
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`fetchProfile failed: ${res.status}`);
  return res.json();
}

export async function updateProfile(req: UpdateProfileRequest): Promise<ProfileResponse> {
  const res = await fetch(`${getBackendUrl()}/profile`, {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(req),
  });
  if (!res.ok) throw new Error(`updateProfile failed: ${res.status}`);
  return res.json();
}
```

- [ ] **Step 2: Add a thin client for mark-self**

Append to `frontend/src/api/speakers.ts`:

```typescript
export async function markSpeakerAsSelf(speakerId: string): Promise<void> {
  const res = await fetch(`${getBackendUrl()}/speakers/${speakerId}/mark-self`, {
    method: 'POST',
  });
  if (!res.ok) throw new Error(`markSpeakerAsSelf failed: ${res.status}`);
}
```

(If `speakers.ts` doesn't already import `getBackendUrl`, add the import.)

- [ ] **Step 3: Typecheck**

Run: `cd frontend && pnpm tsc --noEmit`

Expected: no new errors.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/api/profile.ts frontend/src/api/speakers.ts
git commit -m "feat(frontend-api): profile + markSpeakerAsSelf clients"
```

---

### Task 13: Extend the ProfileSection (About me)

**Files:**
- Modify: `frontend/src/components/settings/ProfileSection.tsx`
- Modify: `frontend/src/store/use-store.ts`
- Modify: `frontend/src/i18n/en.ts`
- Modify: `frontend/src/i18n/zh-CN.ts`

- [ ] **Step 1: Extend the store profile slice**

Open `frontend/src/store/use-store.ts`. Find the existing `profile` state (currently `{ name }`). Replace with:

```typescript
type Profile = {
  display_name: string;
  aliases: string[];
  bio: string;
  loaded: boolean;       // false until first server fetch resolves
};

// Inside the store:
profile: { display_name: '', aliases: [], bio: '', loaded: false } as Profile,

setProfile: (patch: Partial<Profile>) =>
  set((s) => ({ profile: { ...s.profile, ...patch } })),

loadProfile: async () => {
  const remote = await fetchProfile();
  if (remote) {
    set({ profile: {
      display_name: remote.display_name ?? '',
      aliases: remote.aliases,
      bio: remote.bio ?? '',
      loaded: true,
    }});
  } else {
    set((s) => ({ profile: { ...s.profile, loaded: true }}));
  }
},

saveProfile: async () => {
  const p = get().profile;
  await updateProfile({
    display_name: p.display_name,
    aliases: p.aliases,
    bio: p.bio,
  });
},
```

(Add `import { fetchProfile, updateProfile } from '../api/profile';` at the top.)

- [ ] **Step 2: Trigger initial load**

In the existing app-bootstrap effect (search for where `loadSettings` or similar is called on mount — usually `App.tsx` or a root component), call `loadProfile()` alongside it.

- [ ] **Step 3: Replace ProfileSection.tsx**

```tsx
import { useStore } from '../../store/use-store';
import { useT } from '../../i18n';
import { useState } from 'react';

export function ProfileSection() {
  const profile = useStore((s) => s.profile);
  const setProfile = useStore((s) => s.setProfile);
  const saveProfile = useStore((s) => s.saveProfile);
  const t = useT();
  const [draftAlias, setDraftAlias] = useState('');

  const addAlias = () => {
    const v = draftAlias.trim();
    if (!v) return;
    // Per-script case-insensitive dedupe: ASCII lowercased; non-ASCII as-is.
    const isAscii = /^[\x00-\x7F]*$/.test(v);
    const norm = (s: string) =>
      /^[\x00-\x7F]*$/.test(s) ? s.toLowerCase() : s;
    if (profile.aliases.some((a) => norm(a) === norm(v))) {
      setDraftAlias('');
      return;
    }
    setProfile({ aliases: [...profile.aliases, v] });
    setDraftAlias('');
  };

  const removeAlias = (a: string) =>
    setProfile({ aliases: profile.aliases.filter((x) => x !== a) });

  return (
    <section className="settings-section">
      <div className="settings-section__title">{t('settings.profile.title')}</div>

      <div className="settings-field">
        <label className="settings-field__label" htmlFor="profile-name">
          {t('settings.profile.name')}
        </label>
        <input
          id="profile-name"
          type="text"
          className="settings-input"
          value={profile.display_name}
          onChange={(e) => setProfile({ display_name: e.target.value })}
          placeholder={t('settings.profile.namePlaceholder')}
        />
      </div>

      <div className="settings-field">
        <label className="settings-field__label" htmlFor="profile-aliases">
          {t('settings.profile.aliases')}
        </label>
        <div className="alias-chips">
          {profile.aliases.map((a) => (
            <span key={a} className="alias-chip">
              {a}
              <button type="button" onClick={() => removeAlias(a)} aria-label={t('settings.profile.removeAlias')}>×</button>
            </span>
          ))}
        </div>
        <input
          id="profile-aliases"
          type="text"
          className="settings-input"
          value={draftAlias}
          onChange={(e) => setDraftAlias(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); addAlias(); } }}
          placeholder={t('settings.profile.aliasesPlaceholder')}
        />
      </div>

      <div className="settings-field">
        <label className="settings-field__label" htmlFor="profile-bio">
          {t('settings.profile.bio')}
        </label>
        <textarea
          id="profile-bio"
          className="settings-input"
          rows={4}
          value={profile.bio}
          onChange={(e) => setProfile({ bio: e.target.value })}
          placeholder={t('settings.profile.bioPlaceholder')}
        />
      </div>

      <button type="button" className="settings-button" onClick={saveProfile}>
        {t('settings.profile.save')}
      </button>
    </section>
  );
}
```

- [ ] **Step 4: Add i18n keys (English)**

In `frontend/src/i18n/en.ts`, under (or extending) the existing `settings.profile.*` block:

```typescript
'settings.profile.title': 'About me',
'settings.profile.name': 'Display name',
'settings.profile.namePlaceholder': 'e.g. Dake Peng',
'settings.profile.aliases': 'Also called',
'settings.profile.aliasesPlaceholder': 'Type and press Enter (e.g. DK, 彭大可)',
'settings.profile.removeAlias': 'Remove alias',
'settings.profile.bio': 'About you',
'settings.profile.bioPlaceholder': 'A few sentences about who you are and what you care about. The action-item extractor reads this to decide what counts as relevant.',
'settings.profile.save': 'Save',
```

- [ ] **Step 5: Mirror in zh-CN**

In `frontend/src/i18n/zh-CN.ts` add the same keys with translations:

```typescript
'settings.profile.title': '关于我',
'settings.profile.name': '显示名称',
'settings.profile.namePlaceholder': '例如：彭大可',
'settings.profile.aliases': '其他称呼',
'settings.profile.aliasesPlaceholder': '输入后按回车添加（例如 DK、Dake）',
'settings.profile.removeAlias': '移除',
'settings.profile.bio': '个人简介',
'settings.profile.bioPlaceholder': '简单介绍一下你是谁、关心什么。提取器会读取这段文字来判断哪些事项与你相关。',
'settings.profile.save': '保存',
```

- [ ] **Step 6: Run i18n parity test**

Run: `cd frontend && pnpm exec vitest run src/i18n/__tests__/parity.test.ts`

Expected: PASS.

- [ ] **Step 7: Run typecheck and full unit tests**

```bash
cd frontend && pnpm tsc --noEmit && pnpm test
```

Expected: PASS.

- [ ] **Step 8: Manual smoke**

```bash
cd frontend && pnpm dev
```

In the browser: open Settings → About me. Confirm: name persists; aliases add via Enter; bio textarea works; Save sends a PUT (check network tab); reload → values rehydrate from the server.

- [ ] **Step 9: Commit**

```bash
git add frontend/src/components/settings/ProfileSection.tsx \
        frontend/src/store/use-store.ts \
        frontend/src/i18n/en.ts frontend/src/i18n/zh-CN.ts
git commit -m "feat(settings): About me — display name, aliases, bio synced to backend"
```

---

### Task 14: People-tab "This is me" toggle

**Files:**
- Modify: whichever component renders the People-tab speaker cards (search below)
- Modify: `frontend/src/i18n/en.ts`, `frontend/src/i18n/zh-CN.ts`

- [ ] **Step 1: Locate the speaker card component**

Run: `cd frontend && grep -rn "speakers" src/components/ | grep -i "people\|speaker" | head`

Likely a `PeopleTab.tsx` or `SpeakerCard.tsx` under `components/voice/` or `components/people/`.

- [ ] **Step 2: Add the toggle**

Inside the speaker card render, add (next to the existing display name / actions):

```tsx
<label className="speaker-card__self-toggle">
  <input
    type="checkbox"
    role="switch"
    aria-checked={speaker.is_self}
    checked={speaker.is_self}
    onChange={async () => {
      await markSpeakerAsSelf(speaker.id);
      await refreshSpeakers();   // existing list-refresh hook
    }}
  />
  <span>{t('people.thisIsMe')}</span>
</label>
```

(Import `markSpeakerAsSelf` from `../../api/speakers`; reuse the existing list-refresh action — find it via `grep` for `refreshSpeakers` or `loadSpeakers`.)

If the existing speaker list type doesn't include `is_self`, extend it:

```typescript
export type Speaker = {
  id: string;
  display_name: string;
  is_self: boolean;       // ← new
  // ... existing fields
};
```

- [ ] **Step 3: i18n keys**

`en.ts`:

```typescript
'people.thisIsMe': 'This is me',
```

`zh-CN.ts`:

```typescript
'people.thisIsMe': '这是我',
```

- [ ] **Step 4: Run parity, typecheck, tests**

```bash
cd frontend && pnpm exec vitest run src/i18n/__tests__/parity.test.ts && pnpm tsc --noEmit && pnpm test
```

Expected: PASS.

- [ ] **Step 5: Manual smoke**

In the running dev server, open the People tab. Toggle "This is me" on speaker A, then on speaker B. Confirm A's toggle clears (after refresh), B's stays on.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/ frontend/src/i18n/
git commit -m "feat(people): This is me toggle marks the device-owner speaker"
```

---

### Task 15: AboutMeSection unit test (frontend)

**Files:**
- Create: `frontend/src/components/settings/__tests__/AboutMeSection.test.tsx`

- [ ] **Step 1: Write the test**

```tsx
import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { ProfileSection } from '../ProfileSection';

vi.mock('../../../api/profile', () => ({
  fetchProfile: vi.fn(),
  updateProfile: vi.fn(),
}));

import { updateProfile } from '../../../api/profile';

beforeEach(() => {
  vi.clearAllMocks();
});

describe('ProfileSection (About me)', () => {
  it('adds an alias on Enter and dedupes case-insensitively for ASCII', () => {
    render(<ProfileSection />);
    const input = screen.getByPlaceholderText(/Type and press Enter/i) as HTMLInputElement;
    fireEvent.change(input, { target: { value: 'Dake' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(screen.getByText('Dake')).toBeInTheDocument();
    fireEvent.change(input, { target: { value: 'dake' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    // Only one chip total.
    expect(screen.getAllByText(/dake/i).length).toBe(1);
  });

  it('preserves CJK alias verbatim', () => {
    render(<ProfileSection />);
    const input = screen.getByPlaceholderText(/Type and press Enter/i) as HTMLInputElement;
    fireEvent.change(input, { target: { value: '彭大可' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(screen.getByText('彭大可')).toBeInTheDocument();
  });

  it('Save triggers updateProfile', () => {
    render(<ProfileSection />);
    fireEvent.click(screen.getByText(/Save/i));
    expect(updateProfile).toHaveBeenCalledTimes(1);
  });
});
```

- [ ] **Step 2: Run**

Run: `cd frontend && pnpm exec vitest run src/components/settings/__tests__/AboutMeSection.test.tsx`

Expected: PASS. If the store relies on global side-effects on mount, you may need to mock the store too — copy the mock pattern from any other settings test (e.g. `__tests__/LlmSettings.test.tsx` if it exists).

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/settings/__tests__/AboutMeSection.test.tsx
git commit -m "test(settings): AboutMeSection chip dedup, CJK preservation, save"
```

---

### Task 16: Manual extraction smoke + final commit

**Files:** (none — verification only)

- [ ] **Step 1: Run the full backend test suite once**

```bash
cd backend && cargo test -p actio-core --lib
```

Expected: all pass.

- [ ] **Step 2: Run frontend tests once**

```bash
cd frontend && pnpm test
```

Expected: all pass.

- [ ] **Step 3: End-to-end smoke**

1. Start backend: `cd backend && cargo run --bin actio-asr`
2. Start frontend: `cd frontend && pnpm tauri:dev`
3. In Settings → About me, fill in your real name, aliases, bio. Save.
4. In People tab, mark your enrolled voiceprint as "This is me".
5. Speak some realistic mixed audio for ~6 minutes:
   - One genuine self-commitment with a deadline ("I need to ship the speaker fix by Friday").
   - One vague aspiration ("I should clean up that module sometime").
   - One thing said TO you with a deadline ("I'll send you the API doc by EOD").
   - One thing another voice (e.g. a podcast playing nearby) commits to ("we should tighten the sprint goals next week").
6. Wait for the next clip to be processed (check `audio_clips.status='processed'` via SQLite, or watch the board).
7. Inspect the board — expected: items 1 and 3 land on the board; items 2 and 4 do not.

- [ ] **Step 4: Note any deviations**

Record observations in your scratchpad. If the LLM keeps emitting the vague aspiration (item 2), tighten the bio with an explicit "don't track open-ended ideas" line and re-test. If something said TO you is missed (item 3), confirm that the "this is me" speaker tag rendered correctly in the prompt — log `attributed` from `process_window_with` to see what the LLM actually saw.

- [ ] **Step 5: No-op final commit (if any tweaks were made above)**

If you tightened the prompt or fixed a small bug during smoke, commit those individually. Otherwise nothing to do here.

---

## Self-Review Notes (post-write)

Quick check against the spec:

- ✅ `tenant_profile` table — Task 1
- ✅ `is_self` column + partial unique index — Task 1
- ✅ `tenant_profile` repo (get_for_tenant, upsert) — Task 3
- ✅ `speaker::mark_as_self` — Task 4
- ✅ `WINDOW_SYSTEM_PROMPT_PROFILED` + `build_window_messages(..., profile)` — Task 5
- ✅ `LlmRouter` + `RemoteLlmClient` thread profile — Tasks 6, 7
- ✅ `process_window_with` + `extract_for_clip` lookup profile — Tasks 8, 9
- ✅ `GET/PUT /profile` — Task 10
- ✅ `POST /speakers/:id/mark-self` — Task 11
- ✅ Frontend API clients — Task 12
- ✅ About me settings panel (display_name, aliases, bio) — Task 13
- ✅ "This is me" toggle on speaker cards — Task 14
- ✅ AboutMeSection unit tests — Task 15
- ✅ Manual dogfood checklist — Task 16
- ✅ i18n parity (en.ts + zh-CN.ts) — Tasks 13, 14
- ✅ Fallback when profile is missing — Task 5 (`build_window_messages(..., None)` keeps legacy template byte-for-byte; tested)

No placeholders, no TBDs. Method signatures are consistent: `build_window_messages(transcript, labels, date, profile)` is used identically in Tasks 5–9. `mark_as_self(pool, speaker_id)` is consistent across Tasks 4 and 11.

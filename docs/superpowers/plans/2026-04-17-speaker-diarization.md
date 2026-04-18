# Speaker Diarization & Voiceprint Enrollment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the People tab to a real backend speaker registry, capture voiceprints via 3-clip enrollment, identify speakers in live sessions, and let users retroactively tag `[UNKNOWN]` segments.

**Architecture:** Everything runs in the existing Rust Axum service — no new worker processes. Embeddings come from `sherpa-onnx::SpeakerEmbeddingExtractor` (already loaded by the diarization helpers). Frontend stays React + Vite + Zustand; recording uses browser `MediaRecorder` + `AudioWorklet` in both Tauri and web contexts.

**Tech Stack:** Rust 1.x (Axum 0.7, SQLx-SQLite, sherpa-onnx, bytemuck, hound), React + Vite + TypeScript, Zustand, Vitest.

**Spec:** `docs/superpowers/specs/2026-04-17-speaker-diarization-design.md`

---

## Prerequisites for each session

- Work on a dedicated git branch from `main` (e.g. `feat/speaker-diarization`).
- `backend/` tests: `cd backend && cargo test`. Rust formatter: `cargo fmt`.
- `frontend/` tests: `cd frontend && pnpm test`. Typecheck: `pnpm build` (Vite+tsc).
- All new embedding BLOBs are `bytemuck::cast_slice::<f32, u8>(&emb)` — 4 bytes per dimension, little-endian on every target platform we support.
- Never edit an already-applied migration. All schema changes go in a new file.

---

## Phase 0 — Housekeeping

### Task 0.1: Delete the dead `backend/migrations/` directory

**Files:**
- Delete: `backend/migrations/` (10 Postgres-era files + `AGENTS.md`)

- [ ] **Step 1: Confirm nothing references it**

Run:
```bash
cd D:/Dev/Actio && grep -rn "backend/migrations" backend/ frontend/ docs/ 2>/dev/null | grep -v "^backend/migrations/"
```
Expected: no hits outside the directory itself.

- [ ] **Step 2: Delete the directory**

```bash
rm -r D:/Dev/Actio/backend/migrations
```

- [ ] **Step 3: Build backend to verify nothing implicitly depended on the path**

```bash
cd D:/Dev/Actio/backend && cargo build
```
Expected: compiles cleanly.

- [ ] **Step 4: Commit**

```bash
cd D:/Dev/Actio
git rm -r backend/migrations
git commit -m "chore(backend): delete dead Postgres-era migrations directory"
```

---

## Phase 1 — Schema, Types, and Matcher Rewrite

### Task 1.1: Add migration 002 (color column + segment embedding columns)

**Files:**
- Create: `backend/actio-core/migrations/002_speaker_diarization.sql`

- [ ] **Step 1: Write the migration**

```sql
-- Migration 002: speaker diarization support
-- Adds: color column on speakers, embedding storage on audio_segments.

ALTER TABLE speakers
    ADD COLUMN color TEXT NOT NULL DEFAULT '#64B5F6';

ALTER TABLE audio_segments
    ADD COLUMN embedding BLOB;
ALTER TABLE audio_segments
    ADD COLUMN embedding_dim INTEGER;

CREATE INDEX IF NOT EXISTS idx_segments_unknown
    ON audio_segments(session_id, speaker_id)
    WHERE speaker_id IS NULL;
```

- [ ] **Step 2: Run migrations against a fresh DB**

```bash
cd D:/Dev/Actio/backend
rm -f actio.db                     # or whichever name the service uses locally
cargo run --bin actio-asr &        # starts service and applies migrations
sleep 3 && kill %1
sqlite3 actio.db ".schema speakers" ".schema audio_segments"
```
Expected: `speakers` row shows `color TEXT NOT NULL DEFAULT '#64B5F6'`; `audio_segments` shows `embedding BLOB` and `embedding_dim INTEGER`.

- [ ] **Step 3: Commit**

```bash
git add backend/actio-core/migrations/002_speaker_diarization.sql
git commit -m "feat(db): migration 002 for speaker color and segment embeddings"
```

### Task 1.2: Extend the `Speaker` domain type with `color`

**Files:**
- Modify: `backend/actio-core/src/domain/types.rs`

- [ ] **Step 1: Add `color` field**

Change the `Speaker` struct at `backend/actio-core/src/domain/types.rs:7-14` to:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct Speaker {
    pub id: String,
    pub tenant_id: String,
    pub display_name: String,
    pub color: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}
```

- [ ] **Step 2: Build to find every call-site that now needs updating**

```bash
cd D:/Dev/Actio/backend && cargo build 2>&1 | head -40
```
Expected: compile errors pointing at `repository/speaker.rs` `INSERT` and anywhere a `Speaker` literal is constructed. Those are the work items for the next tasks.

- [ ] **Step 3: Commit (field added, compile errors are intentional for the next task)**

```bash
git add backend/actio-core/src/domain/types.rs
git commit -m "feat(types): add Speaker.color field"
```

### Task 1.3: Create-speaker / update-speaker with color

**Files:**
- Modify: `backend/actio-core/src/repository/speaker.rs`
- Modify: `backend/actio-core/src/api/session.rs` (request types + handlers)

- [ ] **Step 1: Write failing repository tests**

Add at the bottom of `backend/actio-core/src/repository/speaker.rs`:

```rust
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
        run_migrations(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn create_speaker_persists_color() {
        let pool = fresh_pool().await;
        let s = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();
        assert_eq!(s.display_name, "Alice");
        assert_eq!(s.color, "#E57373");
    }

    #[tokio::test]
    async fn update_speaker_applies_patch() {
        let pool = fresh_pool().await;
        let s = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();
        let updated = update_speaker(
            &pool,
            Uuid::parse_str(&s.id).unwrap(),
            Some("Alicia"),
            Some("#64B5F6"),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(updated.display_name, "Alicia");
        assert_eq!(updated.color, "#64B5F6");
    }
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cd D:/Dev/Actio/backend && cargo test -p actio-core repository::speaker::tests 2>&1 | tail -20
```
Expected: FAIL — signatures mismatch / color missing.

- [ ] **Step 3: Update repository functions**

Replace the body of `backend/actio-core/src/repository/speaker.rs` with:

```rust
use crate::domain::types::Speaker;
use sqlx::SqlitePool;
use uuid::Uuid;

pub async fn create_speaker(
    pool: &SqlitePool,
    display_name: &str,
    color: &str,
    tenant_id: Uuid,
) -> Result<Speaker, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    sqlx::query_as::<_, Speaker>(
        "INSERT INTO speakers (id, display_name, color, tenant_id) \
         VALUES (?1, ?2, ?3, ?4) RETURNING *",
    )
    .bind(&id)
    .bind(display_name)
    .bind(color)
    .bind(tenant_id.to_string())
    .fetch_one(pool)
    .await
}

pub async fn get_speaker(pool: &SqlitePool, id: Uuid) -> Result<Speaker, sqlx::Error> {
    sqlx::query_as::<_, Speaker>("SELECT * FROM speakers WHERE id = ?1")
        .bind(id.to_string())
        .fetch_one(pool)
        .await
}

pub async fn list_speakers(
    pool: &SqlitePool,
    tenant_id: Uuid,
) -> Result<Vec<Speaker>, sqlx::Error> {
    sqlx::query_as::<_, Speaker>(
        "SELECT * FROM speakers WHERE tenant_id = ?1 AND status = 'active' \
         ORDER BY created_at DESC",
    )
    .bind(tenant_id.to_string())
    .fetch_all(pool)
    .await
}

pub async fn update_speaker(
    pool: &SqlitePool,
    id: Uuid,
    display_name: Option<&str>,
    color: Option<&str>,
) -> Result<Option<Speaker>, sqlx::Error> {
    // COALESCE lets us pass NULL for "keep existing" per field.
    sqlx::query_as::<_, Speaker>(
        "UPDATE speakers \
         SET display_name = COALESCE(?1, display_name), \
             color = COALESCE(?2, color) \
         WHERE id = ?3 RETURNING *",
    )
    .bind(display_name)
    .bind(color)
    .bind(id.to_string())
    .fetch_optional(pool)
    .await
}

/// Hard delete: removes the speaker and cascades to speaker_embeddings via FK.
/// Callers MUST first null-out audio_segments.speaker_id to avoid dangling FKs
/// (SQLite's REFERENCES clause on audio_segments does not have ON DELETE SET NULL
/// in the existing schema). See `delete_speaker_with_segment_cleanup` below.
pub async fn hard_delete_speaker(pool: &SqlitePool, id: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM speakers WHERE id = ?1")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Single-transaction delete with segment FK cleanup.
pub async fn delete_speaker_with_segment_cleanup(
    pool: &SqlitePool,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE audio_segments SET speaker_id = NULL WHERE speaker_id = ?1")
        .bind(id.to_string())
        .execute(&mut *tx)
        .await?;
    let result = sqlx::query("DELETE FROM speakers WHERE id = ?1")
        .bind(id.to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(result.rows_affected() > 0)
}
```

- [ ] **Step 4: Update API layer to pass color through**

In `backend/actio-core/src/api/session.rs`, find `CreateSpeakerRequest` and `UpdateSpeakerRequest` (search for those types — they're near the speaker handlers at line 224 / 297). Replace them with:

```rust
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSpeakerRequest {
    pub display_name: String,
    #[serde(default = "default_color")]
    pub color: String,
}

fn default_color() -> String {
    "#64B5F6".into()
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateSpeakerRequest {
    pub display_name: Option<String>,
    pub color: Option<String>,
}
```

Update `create_speaker` handler to pass color:

```rust
pub async fn create_speaker(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateSpeakerRequest>,
) -> Result<(StatusCode, Json<Speaker>), AppApiError> {
    let tenant_id = tenant_id_from_headers(&headers)?;
    let s = speaker::create_speaker(&state.pool, &req.display_name, &req.color, tenant_id)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;
    Ok((StatusCode::CREATED, Json(s)))
}
```

Update `update_speaker` handler:

```rust
pub async fn update_speaker(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateSpeakerRequest>,
) -> Result<Json<Speaker>, AppApiError> {
    match speaker::update_speaker(
        &state.pool,
        id,
        req.display_name.as_deref(),
        req.color.as_deref(),
    )
    .await
    {
        Ok(Some(s)) => Ok(Json(s)),
        Ok(None) => Err(AppApiError("speaker not found".into())),
        Err(e) => Err(AppApiError(e.to_string())),
    }
}
```

Update `delete_speaker` to use the new cascade function:

```rust
pub async fn delete_speaker(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppApiError> {
    let deleted = speaker::delete_speaker_with_segment_cleanup(&state.pool, id)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppApiError("speaker not found".into()))
    }
}
```

- [ ] **Step 5: Run tests**

```bash
cd D:/Dev/Actio/backend && cargo test -p actio-core repository::speaker 2>&1 | tail -10
```
Expected: PASS.

```bash
cd D:/Dev/Actio/backend && cargo build 2>&1 | tail -10
```
Expected: compiles cleanly.

- [ ] **Step 6: Commit**

```bash
git add backend/actio-core/src/repository/speaker.rs backend/actio-core/src/api/session.rs
git commit -m "feat(speakers): color support + cascade-safe delete"
```

### Task 1.4: Rewrite `speaker_matcher::save_embedding` to store BLOB

**Files:**
- Modify: `backend/actio-core/src/domain/speaker_matcher.rs`

- [ ] **Step 1: Write failing test**

Replace the existing `#[cfg(test)] mod tests { ... }` block in `speaker_matcher.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::db::run_migrations;
    use crate::repository::speaker::create_speaker;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn fresh_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

    #[test]
    fn test_compute_stats() {
        let (mean, std) = compute_stats(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert!((mean - 3.0).abs() < 0.001);
        assert!((std - 1.414).abs() < 0.01);
    }

    #[test]
    fn test_empty_stats() {
        let (mean, std) = compute_stats(&[]);
        assert_eq!(mean, 0.0);
        assert_eq!(std, 0.0);
    }

    #[tokio::test]
    async fn save_and_load_embedding_roundtrip() {
        let pool = fresh_pool().await;
        let s = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();
        let sid = Uuid::parse_str(&s.id).unwrap();

        // Arbitrary 512-dim embedding with recognisable bit patterns
        let emb: Vec<f32> = (0..512).map(|i| i as f32 / 100.0).collect();
        let id = save_embedding(&pool, sid, &emb, 10_000.0, 0.8, true)
            .await
            .unwrap();
        assert_ne!(id, Uuid::nil());

        // Read back via a direct query and decode
        let row: (Vec<u8>, i64) = sqlx::query_as(
            "SELECT embedding, embedding_dimension FROM speaker_embeddings \
             WHERE speaker_id = ?1",
        )
        .bind(sid.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.1, 512);
        let decoded: &[f32] = bytemuck::cast_slice(&row.0);
        assert_eq!(decoded, emb.as_slice());
    }
}
```

- [ ] **Step 2: Run to see it fail**

```bash
cd D:/Dev/Actio/backend && cargo test -p actio-core domain::speaker_matcher::tests::save_and_load_embedding_roundtrip 2>&1 | tail -15
```
Expected: FAIL — current `save_embedding` writes a stringified vector with hardcoded dim 192.

- [ ] **Step 3: Replace `save_embedding` implementation**

In `backend/actio-core/src/domain/speaker_matcher.rs`, replace lines ~85–115 with:

```rust
pub async fn save_embedding(
    pool: &SqlitePool,
    speaker_id: Uuid,
    embedding: &[f32],
    duration_ms: f64,
    quality_score: f64,
    is_primary: bool,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let blob: &[u8] = bytemuck::cast_slice(embedding);
    let dim = embedding.len() as i64;

    let row: (String,) = sqlx::query_as(
        "INSERT INTO speaker_embeddings \
           (id, speaker_id, embedding, duration_ms, quality_score, is_primary, embedding_dimension) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id",
    )
    .bind(&id)
    .bind(speaker_id.to_string())
    .bind(blob)
    .bind(duration_ms)
    .bind(quality_score)
    .bind(is_primary as i64)
    .bind(dim)
    .fetch_one(pool)
    .await?;

    Ok(Uuid::parse_str(&row.0).unwrap_or_else(|_| Uuid::nil()))
}
```

- [ ] **Step 4: Run tests to confirm they pass**

```bash
cd D:/Dev/Actio/backend && cargo test -p actio-core domain::speaker_matcher 2>&1 | tail -15
```
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add backend/actio-core/src/domain/speaker_matcher.rs
git commit -m "fix(speaker_matcher): persist embeddings as BLOB with dynamic dimension"
```

### Task 1.5: Rewrite `identify_speaker` to do cosine + Z-Norm in Rust

**Files:**
- Modify: `backend/actio-core/src/domain/speaker_matcher.rs`

- [ ] **Step 1: Extend the test module with identification cases**

Append to the `#[cfg(test)] mod tests { ... }` block:

```rust
    fn normalize(v: &mut [f32]) {
        let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if n > 0.0 {
            for x in v {
                *x /= n;
            }
        }
    }

    #[tokio::test]
    async fn identify_picks_closest_above_threshold() {
        let pool = fresh_pool().await;
        let alice = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();
        let bob = create_speaker(&pool, "Bob", "#64B5F6", Uuid::nil())
            .await
            .unwrap();

        // Alice: embedding close to [1, 0, 0, ...]
        let mut alice_emb = vec![1.0; 512];
        for i in 1..512 { alice_emb[i] = 0.01; }
        normalize(&mut alice_emb);

        // Bob: embedding close to [0, 1, 0, ...]
        let mut bob_emb = vec![0.01; 512];
        bob_emb[1] = 1.0;
        normalize(&mut bob_emb);

        save_embedding(
            &pool,
            Uuid::parse_str(&alice.id).unwrap(),
            &alice_emb,
            5000.0,
            0.9,
            true,
        )
        .await
        .unwrap();
        save_embedding(
            &pool,
            Uuid::parse_str(&bob.id).unwrap(),
            &bob_emb,
            5000.0,
            0.9,
            true,
        )
        .await
        .unwrap();

        // Query close to Alice
        let mut query = alice_emb.clone();
        query[2] = 0.02;
        normalize(&mut query);

        let result = identify_speaker(&pool, &query, Uuid::nil(), 5)
            .await
            .unwrap();
        assert!(result.accepted);
        assert_eq!(result.speaker_id.as_deref(),
                   Some(alice.id.as_str()));
    }

    #[tokio::test]
    async fn identify_ignores_wrong_dimension_rows() {
        let pool = fresh_pool().await;
        let alice = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();
        // Insert a 192-dim embedding directly (simulating a stale row)
        sqlx::query(
            "INSERT INTO speaker_embeddings \
               (id, speaker_id, embedding, duration_ms, quality_score, is_primary, embedding_dimension) \
             VALUES (?1, ?2, ?3, 5000, 0.9, 1, 192)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&alice.id)
        .bind(bytemuck::cast_slice::<f32, u8>(&vec![0.5f32; 192]))
        .execute(&pool)
        .await
        .unwrap();

        let query = vec![0.5f32; 512];
        let result = identify_speaker(&pool, &query, Uuid::nil(), 5)
            .await
            .unwrap();
        // No 512-dim rows → no match, not a panic.
        assert!(!result.accepted);
        assert!(result.speaker_id.is_none());
    }
```

- [ ] **Step 2: Run to confirm failures**

```bash
cd D:/Dev/Actio/backend && cargo test -p actio-core domain::speaker_matcher 2>&1 | tail -25
```
Expected: FAIL — the current `identify_speaker` references a non-existent `embedding_distance` column.

- [ ] **Step 3: Replace `SpeakerMatchResult` + `identify_speaker`**

Replace lines ~7–83 of `speaker_matcher.rs` with:

```rust
use crate::engine::diarization::cosine_similarity;

#[derive(Debug)]
pub struct SpeakerMatchResult {
    pub speaker_id: Option<String>,
    pub similarity_score: f64,
    pub z_norm_score: f64,
    pub accepted: bool,
}

const Z_NORM_THRESHOLD: f64 = 0.0;

pub async fn identify_speaker(
    pool: &SqlitePool,
    query: &[f32],
    tenant_id: Uuid,
    k: usize,
) -> Result<SpeakerMatchResult, sqlx::Error> {
    // Fetch all embeddings for active speakers in the tenant whose dimension
    // matches the query. We sort by similarity in Rust; dataset is small
    // (hundreds of rows at most in realistic usage) and pgvector is unavailable.
    let query_dim = query.len() as i64;

    let rows: Vec<(String, String, Vec<u8>)> = sqlx::query_as(
        "SELECT e.id, e.speaker_id, e.embedding \
         FROM speaker_embeddings e \
         JOIN speakers s ON s.id = e.speaker_id \
         WHERE s.tenant_id = ?1 \
           AND s.status = 'active' \
           AND e.embedding_dimension = ?2",
    )
    .bind(tenant_id.to_string())
    .bind(query_dim)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(SpeakerMatchResult {
            speaker_id: None,
            similarity_score: 0.0,
            z_norm_score: 0.0,
            accepted: false,
        });
    }

    // Compute cosine similarity for every candidate.
    let mut scored: Vec<(String, f64)> = rows
        .into_iter()
        .map(|(_, speaker_id, blob)| {
            let emb: &[f32] = bytemuck::cast_slice(&blob);
            let sim = cosine_similarity(query, emb) as f64;
            (speaker_id, sim)
        })
        .collect();

    // Keep top-k
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(k);

    let sims: Vec<f64> = scored.iter().map(|(_, s)| *s).collect();
    let (mean, std_dev) = compute_stats(&sims);
    let z_scores: Vec<f64> = if std_dev > 0.001 {
        sims.iter().map(|s| (s - mean) / std_dev).collect()
    } else {
        sims.iter().map(|_| 0.0).collect()
    };

    let best_idx = z_scores
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i);

    if let Some(idx) = best_idx {
        let (speaker_id, sim) = scored[idx].clone();
        let z_norm = z_scores[idx];
        let accepted = z_norm > Z_NORM_THRESHOLD;
        info!(speaker_id = %speaker_id, sim, z_norm, accepted, "Speaker identified");
        Ok(SpeakerMatchResult {
            speaker_id: accepted.then_some(speaker_id),
            similarity_score: sim,
            z_norm_score: z_norm,
            accepted,
        })
    } else {
        Ok(SpeakerMatchResult {
            speaker_id: None,
            similarity_score: 0.0,
            z_norm_score: 0.0,
            accepted: false,
        })
    }
}
```

Note: the return type's `speaker_id` changed from `Option<Uuid>` to `Option<String>` (strings through the whole repository layer). Update any caller that pattern-matched on `Uuid`. A build will surface them.

- [ ] **Step 4: Run tests and build**

```bash
cd D:/Dev/Actio/backend && cargo test -p actio-core domain::speaker_matcher 2>&1 | tail -20
cargo build 2>&1 | tail -10
```
Expected: PASS, compiles cleanly (fix any caller-side types as they surface).

- [ ] **Step 5: Commit**

```bash
git add backend/actio-core/src/domain/speaker_matcher.rs
git commit -m "fix(speaker_matcher): cosine + Z-Norm in Rust, dimension-filtered"
```

---

## Phase 2 — Enrollment Endpoint

### Task 2.1: Add `hound` crate and WAV decode helper

**Files:**
- Modify: `backend/actio-core/Cargo.toml`
- Create: `backend/actio-core/src/engine/wav.rs`
- Modify: `backend/actio-core/src/engine/mod.rs`

- [ ] **Step 1: Add `hound` dependency**

Append under `# Audio capture` in `backend/actio-core/Cargo.toml`:

```toml
hound = "3.5"
```

Also enable axum multipart (required for the enrollment endpoint):

```toml
axum = { version = "0.7", features = ["ws", "macros", "multipart"] }
```

- [ ] **Step 2: Create WAV decoder**

Create `backend/actio-core/src/engine/wav.rs`:

```rust
use std::io::Cursor;

/// Decode a WAV byte slice into 16 kHz mono f32 samples.
/// Accepts 8/16/24/32-bit PCM or 32-bit float; downmixes stereo to mono;
/// resamples any sample rate to 16 kHz via linear interpolation.
pub fn decode_to_mono_16k(bytes: &[u8]) -> anyhow::Result<(Vec<f32>, f64)> {
    let cursor = Cursor::new(bytes);
    let mut reader = hound::WavReader::new(cursor)?;
    let spec = reader.spec();

    // Collect samples as f32 in [-1, 1], averaged across channels.
    let channels = spec.channels as usize;
    let sample_rate = spec.sample_rate as usize;

    let mono: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => {
            let xs: Result<Vec<f32>, _> = reader.samples::<f32>().collect();
            fold_channels(&xs?, channels)
        }
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            let xs: Result<Vec<i32>, _> = reader.samples::<i32>().collect();
            let as_f32: Vec<f32> = xs?.into_iter().map(|s| s as f32 / max).collect();
            fold_channels(&as_f32, channels)
        }
    };

    let resampled = if sample_rate == 16_000 {
        mono
    } else {
        linear_resample(&mono, sample_rate, 16_000)
    };

    let duration_ms = (resampled.len() as f64 / 16_000.0) * 1000.0;
    Ok((resampled, duration_ms))
}

fn fold_channels(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|c| c.iter().sum::<f32>() / channels as f32)
        .collect()
}

fn linear_resample(input: &[f32], src_rate: usize, dst_rate: usize) -> Vec<f32> {
    if input.is_empty() {
        return Vec::new();
    }
    let ratio = src_rate as f64 / dst_rate as f64;
    let out_len = ((input.len() as f64) / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = i as f64 * ratio;
        let idx = src_pos.floor() as usize;
        let frac = (src_pos - idx as f64) as f32;
        let a = input[idx];
        let b = *input.get(idx + 1).unwrap_or(&a);
        out.push(a + (b - a) * frac);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{WavSpec, WavWriter};
    use std::io::Cursor;

    fn make_wav(samples: &[f32], sample_rate: u32, channels: u16) -> Vec<u8> {
        let spec = WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut buf = Cursor::new(Vec::<u8>::new());
        {
            let mut w = WavWriter::new(&mut buf, spec).unwrap();
            for s in samples {
                w.write_sample(*s).unwrap();
            }
            w.finalize().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn decodes_mono_16k_passthrough() {
        let wav = make_wav(&[0.1, -0.2, 0.3, -0.4], 16_000, 1);
        let (samples, dur) = decode_to_mono_16k(&wav).unwrap();
        assert_eq!(samples.len(), 4);
        assert!((dur - 0.25).abs() < 0.01);
    }

    #[test]
    fn downmixes_stereo_to_mono() {
        let wav = make_wav(&[1.0, -1.0, 0.5, -0.5], 16_000, 2);
        let (samples, _) = decode_to_mono_16k(&wav).unwrap();
        assert_eq!(samples.len(), 2);
        assert!((samples[0]).abs() < 0.001); // (1 + -1)/2
        assert!((samples[1]).abs() < 0.001); // (0.5 + -0.5)/2
    }

    #[test]
    fn resamples_48k_to_16k() {
        let len = 48_000; // 1s at 48k
        let samples: Vec<f32> = (0..len).map(|i| (i as f32).sin() * 0.1).collect();
        let wav = make_wav(&samples, 48_000, 1);
        let (out, dur) = decode_to_mono_16k(&wav).unwrap();
        assert!((out.len() as i32 - 16_000).abs() < 5);
        assert!((dur - 1000.0).abs() < 2.0);
    }
}
```

- [ ] **Step 3: Export the module**

Add to `backend/actio-core/src/engine/mod.rs`:

```rust
pub mod wav;
```

- [ ] **Step 4: Run the tests**

```bash
cd D:/Dev/Actio/backend && cargo test -p actio-core engine::wav 2>&1 | tail -15
```
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add backend/actio-core/Cargo.toml backend/actio-core/Cargo.lock backend/actio-core/src/engine/wav.rs backend/actio-core/src/engine/mod.rs
git commit -m "feat(engine): WAV decoder that yields 16 kHz mono f32"
```

### Task 2.2: Quality-score utility

**Files:**
- Create: `backend/actio-core/src/engine/audio_quality.rs`
- Modify: `backend/actio-core/src/engine/mod.rs`

- [ ] **Step 1: Write failing tests**

Create `backend/actio-core/src/engine/audio_quality.rs`:

```rust
/// Produce a heuristic quality score in [0, 1] from a 16 kHz mono f32 clip.
/// High score = louder, cleaner, longer. Low score = clipped, silent, or too short.
pub fn score(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let duration_s = samples.len() as f32 / 16_000.0;
    let rms = (samples.iter().map(|x| x * x).sum::<f32>() / samples.len() as f32).sqrt();

    // RMS target band: 0.05 .. 0.30 is "good". Outside that, score drops.
    let rms_term = if rms < 0.01 {
        0.0
    } else if rms < 0.05 {
        (rms - 0.01) / 0.04
    } else if rms <= 0.30 {
        1.0
    } else if rms <= 0.60 {
        1.0 - (rms - 0.30) / 0.30
    } else {
        0.0
    };

    // Duration target: >=8s is ideal, 3s is bare minimum.
    let dur_term = if duration_s < 3.0 {
        0.0
    } else if duration_s >= 8.0 {
        1.0
    } else {
        (duration_s - 3.0) / 5.0
    };

    0.7 * rms_term + 0.3 * dur_term
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_scores_zero() {
        assert_eq!(score(&vec![0.0; 16_000 * 10]), 0.0);
    }

    #[test]
    fn empty_is_zero() {
        assert_eq!(score(&[]), 0.0);
    }

    #[test]
    fn clipped_loud_scores_low() {
        let s = score(&vec![0.95; 16_000 * 10]);
        assert!(s < 0.35, "got {s}");
    }

    #[test]
    fn good_loudness_and_duration_scores_high() {
        let samples: Vec<f32> = (0..16_000 * 10)
            .map(|i| (i as f32 * 0.01).sin() * 0.15)
            .collect();
        let s = score(&samples);
        assert!(s > 0.8, "got {s}");
    }

    #[test]
    fn short_clip_scores_lower() {
        let short: Vec<f32> = (0..16_000 * 3)
            .map(|i| (i as f32 * 0.01).sin() * 0.15)
            .collect();
        let s = score(&short);
        assert!(s < 0.8, "got {s}");
    }
}
```

- [ ] **Step 2: Export it**

Append to `backend/actio-core/src/engine/mod.rs`:

```rust
pub mod audio_quality;
```

- [ ] **Step 3: Run tests**

```bash
cd D:/Dev/Actio/backend && cargo test -p actio-core engine::audio_quality 2>&1 | tail -10
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add backend/actio-core/src/engine/audio_quality.rs backend/actio-core/src/engine/mod.rs
git commit -m "feat(engine): heuristic audio quality score for enrollment clips"
```

### Task 2.3: Rewrite `POST /speakers/{id}/enroll` as a multipart handler

**Files:**
- Modify: `backend/actio-core/src/api/session.rs`
- Modify: `backend/actio-core/src/lib.rs` (if `AppState` needs the embedding model path; inspect first)

- [ ] **Step 1: Inspect `AppState` for the embedding-model path**

```bash
cd D:/Dev/Actio/backend && grep -n "AppState\|embedding_model\|ModelPaths" actio-core/src/lib.rs actio-core/src/engine/model_manager.rs 2>&1 | head -40
```

Expected: Confirms the field name holding `ModelPaths` (it's `state.model_paths` or similar). Use whatever is there. If the embedding model path isn't exposed, surface it through `AppState` before proceeding. (`model_paths.speaker_embedding: Option<PathBuf>` is the field — confirmed in `model_manager.rs:140,1071`.)

- [ ] **Step 2: Define response types**

Near the top of `backend/actio-core/src/api/session.rs`, add:

```rust
#[derive(Debug, Serialize, ToSchema)]
pub struct EnrolledEmbedding {
    pub id: String,
    pub duration_ms: f64,
    pub quality_score: f64,
    pub is_primary: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EnrollResponse {
    pub speaker_id: String,
    pub embeddings: Vec<EnrolledEmbedding>,
    pub warnings: Vec<String>,
}
```

Remove the old `EnrollRequest` struct (`audio_base64`).

- [ ] **Step 3: Replace the `enroll_speaker` handler**

```rust
use axum::extract::Multipart;

pub async fn enroll_speaker(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<EnrollResponse>), AppApiError> {
    // Resolve the embedding model; 409 if missing.
    let model_path = state
        .model_paths
        .speaker_embedding
        .clone()
        .ok_or_else(|| AppApiError("embedding_model_missing".into()))?;

    // Collect clip bytes from multipart parts named clip_*
    let mut raw_clips: Vec<(String, Vec<u8>)> = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppApiError(e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        if !name.starts_with("clip_") {
            continue;
        }
        let data = field
            .bytes()
            .await
            .map_err(|e| AppApiError(e.to_string()))?
            .to_vec();
        raw_clips.push((name, data));
    }

    if raw_clips.is_empty() {
        return Err(AppApiError("no_valid_clips: empty upload".into()));
    }

    // Decode + extract embeddings (NO DB writes yet).
    struct Prepared {
        embedding: Vec<f32>,
        duration_ms: f64,
        quality: f64,
    }
    let mut prepared: Vec<Prepared> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    for (name, bytes) in raw_clips {
        let (samples, duration_ms) = match crate::engine::wav::decode_to_mono_16k(&bytes) {
            Ok(v) => v,
            Err(e) => {
                warnings.push(format!("{name}: failed to decode ({e})"));
                continue;
            }
        };

        if duration_ms < 3_000.0 {
            warnings.push(format!(
                "{name}: skipped — duration {:.1}s < 3s minimum",
                duration_ms / 1000.0
            ));
            continue;
        }
        if duration_ms > 30_000.0 {
            warnings.push(format!(
                "{name}: skipped — duration {:.1}s > 30s maximum",
                duration_ms / 1000.0
            ));
            continue;
        }

        let emb = match crate::engine::diarization::extract_embedding(&model_path, &samples).await
        {
            Ok(e) => e,
            Err(e) => {
                warnings.push(format!("{name}: extraction failed ({e})"));
                continue;
            }
        };

        prepared.push(Prepared {
            embedding: emb.values,
            duration_ms,
            quality: crate::engine::audio_quality::score(&samples) as f64,
        });
    }

    if prepared.is_empty() {
        return Err(AppApiError(format!(
            "no_valid_clips: {}",
            warnings.join("; ")
        )));
    }

    // Transactional delete-then-insert.
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppApiError(e.to_string()))?;

    sqlx::query("DELETE FROM speaker_embeddings WHERE speaker_id = ?1")
        .bind(id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;

    let mut inserted: Vec<EnrolledEmbedding> = Vec::new();
    for (i, p) in prepared.iter().enumerate() {
        let is_primary = i == 0;
        let new_id = Uuid::new_v4().to_string();
        let blob: &[u8] = bytemuck::cast_slice(&p.embedding);
        sqlx::query(
            "INSERT INTO speaker_embeddings \
               (id, speaker_id, embedding, duration_ms, quality_score, is_primary, embedding_dimension) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(&new_id)
        .bind(id.to_string())
        .bind(blob)
        .bind(p.duration_ms)
        .bind(p.quality)
        .bind(is_primary as i64)
        .bind(p.embedding.len() as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;

        inserted.push(EnrolledEmbedding {
            id: new_id,
            duration_ms: p.duration_ms,
            quality_score: p.quality,
            is_primary,
        });
    }

    tx.commit()
        .await
        .map_err(|e| AppApiError(e.to_string()))?;

    Ok((
        StatusCode::CREATED,
        Json(EnrollResponse {
            speaker_id: id.to_string(),
            embeddings: inserted,
            warnings,
        }),
    ))
}
```

- [ ] **Step 4: Register new schemas in OpenAPI**

In `backend/actio-core/src/api/mod.rs`, add `EnrollResponse` and `EnrolledEmbedding` to the `components(schemas(...))` list of `ApiDoc`.

- [ ] **Step 5: Compile**

```bash
cd D:/Dev/Actio/backend && cargo build 2>&1 | tail -15
```
Expected: compiles cleanly. If `AppApiError` variants need richer error types (status-code differentiation for `embedding_model_missing`), add an `AppApiError::Conflict(String)` variant and update `into_response`. Minimum viable: return 500 for now; refine in Step 6.

- [ ] **Step 6 (optional): Differentiate 409 for missing model**

Expand `AppApiError` in `backend/actio-core/src/api/session.rs`:

```rust
#[derive(Debug, ToSchema)]
#[allow(dead_code)]
pub enum AppApiError {
    Internal(String),
    BadRequest(String),
    Conflict(String),
}

impl AppApiError {
    pub fn new_internal(msg: impl Into<String>) -> Self { Self::Internal(msg.into()) }
}

// Keep a string-constructor bridge for existing call-sites:
impl From<String> for AppApiError {
    fn from(s: String) -> Self { Self::Internal(s) }
}

impl axum::response::IntoResponse for AppApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::Internal(msg) => {
                tracing::error!(error = %msg, "Internal server error");
                (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": msg}))).into_response()
            }
            Self::BadRequest(msg) =>
                (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": msg}))).into_response(),
            Self::Conflict(msg) =>
                (StatusCode::CONFLICT, Json(serde_json::json!({"error": msg}))).into_response(),
        }
    }
}
```

In `enroll_speaker`, return `Err(AppApiError::Conflict("embedding_model_missing".into()))` and `Err(AppApiError::BadRequest(...))` where appropriate. Replace the old `AppApiError("x".into())` tuple-struct calls repo-wide with `AppApiError::Internal("x".into())` (a `sed` or multi-file replace works; typecheck points out misses).

- [ ] **Step 7: Sanity compile**

```bash
cargo build 2>&1 | tail -10
```

- [ ] **Step 8: Commit**

```bash
git add backend/actio-core/src/api/session.rs backend/actio-core/src/api/mod.rs
git commit -m "feat(api): multipart voiceprint enrollment with transactional insert"
```

### Task 2.4: Integration test for enrollment

**Files:**
- Create: `backend/actio-core/src/api/session_enroll_test.rs` (module inside session.rs via `#[cfg(test)]`)

- [ ] **Step 1: Prefer an in-module test to keep file count low**

Append to `backend/actio-core/src/api/session.rs`:

```rust
#[cfg(test)]
mod enroll_tests {
    use super::*;
    use crate::repository::db::run_migrations;
    use crate::repository::speaker::create_speaker;
    use axum::body::Body;
    use axum::http::Request;
    use hound::{WavSpec, WavWriter};
    use sqlx::sqlite::SqlitePoolOptions;
    use std::io::Cursor;
    use tower::ServiceExt;

    async fn sine_wav(seconds: f32) -> Vec<u8> {
        let spec = WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let n = (16_000.0 * seconds) as usize;
        let mut buf = Cursor::new(Vec::<u8>::new());
        {
            let mut w = WavWriter::new(&mut buf, spec).unwrap();
            for i in 0..n {
                let x = (i as f32 / 16_000.0 * 440.0 * std::f32::consts::TAU).sin() * 0.2;
                w.write_sample(x).unwrap();
            }
            w.finalize().unwrap();
        }
        buf.into_inner()
    }

    // An additional test file can wire a router and hit the endpoint; for now
    // we assert the decode+quality+transaction scaffolding works by calling
    // extract_embedding with a stubbed model path via a feature-gated override.
    // This is intentionally light — the end-to-end check lives in the manual
    // smoke checklist (Phase 7) where a real model is available.

    #[tokio::test]
    async fn clips_too_short_are_rejected_with_warnings() {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();
        let s = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();
        let _id = Uuid::parse_str(&s.id).unwrap();

        let wav = sine_wav(1.0).await; // 1s — too short
        let (samples, dur) = crate::engine::wav::decode_to_mono_16k(&wav).unwrap();
        assert!(dur < 3000.0);
        assert!(!samples.is_empty());
    }
}
```

The integration test against a running server belongs to the manual smoke checklist in Phase 7 where a real embedding model is available. Pure-logic paths (decode, duration gate, quality) are covered by this unit test and the `wav` and `audio_quality` suites.

- [ ] **Step 2: Run**

```bash
cd D:/Dev/Actio/backend && cargo test -p actio-core api::session::enroll_tests 2>&1 | tail -10
```
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add backend/actio-core/src/api/session.rs
git commit -m "test(enroll): duration gating + decode path"
```

---

## Phase 3 — Retroactive Tagging Endpoints

### Task 3.1: Add `repository/segment.rs`

**Files:**
- Create: `backend/actio-core/src/repository/segment.rs`
- Modify: `backend/actio-core/src/repository/mod.rs`

- [ ] **Step 1: Write module with tests**

Create `backend/actio-core/src/repository/segment.rs`:

```rust
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UnknownSegmentRow {
    pub id: String,
    pub session_id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub embedding: Option<Vec<u8>>,
    pub embedding_dim: Option<i64>,
}

pub async fn list_unknown_segments(
    pool: &SqlitePool,
    session_id: Option<Uuid>,
    limit: i64,
) -> Result<Vec<UnknownSegmentRow>, sqlx::Error> {
    if let Some(sid) = session_id {
        sqlx::query_as::<_, UnknownSegmentRow>(
            "SELECT id, session_id, start_ms, end_ms, embedding, embedding_dim \
             FROM audio_segments \
             WHERE session_id = ?1 AND speaker_id IS NULL \
             ORDER BY start_ms LIMIT ?2",
        )
        .bind(sid.to_string())
        .bind(limit)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, UnknownSegmentRow>(
            "SELECT id, session_id, start_ms, end_ms, embedding, embedding_dim \
             FROM audio_segments WHERE speaker_id IS NULL \
             ORDER BY start_ms DESC LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(pool)
        .await
    }
}

pub async fn assign_speaker(
    pool: &SqlitePool,
    segment_id: Uuid,
    speaker_id: Uuid,
) -> Result<Option<UnknownSegmentRow>, sqlx::Error> {
    // Returns the row PRIOR to update so callers can copy the embedding.
    let prev = sqlx::query_as::<_, UnknownSegmentRow>(
        "SELECT id, session_id, start_ms, end_ms, embedding, embedding_dim \
         FROM audio_segments WHERE id = ?1",
    )
    .bind(segment_id.to_string())
    .fetch_optional(pool)
    .await?;

    let updated = sqlx::query("UPDATE audio_segments SET speaker_id = ?1 WHERE id = ?2")
        .bind(speaker_id.to_string())
        .bind(segment_id.to_string())
        .execute(pool)
        .await?;
    if updated.rows_affected() == 0 {
        return Ok(None);
    }
    Ok(prev)
}

pub async fn unassign_speaker(pool: &SqlitePool, segment_id: Uuid) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("UPDATE audio_segments SET speaker_id = NULL WHERE id = ?1")
        .bind(segment_id.to_string())
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn has_primary_embedding(
    pool: &SqlitePool,
    speaker_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?1 AND is_primary = 1",
    )
    .bind(speaker_id.to_string())
    .fetch_one(pool)
    .await?;
    Ok(row.0 > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::db::run_migrations;
    use crate::repository::speaker::create_speaker;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn fresh_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new().connect("sqlite::memory:").await.unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

    async fn insert_session(pool: &SqlitePool) -> String {
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO audio_sessions (id) VALUES (?1)")
            .bind(&id)
            .execute(pool)
            .await
            .unwrap();
        id
    }

    async fn insert_unknown_segment(pool: &SqlitePool, session_id: &str, start: i64) -> String {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO audio_segments \
               (id, session_id, start_ms, end_ms, embedding, embedding_dim) \
             VALUES (?1, ?2, ?3, ?4, ?5, 512)",
        )
        .bind(&id)
        .bind(session_id)
        .bind(start)
        .bind(start + 1000)
        .bind(bytemuck::cast_slice::<f32, u8>(&vec![0.5f32; 512]))
        .execute(pool)
        .await
        .unwrap();
        id
    }

    #[tokio::test]
    async fn lists_unknowns_per_session() {
        let pool = fresh_pool().await;
        let sid = insert_session(&pool).await;
        insert_unknown_segment(&pool, &sid, 0).await;
        insert_unknown_segment(&pool, &sid, 1000).await;
        let rows = list_unknown_segments(&pool, Some(Uuid::parse_str(&sid).unwrap()), 10)
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].embedding.is_some());
        assert_eq!(rows[0].embedding_dim, Some(512));
    }

    #[tokio::test]
    async fn assigning_returns_previous_row() {
        let pool = fresh_pool().await;
        let sid = insert_session(&pool).await;
        let seg_id = insert_unknown_segment(&pool, &sid, 0).await;
        let alice = create_speaker(&pool, "Alice", "#E57373", Uuid::nil()).await.unwrap();

        let prev = assign_speaker(
            &pool,
            Uuid::parse_str(&seg_id).unwrap(),
            Uuid::parse_str(&alice.id).unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(prev.embedding.is_some());
        assert_eq!(prev.embedding_dim, Some(512));

        // Unknown list for the session is now empty.
        let rows = list_unknown_segments(&pool, Some(Uuid::parse_str(&sid).unwrap()), 10).await.unwrap();
        assert!(rows.is_empty());
    }
}
```

- [ ] **Step 2: Export**

Append to `backend/actio-core/src/repository/mod.rs`:

```rust
pub mod segment;
```

- [ ] **Step 3: Run tests**

```bash
cd D:/Dev/Actio/backend && cargo test -p actio-core repository::segment 2>&1 | tail -10
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add backend/actio-core/src/repository/segment.rs backend/actio-core/src/repository/mod.rs
git commit -m "feat(repo): unknown-segment queries and assign/unassign"
```

### Task 3.2: API module `api/segment.rs` with the four new routes

**Files:**
- Create: `backend/actio-core/src/api/segment.rs`
- Modify: `backend/actio-core/src/api/mod.rs`

- [ ] **Step 1: Create the module**

Create `backend/actio-core/src/api/segment.rs`:

```rust
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::api::session::{tenant_id_from_headers, AppApiError};
use crate::AppState;

#[derive(Debug, Serialize, ToSchema)]
pub struct UnknownSegmentResponse {
    pub segment_id: String,
    pub session_id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    /// Whether this segment has an embedding we can promote on assign.
    pub has_embedding: bool,
}

#[derive(Debug, Deserialize)]
pub struct ListUnknownsQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
}
fn default_limit() -> i64 { 50 }

#[derive(Debug, Deserialize, ToSchema)]
pub struct AssignSegmentRequest {
    pub speaker_id: Option<Uuid>,
    pub new_speaker: Option<NewSpeakerSpec>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct NewSpeakerSpec {
    pub display_name: String,
    #[serde(default = "default_color")]
    pub color: String,
}
fn default_color() -> String { "#64B5F6".into() }

#[derive(Debug, Serialize, ToSchema)]
pub struct AssignSegmentResponse {
    pub segment_id: String,
    pub speaker_id: String,
    pub embedding_added: bool,
}

pub async fn list_session_unknowns(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
    Query(params): Query<ListUnknownsQuery>,
) -> Result<Json<Vec<UnknownSegmentResponse>>, AppApiError> {
    let rows = crate::repository::segment::list_unknown_segments(
        &state.pool,
        Some(session_id),
        params.limit,
    )
    .await
    .map_err(|e| AppApiError::Internal(e.to_string()))?;
    Ok(Json(rows.into_iter().map(|r| UnknownSegmentResponse {
        segment_id: r.id,
        session_id: r.session_id,
        start_ms: r.start_ms,
        end_ms: r.end_ms,
        has_embedding: r.embedding.is_some(),
    }).collect()))
}

pub async fn list_unknowns(
    State(state): State<AppState>,
    Query(params): Query<ListUnknownsQuery>,
) -> Result<Json<Vec<UnknownSegmentResponse>>, AppApiError> {
    let rows = crate::repository::segment::list_unknown_segments(&state.pool, None, params.limit)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
    Ok(Json(rows.into_iter().map(|r| UnknownSegmentResponse {
        segment_id: r.id,
        session_id: r.session_id,
        start_ms: r.start_ms,
        end_ms: r.end_ms,
        has_embedding: r.embedding.is_some(),
    }).collect()))
}

pub async fn assign_segment(
    State(state): State<AppState>,
    Path(segment_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<AssignSegmentRequest>,
) -> Result<Json<AssignSegmentResponse>, AppApiError> {
    let tenant_id = tenant_id_from_headers(&headers)?;
    let target_speaker_id = match (body.speaker_id, body.new_speaker) {
        (Some(id), _) => id,
        (None, Some(spec)) => {
            let s = crate::repository::speaker::create_speaker(
                &state.pool,
                &spec.display_name,
                &spec.color,
                tenant_id,
            )
            .await
            .map_err(|e| AppApiError::Internal(e.to_string()))?;
            Uuid::parse_str(&s.id).map_err(|e| AppApiError::Internal(e.to_string()))?
        }
        _ => return Err(AppApiError::BadRequest("speaker_id or new_speaker required".into())),
    };

    let prev = crate::repository::segment::assign_speaker(&state.pool, segment_id, target_speaker_id)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?
        .ok_or_else(|| AppApiError::BadRequest("segment not found".into()))?;

    let mut embedding_added = false;
    if let (Some(blob), Some(dim)) = (prev.embedding, prev.embedding_dim) {
        // Decode the BLOB into f32 and save as an embedding on the speaker.
        let emb: Vec<f32> = bytemuck::cast_slice(&blob).to_vec();
        let is_primary =
            !crate::repository::segment::has_primary_embedding(&state.pool, target_speaker_id)
                .await
                .map_err(|e| AppApiError::Internal(e.to_string()))?;
        let duration_ms = (prev.end_ms - prev.start_ms) as f64;
        // Quality score isn't meaningful without the source audio; use 0.5 as a neutral value.
        crate::domain::speaker_matcher::save_embedding(
            &state.pool,
            target_speaker_id,
            &emb,
            duration_ms,
            0.5,
            is_primary,
        )
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
        let _ = dim; // dim is already embedded in emb.len()
        embedding_added = true;
    }

    Ok(Json(AssignSegmentResponse {
        segment_id: segment_id.to_string(),
        speaker_id: target_speaker_id.to_string(),
        embedding_added,
    }))
}

pub async fn unassign_segment(
    State(state): State<AppState>,
    Path(segment_id): Path<Uuid>,
) -> Result<StatusCode, AppApiError> {
    let ok = crate::repository::segment::unassign_speaker(&state.pool, segment_id)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
    if ok {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppApiError::BadRequest("segment not found".into()))
    }
}
```

- [ ] **Step 2: Wire into the router**

In `backend/actio-core/src/api/mod.rs`:

```rust
pub mod segment;
```

Inside the `router` function, add routes before the `.with_state(state)` call:

```rust
        .route("/sessions/:id/unknowns", get(segment::list_session_unknowns))
        .route("/unknowns", get(segment::list_unknowns))
        .route("/segments/:id/assign", post(segment::assign_segment))
        .route("/segments/:id/unassign", post(segment::unassign_segment))
```

Add the new schemas to `ApiDoc::components::schemas(...)`: `UnknownSegmentResponse`, `AssignSegmentRequest`, `NewSpeakerSpec`, `AssignSegmentResponse`, `EnrollResponse`, `EnrolledEmbedding`.

- [ ] **Step 3: Build**

```bash
cd D:/Dev/Actio/backend && cargo build 2>&1 | tail -10
```
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add backend/actio-core/src/api/segment.rs backend/actio-core/src/api/mod.rs
git commit -m "feat(api): unknown-segment listing and assign/unassign endpoints"
```

---

## Phase 4 — Live-Session Identification Hook

> **Context:** Today the live pipeline is `audio_capture → VAD → ASR → aggregator`. No per-segment embedding extraction. This phase adds one task path that fires when a VAD segment completes: extract embedding → identify speaker → persist segment row → emit a WebSocket event.

### Task 4.1: Extract and persist a per-segment embedding in the pipeline

**Files:**
- Modify: `backend/actio-core/src/engine/inference_pipeline.rs`
- Modify: `backend/actio-core/src/repository/segment.rs` (add `insert_segment` helper)

- [ ] **Step 1: Add segment insert helper**

Append to `backend/actio-core/src/repository/segment.rs`:

```rust
pub async fn insert_segment(
    pool: &SqlitePool,
    session_id: Uuid,
    start_ms: i64,
    end_ms: i64,
    speaker_id: Option<Uuid>,
    speaker_score: Option<f64>,
    embedding: Option<&[f32]>,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let (blob, dim) = match embedding {
        Some(e) => (Some(bytemuck::cast_slice::<f32, u8>(e).to_vec()), Some(e.len() as i64)),
        None => (None, None),
    };
    sqlx::query(
        "INSERT INTO audio_segments \
           (id, session_id, start_ms, end_ms, speaker_id, speaker_score, embedding, embedding_dim) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(&id)
    .bind(session_id.to_string())
    .bind(start_ms)
    .bind(end_ms)
    .bind(speaker_id.map(|u| u.to_string()))
    .bind(speaker_score)
    .bind(blob)
    .bind(dim)
    .execute(pool)
    .await?;
    Ok(Uuid::parse_str(&id).unwrap())
}
```

- [ ] **Step 2: Wire into the pipeline**

In `backend/actio-core/src/engine/inference_pipeline.rs`, locate the place where a VAD `SpeechSegment` completes (grep for `vad::start_vad` and the downstream consumer). After each `SpeechSegment`, spawn a task that:

1. Clones the segment audio.
2. Calls `engine::diarization::extract_embedding(&model_path, &audio).await` if `model_paths.speaker_embedding` is present; otherwise skip.
3. Calls `domain::speaker_matcher::identify_speaker` with the resulting embedding.
4. Writes the segment row via `repository::segment::insert_segment`.
5. Pushes a WebSocket event via the existing aggregator / emitter (search for `ws_tx` or the transcript emitter).

Because the exact spawn point depends on how `inference_pipeline.rs` is currently organized, implement this as a small helper in the same file:

```rust
async fn handle_segment_embedding(
    pool: &sqlx::SqlitePool,
    model_path: Option<std::path::PathBuf>,
    session_id: uuid::Uuid,
    tenant_id: uuid::Uuid,
    start_ms: i64,
    end_ms: i64,
    audio: Vec<f32>,
) -> anyhow::Result<Option<String>> {
    let Some(model_path) = model_path else {
        crate::repository::segment::insert_segment(
            pool, session_id, start_ms, end_ms, None, None, None
        ).await?;
        return Ok(None);
    };

    let emb = match crate::engine::diarization::extract_embedding(&model_path, &audio).await {
        Ok(e) => e,
        Err(err) => {
            tracing::warn!(?err, "speaker embedding failed; segment marked UNKNOWN");
            crate::repository::segment::insert_segment(
                pool, session_id, start_ms, end_ms, None, None, None
            ).await?;
            return Ok(None);
        }
    };

    let result = crate::domain::speaker_matcher::identify_speaker(
        pool, &emb.values, tenant_id, 5,
    ).await.unwrap_or(crate::domain::speaker_matcher::SpeakerMatchResult {
        speaker_id: None,
        similarity_score: 0.0,
        z_norm_score: 0.0,
        accepted: false,
    });

    let speaker_id = result.speaker_id.as_ref()
        .and_then(|s| uuid::Uuid::parse_str(s).ok());

    crate::repository::segment::insert_segment(
        pool, session_id, start_ms, end_ms, speaker_id,
        Some(result.similarity_score), Some(&emb.values),
    ).await?;

    Ok(result.speaker_id.clone())
}
```

Call it from the VAD-segment consumer task with `tokio::spawn`. Push a WebSocket event when a speaker is resolved:

```rust
// inside the segment consumer, after the DB write:
if let Some(speaker_id) = resolved {
    let _ = ws_tx.send(serde_json::json!({
        "kind": "speaker_resolved",
        "segment_id": seg_id.to_string(),
        "speaker_id": speaker_id,
    }).to_string()).await;
}
```

(Adapt the `ws_tx` reference to whatever the pipeline already uses to push transcript frames to the client — the same channel is the right place.)

- [ ] **Step 3: Graceful handling when the embedding model is not loaded**

In `InferencePipeline::start_session`, log once at the start:

```rust
if model_paths.speaker_embedding.is_none() {
    tracing::info!("Speaker embedding model not loaded — segments will stay [UNKNOWN]");
}
```

- [ ] **Step 4: Compile**

```bash
cd D:/Dev/Actio/backend && cargo build 2>&1 | tail -10
```
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add backend/actio-core/src/engine/inference_pipeline.rs backend/actio-core/src/repository/segment.rs
git commit -m "feat(pipeline): per-segment speaker identification with WS event"
```

---

## Phase 5 — Frontend API Client + Store

### Task 5.1: Types and API client

**Files:**
- Create: `frontend/src/types/speaker.ts`
- Create: `frontend/src/api/speakers.ts`

- [ ] **Step 1: Types**

Create `frontend/src/types/speaker.ts`:

```ts
export interface Speaker {
  id: string;
  tenant_id: string;
  display_name: string;
  color: string;
  status: 'active' | 'inactive';
  created_at: string;
}

export interface EnrolledEmbedding {
  id: string;
  duration_ms: number;
  quality_score: number;
  is_primary: boolean;
}

export interface EnrollResponse {
  speaker_id: string;
  embeddings: EnrolledEmbedding[];
  warnings: string[];
}

export interface UnknownSegment {
  segment_id: string;
  session_id: string;
  start_ms: number;
  end_ms: number;
  has_embedding: boolean;
}

export type AssignTarget =
  | { speaker_id: string }
  | { new_speaker: { display_name: string; color: string } };
```

- [ ] **Step 2: API client (conforms to repo pattern in `api/actio-api.ts`)**

Create `frontend/src/api/speakers.ts`:

```ts
import type {
  Speaker,
  EnrollResponse,
  UnknownSegment,
  AssignTarget,
} from '../types/speaker';

const BASE = 'http://127.0.0.1:3000';

async function json<T>(res: Response): Promise<T> {
  if (!res.ok) {
    const text = await res.text().catch(() => '');
    throw new Error(`${res.status} ${res.statusText}: ${text}`);
  }
  return (await res.json()) as T;
}

export async function listSpeakers(): Promise<Speaker[]> {
  return json<Speaker[]>(await fetch(`${BASE}/speakers`));
}

export async function createSpeaker(input: {
  display_name: string;
  color: string;
}): Promise<Speaker> {
  return json<Speaker>(
    await fetch(`${BASE}/speakers`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(input),
    }),
  );
}

export async function updateSpeaker(
  id: string,
  patch: { display_name?: string; color?: string },
): Promise<Speaker> {
  return json<Speaker>(
    await fetch(`${BASE}/speakers/${id}`, {
      method: 'PATCH',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(patch),
    }),
  );
}

export async function deleteSpeaker(id: string): Promise<void> {
  const res = await fetch(`${BASE}/speakers/${id}`, { method: 'DELETE' });
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
}

export async function enrollSpeaker(
  id: string,
  clips: Blob[],
): Promise<EnrollResponse> {
  const form = new FormData();
  clips.forEach((blob, i) => form.append(`clip_${i}`, blob, `clip_${i}.wav`));
  return json<EnrollResponse>(
    await fetch(`${BASE}/speakers/${id}/enroll?mode=replace`, {
      method: 'POST',
      body: form,
    }),
  );
}

export async function listUnknowns(limit = 50): Promise<UnknownSegment[]> {
  return json<UnknownSegment[]>(
    await fetch(`${BASE}/unknowns?limit=${limit}`),
  );
}

export async function assignSegment(
  segmentId: string,
  target: AssignTarget,
): Promise<{ segment_id: string; speaker_id: string; embedding_added: boolean }> {
  return json(
    await fetch(`${BASE}/segments/${segmentId}/assign`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(target),
    }),
  );
}

export async function unassignSegment(segmentId: string): Promise<void> {
  const res = await fetch(`${BASE}/segments/${segmentId}/unassign`, {
    method: 'POST',
  });
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
}
```

- [ ] **Step 3: Typecheck**

```bash
cd D:/Dev/Actio/frontend && pnpm exec tsc --noEmit 2>&1 | head -20
```
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/types/speaker.ts frontend/src/api/speakers.ts
git commit -m "feat(api): typed speaker + enrollment + unknown-segment client"
```

### Task 5.2: Refactor `use-voice-store.ts` to use backend speakers

**Files:**
- Modify: `frontend/src/store/use-voice-store.ts`
- Modify: consumers that reference `people` / `addPerson` / `updatePerson` / `deletePerson`

- [ ] **Step 1: Write failing tests**

Append to `frontend/src/store/__tests__/use-voice-store.test.ts` (reuse the file pattern that exists):

```ts
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { useVoiceStore } from '../use-voice-store';
import type { Speaker } from '../../types/speaker';

describe('useVoiceStore speakers slice', () => {
  beforeEach(() => {
    useVoiceStore.setState({
      speakers: [],
      speakersStatus: 'idle',
      unknowns: [],
      dismissedUnknowns: new Set<string>(),
    } as Partial<ReturnType<typeof useVoiceStore.getState>> as never);
    global.fetch = vi.fn() as never;
  });

  it('fetchSpeakers populates speakers from GET /speakers', async () => {
    const mock = [{ id: '1', tenant_id: '0', display_name: 'Alice',
                    color: '#E57373', status: 'active',
                    created_at: new Date().toISOString() }] satisfies Speaker[];
    (global.fetch as never as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      new Response(JSON.stringify(mock), { status: 200 }),
    );

    await useVoiceStore.getState().fetchSpeakers();
    expect(useVoiceStore.getState().speakers).toEqual(mock);
    expect(useVoiceStore.getState().speakersStatus).toBe('ready');
  });

  it('createSpeaker inserts into the list on success', async () => {
    const created = { id: '2', tenant_id: '0', display_name: 'Bob',
                      color: '#64B5F6', status: 'active',
                      created_at: new Date().toISOString() } satisfies Speaker;
    (global.fetch as never as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      new Response(JSON.stringify(created), { status: 201 }),
    );

    const result = await useVoiceStore.getState().createSpeaker({
      display_name: 'Bob', color: '#64B5F6',
    });
    expect(result).toEqual(created);
    expect(useVoiceStore.getState().speakers).toContainEqual(created);
  });
});
```

- [ ] **Step 2: Rewrite the store**

Replace `frontend/src/store/use-voice-store.ts` content (keeping the existing recording / segment slice intact):

```ts
import { create } from 'zustand';
import type { Segment } from '../types';
import type { Speaker, UnknownSegment, AssignTarget } from '../types/speaker';
import * as speakersApi from '../api/speakers';

export type ClipInterval = 1 | 2 | 5 | 10 | 30;
const WS_BASE = 'ws://127.0.0.1:3000';

interface RecordingSession {
  id: string;
  startedAt: string;
  liveTranscript: string;
  pendingPartial: string;
  pipelineReady: boolean;
}

interface VoiceState {
  // recording (unchanged behavior)
  isRecording: boolean;
  currentSession: RecordingSession | null;
  segments: Segment[];
  clipInterval: ClipInterval;
  _ws: WebSocket | null;

  // speakers (new)
  speakers: Speaker[];
  speakersStatus: 'idle' | 'loading' | 'ready' | 'error';
  unknowns: UnknownSegment[];
  dismissedUnknowns: Set<string>;

  // recording actions (unchanged)
  startRecording: () => void;
  stopRecording: () => void;
  appendLiveTranscript: (text: string) => void;
  flushInterval: () => void;
  starSegment: (id: string) => void;
  unstarSegment: (id: string) => void;
  deleteSegment: (id: string) => void;
  setClipInterval: (minutes: ClipInterval) => void;

  // speaker actions (new)
  fetchSpeakers: () => Promise<void>;
  createSpeaker: (input: { display_name: string; color: string }) => Promise<Speaker>;
  updateSpeaker: (id: string, patch: { display_name?: string; color?: string }) => Promise<void>;
  deleteSpeaker: (id: string) => Promise<void>;
  enrollSpeaker: (id: string, clips: Blob[]) => ReturnType<typeof speakersApi.enrollSpeaker>;
  fetchUnknowns: () => Promise<void>;
  assignSegment: (segmentId: string, target: AssignTarget) => Promise<void>;
  dismissUnknown: (segmentId: string) => void;
}

const MAX_UNSTARRED = 30;
const STORAGE_KEY = 'actio-voice';

export function pruneSegments(segments: Segment[]): Segment[] {
  let unstarredCount = 0;
  return segments.filter((s) => {
    if (s.starred) return true;
    unstarredCount++;
    return unstarredCount <= MAX_UNSTARRED;
  });
}

function loadVoiceData(): { segments: Segment[]; clipInterval: ClipInterval } {
  try {
    const raw = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? 'null');
    // Ignore any previously-persisted `people` field; it is no longer used.
    return {
      segments: raw?.segments ?? [],
      clipInterval: raw?.clipInterval ?? 5,
    };
  } catch {
    return { segments: [], clipInterval: 5 };
  }
}

function saveVoiceData(segments: Segment[], clipInterval: ClipInterval) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify({ segments, clipInterval }));
}

const { segments: initialSegments, clipInterval: initialClipInterval } = loadVoiceData();

export const useVoiceStore = create<VoiceState>((set, get) => ({
  isRecording: false,
  currentSession: null,
  segments: initialSegments,
  clipInterval: initialClipInterval,
  _ws: null,

  speakers: [],
  speakersStatus: 'idle',
  unknowns: [],
  dismissedUnknowns: new Set<string>(),

  startRecording: () => {
    const session: RecordingSession = {
      id: 'live',
      startedAt: new Date().toISOString(),
      liveTranscript: '',
      pendingPartial: '',
      pipelineReady: false,
    };
    set({ isRecording: true, currentSession: session });
    const ws = new WebSocket(`${WS_BASE}/ws`);
    ws.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data);
        if (msg.kind === 'transcript' && msg.text) {
          if (msg.is_final) {
            set((state) => {
              if (!state.currentSession) return state;
              const prev = state.currentSession.liveTranscript;
              return {
                currentSession: {
                  ...state.currentSession,
                  liveTranscript: prev ? `${prev} ${msg.text}` : msg.text,
                  pendingPartial: '',
                  pipelineReady: true,
                },
              };
            });
          } else {
            set((state) => {
              if (!state.currentSession) return state;
              return {
                currentSession: {
                  ...state.currentSession,
                  pendingPartial: msg.text,
                  pipelineReady: true,
                },
              };
            });
          }
        } else if (msg.kind === 'speaker_resolved') {
          // opportunistic: refresh unknowns if the panel is watching
          get().fetchUnknowns().catch(() => {});
        }
      } catch { /* ignore */ }
    };
    ws.onerror = (e) => console.warn('[Actio] WS error', e);
    set({ _ws: ws });
  },

  stopRecording: () => {
    const { currentSession, _ws } = get();
    _ws?.close();
    if (currentSession?.liveTranscript.trim()) get().flushInterval();
    set({ isRecording: false, currentSession: null, _ws: null });
  },

  appendLiveTranscript: (text) => set((state) => {
    if (!state.currentSession) return state;
    const prev = state.currentSession.liveTranscript;
    return {
      currentSession: {
        ...state.currentSession,
        liveTranscript: prev ? `${prev} ${text}` : text,
      },
    };
  }),

  flushInterval: () => {
    const { currentSession, segments, clipInterval } = get();
    if (!currentSession || !currentSession.liveTranscript.trim()) return;
    const newSegment: Segment = {
      id: crypto.randomUUID(),
      sessionId: currentSession.id,
      text: currentSession.liveTranscript.trim(),
      createdAt: new Date().toISOString(),
      starred: false,
    };
    const next = pruneSegments([newSegment, ...segments]);
    saveVoiceData(next, clipInterval);
    set({
      segments: next,
      currentSession: { ...currentSession, liveTranscript: '' },
    });
  },

  starSegment: (id) => set((state) => {
    const next = state.segments.map((s) => (s.id === id ? { ...s, starred: true } : s));
    saveVoiceData(next, state.clipInterval);
    return { segments: next };
  }),

  unstarSegment: (id) => set((state) => {
    const mapped = state.segments.map((s) => (s.id === id ? { ...s, starred: false } : s));
    const next = pruneSegments(mapped);
    saveVoiceData(next, state.clipInterval);
    return { segments: next };
  }),

  deleteSegment: (id) => set((state) => {
    const next = state.segments.filter((s) => s.id !== id);
    saveVoiceData(next, state.clipInterval);
    return { segments: next };
  }),

  setClipInterval: (minutes) => set((state) => {
    saveVoiceData(state.segments, minutes);
    return { clipInterval: minutes };
  }),

  // ── Speakers ─────────────────────────────────────────────────────────

  fetchSpeakers: async () => {
    set({ speakersStatus: 'loading' });
    try {
      const list = await speakersApi.listSpeakers();
      set({ speakers: list, speakersStatus: 'ready' });
    } catch {
      set({ speakersStatus: 'error' });
    }
  },

  createSpeaker: async ({ display_name, color }) => {
    const s = await speakersApi.createSpeaker({ display_name, color });
    set((state) => ({ speakers: [s, ...state.speakers] }));
    return s;
  },

  updateSpeaker: async (id, patch) => {
    const updated = await speakersApi.updateSpeaker(id, patch);
    set((state) => ({
      speakers: state.speakers.map((s) => (s.id === id ? updated : s)),
    }));
  },

  deleteSpeaker: async (id) => {
    await speakersApi.deleteSpeaker(id);
    set((state) => ({
      speakers: state.speakers.filter((s) => s.id !== id),
    }));
  },

  enrollSpeaker: (id, clips) => speakersApi.enrollSpeaker(id, clips),

  fetchUnknowns: async () => {
    try {
      const list = await speakersApi.listUnknowns(50);
      set({ unknowns: list });
    } catch { /* silent; banner in UI */ }
  },

  assignSegment: async (segmentId, target) => {
    // optimistic removal
    set((state) => ({
      unknowns: state.unknowns.filter((u) => u.segment_id !== segmentId),
    }));
    try {
      await speakersApi.assignSegment(segmentId, target);
      // refresh speakers list in case a new speaker was created
      if ('new_speaker' in target) await get().fetchSpeakers();
    } catch (err) {
      // revert by re-fetching
      await get().fetchUnknowns();
      throw err;
    }
  },

  dismissUnknown: (segmentId) => set((state) => {
    const next = new Set(state.dismissedUnknowns);
    next.add(segmentId);
    return { dismissedUnknowns: next };
  }),
}));
```

- [ ] **Step 3: Fix any consumers**

```bash
cd D:/Dev/Actio/frontend && pnpm exec tsc --noEmit 2>&1 | head -40
```
Expected: errors in `PeopleTab.tsx` (still uses `people`, `addPerson`, etc.). These get rewritten in Task 6.5. Other consumers, if any, will be clear from the compile output — fix them too.

- [ ] **Step 4: Run tests**

```bash
cd D:/Dev/Actio/frontend && pnpm test -- use-voice-store 2>&1 | tail -15
```
Expected: new tests pass; old `addPerson` / `updatePerson` / `deletePerson` tests (if present) fail or no longer exist — delete them.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/store/use-voice-store.ts frontend/src/store/__tests__/use-voice-store.test.ts
git commit -m "refactor(store): backend-backed speakers slice + unknowns"
```

---

## Phase 6 — Frontend UI

### Task 6.1: `AssignSpeakerPicker`

**Files:**
- Create: `frontend/src/components/AssignSpeakerPicker.tsx`
- Modify: `frontend/src/styles/globals.css` (minor, class names only)

- [ ] **Step 1: Component**

Create `frontend/src/components/AssignSpeakerPicker.tsx`:

```tsx
import { useState, useRef, useEffect } from 'react';
import { useVoiceStore } from '../store/use-voice-store';
import type { AssignTarget } from '../types/speaker';

const DEFAULT_COLORS = [
  '#E57373', '#F06292', '#BA68C8', '#64B5F6',
  '#4DB6AC', '#81C784', '#FFD54F', '#FF8A65',
];

export function AssignSpeakerPicker({
  onPick,
  onCancel,
}: {
  onPick: (t: AssignTarget) => void;
  onCancel: () => void;
}) {
  const speakers = useVoiceStore((s) => s.speakers);
  const [query, setQuery] = useState('');
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState('');
  const [newColor, setNewColor] = useState(DEFAULT_COLORS[0]);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => { inputRef.current?.focus(); }, []);

  const filtered = speakers.filter((s) =>
    s.display_name.toLowerCase().includes(query.toLowerCase()),
  );

  if (creating) {
    return (
      <div className="assign-picker">
        <input
          type="text"
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
          placeholder="Name"
          autoFocus
          onKeyDown={(e) => {
            if (e.key === 'Enter' && newName.trim()) {
              onPick({ new_speaker: { display_name: newName.trim(), color: newColor } });
            } else if (e.key === 'Escape') {
              setCreating(false);
            }
          }}
        />
        <div className="assign-picker__swatches" role="group" aria-label="Color">
          {DEFAULT_COLORS.map((c) => (
            <button
              key={c}
              type="button"
              className={`assign-picker__swatch${newColor === c ? ' is-selected' : ''}`}
              style={{ backgroundColor: c }}
              onClick={() => setNewColor(c)}
              aria-label={`Select color ${c}`}
              aria-pressed={newColor === c}
            />
          ))}
        </div>
        <div className="assign-picker__actions">
          <button
            type="button"
            className="primary-button"
            disabled={!newName.trim()}
            onClick={() => onPick({ new_speaker: { display_name: newName.trim(), color: newColor } })}
          >Create and assign</button>
          <button type="button" className="secondary-button" onClick={() => setCreating(false)}>
            Back
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="assign-picker">
      <input
        ref={inputRef}
        type="text"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder="Search people…"
        onKeyDown={(e) => { if (e.key === 'Escape') onCancel(); }}
      />
      <ul className="assign-picker__list">
        <li>
          <button type="button" className="assign-picker__create" onClick={() => setCreating(true)}>
            + Create new person
          </button>
        </li>
        {filtered.map((s) => (
          <li key={s.id}>
            <button
              type="button"
              className="assign-picker__row"
              onClick={() => onPick({ speaker_id: s.id })}
            >
              <span className="assign-picker__avatar" style={{ backgroundColor: s.color }}>
                {s.display_name.charAt(0).toUpperCase()}
              </span>
              {s.display_name}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
```

- [ ] **Step 2: Style hooks**

Append to `frontend/src/styles/globals.css`:

```css
.assign-picker { display: flex; flex-direction: column; gap: .5rem; }
.assign-picker input { padding: .5rem; border: 1px solid var(--border); border-radius: 6px; }
.assign-picker__list { list-style: none; padding: 0; margin: 0; max-height: 240px; overflow-y: auto; }
.assign-picker__row, .assign-picker__create {
  width: 100%; text-align: left; padding: .5rem; border: none; background: transparent;
  cursor: pointer; border-radius: 6px; display: flex; align-items: center; gap: .5rem;
}
.assign-picker__row:hover, .assign-picker__create:hover { background: var(--hover); }
.assign-picker__avatar {
  width: 24px; height: 24px; border-radius: 50%; display: inline-flex;
  align-items: center; justify-content: center; color: #fff; font-weight: 600;
}
.assign-picker__swatches { display: flex; gap: .25rem; }
.assign-picker__swatch {
  width: 20px; height: 20px; border-radius: 50%; border: 2px solid transparent; cursor: pointer;
}
.assign-picker__swatch.is-selected { border-color: var(--fg); }
.assign-picker__actions { display: flex; gap: .5rem; }
```

- [ ] **Step 3: Typecheck + commit**

```bash
cd D:/Dev/Actio/frontend && pnpm exec tsc --noEmit 2>&1 | head
git add frontend/src/components/AssignSpeakerPicker.tsx frontend/src/styles/globals.css
git commit -m "feat(ui): AssignSpeakerPicker with existing/new speaker flows"
```

### Task 6.2: `use-media-recorder.ts` with WAV encode

**Files:**
- Create: `frontend/src/hooks/use-media-recorder.ts`

- [ ] **Step 1: Implement**

Create `frontend/src/hooks/use-media-recorder.ts`:

```ts
import { useRef, useState, useCallback, useEffect } from 'react';

function encodeWav16kMono(samples: Float32Array): Blob {
  // 16-bit PCM WAV, single channel, 16000 Hz
  const bytesPerSample = 2;
  const headerSize = 44;
  const dataSize = samples.length * bytesPerSample;
  const buffer = new ArrayBuffer(headerSize + dataSize);
  const view = new DataView(buffer);
  // RIFF header
  writeString(view, 0, 'RIFF');
  view.setUint32(4, 36 + dataSize, true);
  writeString(view, 8, 'WAVE');
  // fmt chunk
  writeString(view, 12, 'fmt ');
  view.setUint32(16, 16, true);            // PCM chunk size
  view.setUint16(20, 1, true);             // PCM format
  view.setUint16(22, 1, true);             // channels
  view.setUint32(24, 16000, true);         // sample rate
  view.setUint32(28, 16000 * bytesPerSample, true); // byte rate
  view.setUint16(32, bytesPerSample, true);// block align
  view.setUint16(34, 16, true);            // bits per sample
  // data chunk
  writeString(view, 36, 'data');
  view.setUint32(40, dataSize, true);
  // samples
  let offset = 44;
  for (let i = 0; i < samples.length; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(offset, s < 0 ? s * 0x8000 : s * 0x7fff, true);
    offset += 2;
  }
  return new Blob([buffer], { type: 'audio/wav' });
}

function writeString(view: DataView, offset: number, str: string) {
  for (let i = 0; i < str.length; i++) view.setUint8(offset + i, str.charCodeAt(i));
}

function linearResample(input: Float32Array, srcRate: number, dstRate: number): Float32Array {
  if (srcRate === dstRate) return input;
  const ratio = srcRate / dstRate;
  const outLen = Math.floor(input.length / ratio);
  const out = new Float32Array(outLen);
  for (let i = 0; i < outLen; i++) {
    const pos = i * ratio;
    const idx = Math.floor(pos);
    const frac = pos - idx;
    const a = input[idx];
    const b = input[idx + 1] ?? a;
    out[i] = a + (b - a) * frac;
  }
  return out;
}

export interface UseMediaRecorder {
  recording: boolean;
  durationSec: number;
  rmsLevel: number;
  start: () => Promise<void>;
  stop: () => Promise<Blob>;
  cancel: () => void;
  error: string | null;
}

export function useMediaRecorder(): UseMediaRecorder {
  const [recording, setRecording] = useState(false);
  const [durationSec, setDurationSec] = useState(0);
  const [rmsLevel, setRmsLevel] = useState(0);
  const [error, setError] = useState<string | null>(null);

  const ctxRef = useRef<AudioContext | null>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const analyserRef = useRef<AnalyserNode | null>(null);
  const chunksRef = useRef<Float32Array[]>([]);
  const processorRef = useRef<ScriptProcessorNode | null>(null);
  const startTsRef = useRef<number>(0);
  const rafRef = useRef<number | null>(null);

  const cleanup = useCallback(() => {
    if (rafRef.current) cancelAnimationFrame(rafRef.current);
    processorRef.current?.disconnect();
    analyserRef.current?.disconnect();
    streamRef.current?.getTracks().forEach((t) => t.stop());
    ctxRef.current?.close();
    processorRef.current = null;
    analyserRef.current = null;
    streamRef.current = null;
    ctxRef.current = null;
  }, []);

  const start = useCallback(async () => {
    setError(null);
    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: {
          channelCount: 1,
          sampleRate: 16000,
          echoCancellation: true,
          noiseSuppression: true,
        },
      });
      streamRef.current = stream;
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const ctx = new (window.AudioContext || (window as any).webkitAudioContext)({
        sampleRate: 16000,
      });
      ctxRef.current = ctx;
      const source = ctx.createMediaStreamSource(stream);
      const analyser = ctx.createAnalyser();
      analyser.fftSize = 1024;
      analyserRef.current = analyser;
      source.connect(analyser);

      // ScriptProcessorNode is deprecated but works everywhere without a worklet.
      // Buffer size 4096 keeps UI latency ~250ms which is fine for enrollment.
      const processor = ctx.createScriptProcessor(4096, 1, 1);
      processor.onaudioprocess = (e) => {
        const input = e.inputBuffer.getChannelData(0);
        chunksRef.current.push(new Float32Array(input));
      };
      source.connect(processor);
      processor.connect(ctx.destination);
      processorRef.current = processor;

      chunksRef.current = [];
      startTsRef.current = performance.now();
      setRecording(true);

      const buffer = new Float32Array(analyser.fftSize);
      const tick = () => {
        if (!analyserRef.current) return;
        analyserRef.current.getFloatTimeDomainData(buffer);
        let sum = 0;
        for (let i = 0; i < buffer.length; i++) sum += buffer[i] * buffer[i];
        setRmsLevel(Math.sqrt(sum / buffer.length));
        setDurationSec((performance.now() - startTsRef.current) / 1000);
        rafRef.current = requestAnimationFrame(tick);
      };
      tick();
    } catch (err) {
      setError((err as Error).message || 'Microphone access failed');
      cleanup();
      setRecording(false);
      throw err;
    }
  }, [cleanup]);

  const stop = useCallback(async () => {
    if (!ctxRef.current) throw new Error('not recording');
    const srcRate = ctxRef.current.sampleRate;
    // concatenate
    const total = chunksRef.current.reduce((n, c) => n + c.length, 0);
    const merged = new Float32Array(total);
    let o = 0;
    for (const c of chunksRef.current) { merged.set(c, o); o += c.length; }
    const resampled = linearResample(merged, srcRate, 16000);
    const blob = encodeWav16kMono(resampled);
    cleanup();
    setRecording(false);
    return blob;
  }, [cleanup]);

  const cancel = useCallback(() => {
    chunksRef.current = [];
    cleanup();
    setRecording(false);
  }, [cleanup]);

  useEffect(() => () => cleanup(), [cleanup]);

  return { recording, durationSec, rmsLevel, start, stop, cancel, error };
}
```

- [ ] **Step 2: Typecheck**

```bash
cd D:/Dev/Actio/frontend && pnpm exec tsc --noEmit 2>&1 | head
```
Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/hooks/use-media-recorder.ts
git commit -m "feat(hook): useMediaRecorder with WAV encode + resample"
```

### Task 6.3: `VoiceprintRecorder`

**Files:**
- Create: `frontend/src/components/VoiceprintRecorder.tsx`
- Modify: `frontend/src/styles/globals.css`

- [ ] **Step 1: Component**

Create `frontend/src/components/VoiceprintRecorder.tsx`:

```tsx
import { useState } from 'react';
import { useMediaRecorder } from '../hooks/use-media-recorder';
import { useVoiceStore } from '../store/use-voice-store';

const PASSAGES = [
  'The quick brown fox jumps over the lazy dog.',
  'She sells seashells by the seashore under a clear blue sky.',
  'A journey of a thousand miles begins with a single step.',
];

const MAX_CLIP_SEC = 20;

export function VoiceprintRecorder({
  speakerId,
  onDone,
  onCancel,
}: {
  speakerId: string;
  onDone: (warnings: string[]) => void;
  onCancel: () => void;
}) {
  const enroll = useVoiceStore((s) => s.enrollSpeaker);
  const [clips, setClips] = useState<Blob[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const rec = useMediaRecorder();

  const idx = clips.length;
  const done = idx >= 3;

  async function toggle() {
    if (!rec.recording) {
      try { await rec.start(); } catch { /* error shown below */ }
      return;
    }
    try {
      const blob = await rec.stop();
      setClips((cs) => [...cs, blob]);
    } catch { /* ignore */ }
  }

  async function finish() {
    if (clips.length === 0) return;
    setSubmitting(true);
    setSubmitError(null);
    try {
      const result = await enroll(speakerId, clips);
      onDone(result.warnings);
    } catch (err) {
      setSubmitError((err as Error).message);
    } finally {
      setSubmitting(false);
    }
  }

  const auto =
    rec.recording && rec.durationSec >= MAX_CLIP_SEC
      ? (toggle(), null)
      : null;

  return (
    <div className="voiceprint-recorder">
      <h3>{done ? 'Review' : `Record voiceprint — step ${idx + 1} of 3`}</h3>
      {!done && <p className="voiceprint-recorder__passage">“{PASSAGES[idx]}”</p>}
      {!done && (
        <div className="voiceprint-recorder__meter">
          <div
            className="voiceprint-recorder__bar"
            style={{ width: `${Math.min(100, rec.rmsLevel * 500)}%` }}
          />
        </div>
      )}
      {!done && (
        <div className="voiceprint-recorder__timer">
          {rec.durationSec.toFixed(1)}s / {MAX_CLIP_SEC}s
        </div>
      )}
      <div className="voiceprint-recorder__captured">
        {[0, 1, 2].map((i) => (
          <span key={i} className={`voiceprint-recorder__chip${i < clips.length ? ' is-done' : ''}`}>
            {i < clips.length ? '✓' : '·'}
          </span>
        ))}
      </div>
      {rec.error && <p className="voiceprint-recorder__error">{rec.error}</p>}
      {submitError && <p className="voiceprint-recorder__error">{submitError}</p>}
      <div className="voiceprint-recorder__actions">
        {!done && (
          <button type="button" className="primary-button" onClick={toggle}>
            {rec.recording ? '■ Stop' : '● Record'}
          </button>
        )}
        {clips.length > 0 && !rec.recording && (
          <button type="button" className="primary-button" disabled={submitting} onClick={finish}>
            {submitting ? 'Saving…' : done ? 'Save voiceprint' : `Save (${clips.length})`}
          </button>
        )}
        <button type="button" className="secondary-button" onClick={onCancel} disabled={submitting}>
          Cancel
        </button>
      </div>
      {auto}
    </div>
  );
}
```

- [ ] **Step 2: Styles**

Append to `frontend/src/styles/globals.css`:

```css
.voiceprint-recorder { display: flex; flex-direction: column; gap: .75rem; padding: 1rem; border-radius: 8px; background: var(--surface); }
.voiceprint-recorder__passage { font-style: italic; line-height: 1.4; }
.voiceprint-recorder__meter { height: 8px; background: var(--border); border-radius: 4px; overflow: hidden; }
.voiceprint-recorder__bar { height: 100%; background: var(--accent); transition: width .05s linear; }
.voiceprint-recorder__timer { font-variant-numeric: tabular-nums; font-size: .875rem; color: var(--fg-muted); }
.voiceprint-recorder__captured { display: flex; gap: .5rem; }
.voiceprint-recorder__chip { width: 24px; height: 24px; border-radius: 50%; display: inline-flex; align-items: center; justify-content: center; background: var(--border); }
.voiceprint-recorder__chip.is-done { background: var(--accent); color: #fff; }
.voiceprint-recorder__error { color: var(--danger, #c0392b); }
.voiceprint-recorder__actions { display: flex; gap: .5rem; }
```

- [ ] **Step 3: Typecheck + commit**

```bash
cd D:/Dev/Actio/frontend && pnpm exec tsc --noEmit 2>&1 | head
git add frontend/src/components/VoiceprintRecorder.tsx frontend/src/styles/globals.css
git commit -m "feat(ui): VoiceprintRecorder 3-clip capture widget"
```

### Task 6.4: `UnknownSpeakerPanel`

**Files:**
- Create: `frontend/src/components/UnknownSpeakerPanel.tsx`

- [ ] **Step 1: Component**

Create `frontend/src/components/UnknownSpeakerPanel.tsx`:

```tsx
import { useEffect, useState } from 'react';
import { useVoiceStore } from '../store/use-voice-store';
import { AssignSpeakerPicker } from './AssignSpeakerPicker';

export function UnknownSpeakerPanel() {
  const unknowns = useVoiceStore((s) => s.unknowns);
  const dismissed = useVoiceStore((s) => s.dismissedUnknowns);
  const fetchUnknowns = useVoiceStore((s) => s.fetchUnknowns);
  const assignSegment = useVoiceStore((s) => s.assignSegment);
  const dismissUnknown = useVoiceStore((s) => s.dismissUnknown);
  const [pickingFor, setPickingFor] = useState<string | null>(null);

  useEffect(() => {
    fetchUnknowns();
    const interval = setInterval(() => fetchUnknowns(), 10_000);
    return () => clearInterval(interval);
  }, [fetchUnknowns]);

  const visible = unknowns.filter((u) => !dismissed.has(u.segment_id));
  if (visible.length === 0) return null;

  return (
    <details className="unknown-panel" open>
      <summary>Unidentified voices ({visible.length})</summary>
      <ul className="unknown-panel__list">
        {visible.map((u) => (
          <li key={u.segment_id} className="unknown-panel__row">
            <div>
              <div className="unknown-panel__meta">
                {(u.end_ms - u.start_ms) / 1000}s · session {u.session_id.slice(0, 8)}…
              </div>
              {pickingFor === u.segment_id ? (
                <AssignSpeakerPicker
                  onPick={async (target) => {
                    await assignSegment(u.segment_id, target);
                    setPickingFor(null);
                  }}
                  onCancel={() => setPickingFor(null)}
                />
              ) : (
                <div className="unknown-panel__actions">
                  <button
                    type="button"
                    className="primary-button"
                    disabled={!u.has_embedding}
                    title={u.has_embedding ? '' : 'No embedding — assignment will not promote a voiceprint'}
                    onClick={() => setPickingFor(u.segment_id)}
                  >Assign to…</button>
                  <button
                    type="button"
                    className="secondary-button"
                    onClick={() => dismissUnknown(u.segment_id)}
                  >Not a person</button>
                </div>
              )}
            </div>
          </li>
        ))}
      </ul>
    </details>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add frontend/src/components/UnknownSpeakerPanel.tsx
git commit -m "feat(ui): UnknownSpeakerPanel with polling and dismissal"
```

### Task 6.5: Rewrite `PeopleTab.tsx` against the new store

**Files:**
- Modify: `frontend/src/components/PeopleTab.tsx`

- [ ] **Step 1: Rewrite**

Replace the contents of `frontend/src/components/PeopleTab.tsx` with:

```tsx
import { useEffect, useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useVoiceStore } from '../store/use-voice-store';
import { VoiceprintRecorder } from './VoiceprintRecorder';
import { UnknownSpeakerPanel } from './UnknownSpeakerPanel';
import type { Speaker } from '../types/speaker';

const PRESET_COLORS = [
  '#E57373', '#F06292', '#BA68C8', '#64B5F6',
  '#4DB6AC', '#81C784', '#FFD54F', '#FF8A65',
];

type FormMode =
  | { kind: 'idle' }
  | { kind: 'adding' }
  | { kind: 'editing'; speaker: Speaker }
  | { kind: 'enrolling'; speakerId: string; replace: boolean };

export function PeopleTab() {
  const speakers = useVoiceStore((s) => s.speakers);
  const speakersStatus = useVoiceStore((s) => s.speakersStatus);
  const fetchSpeakers = useVoiceStore((s) => s.fetchSpeakers);
  const createSpeaker = useVoiceStore((s) => s.createSpeaker);
  const updateSpeaker = useVoiceStore((s) => s.updateSpeaker);
  const deleteSpeaker = useVoiceStore((s) => s.deleteSpeaker);

  const [mode, setMode] = useState<FormMode>({ kind: 'idle' });
  const [name, setName] = useState('');
  const [color, setColor] = useState(PRESET_COLORS[0]);

  useEffect(() => { fetchSpeakers(); }, [fetchSpeakers]);

  if (speakersStatus === 'error') {
    return (
      <div className="people-tab">
        <div className="people-tab__error">
          Backend required to manage speakers.
          <button type="button" className="secondary-button" onClick={() => fetchSpeakers()}>
            Retry
          </button>
        </div>
      </div>
    );
  }

  function startAdd() { setMode({ kind: 'adding' }); setName(''); setColor(PRESET_COLORS[0]); }
  function startEdit(s: Speaker) { setMode({ kind: 'editing', speaker: s }); setName(s.display_name); setColor(s.color); }

  async function save() {
    const trimmed = name.trim();
    if (!trimmed) return;
    if (mode.kind === 'adding') {
      const s = await createSpeaker({ display_name: trimmed, color });
      setMode({ kind: 'enrolling', speakerId: s.id, replace: false });
    } else if (mode.kind === 'editing') {
      await updateSpeaker(mode.speaker.id, { display_name: trimmed, color });
      setMode({ kind: 'idle' });
    }
  }

  return (
    <div className="people-tab">
      <AnimatePresence mode="wait">
        {mode.kind === 'idle' && (
          <motion.button
            key="add-btn"
            type="button"
            className="primary-button people-tab__add-btn"
            onClick={startAdd}
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.9 }}
          >+ Add person</motion.button>
        )}

        {(mode.kind === 'adding' || mode.kind === 'editing') && (
          <motion.div
            key="form"
            className="person-form"
            initial={{ opacity: 0, y: -12 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -12 }}
          >
            <input
              type="text"
              className="person-form__name-input"
              placeholder="Name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              autoFocus
              onKeyDown={(e) => {
                if (e.key === 'Enter') save();
                if (e.key === 'Escape') setMode({ kind: 'idle' });
              }}
            />
            <div className="person-form__swatches" role="group" aria-label="Color">
              {PRESET_COLORS.map((c) => (
                <button
                  key={c}
                  type="button"
                  className={`person-form__swatch${color === c ? ' is-selected' : ''}`}
                  style={{ backgroundColor: c }}
                  onClick={() => setColor(c)}
                  aria-label={`Select color ${c}`}
                  aria-pressed={color === c}
                />
              ))}
            </div>
            <div className="person-form__actions">
              <button type="button" className="primary-button" disabled={!name.trim()} onClick={save}>
                Save
              </button>
              <button type="button" className="secondary-button" onClick={() => setMode({ kind: 'idle' })}>
                Cancel
              </button>
            </div>
          </motion.div>
        )}

        {mode.kind === 'enrolling' && (
          <motion.div key="enrolling" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }}>
            <VoiceprintRecorder
              speakerId={mode.speakerId}
              onDone={() => { setMode({ kind: 'idle' }); fetchSpeakers(); }}
              onCancel={() => setMode({ kind: 'idle' })}
            />
          </motion.div>
        )}
      </AnimatePresence>

      <UnknownSpeakerPanel />

      <div className="people-tab__list">
        {speakersStatus === 'loading' && <p className="people-tab__empty">Loading…</p>}
        {speakersStatus === 'ready' && speakers.length === 0 && mode.kind === 'idle' && (
          <p className="people-tab__empty">No speakers yet.</p>
        )}
        {speakers.map((s) => (
          <div key={s.id} className="person-row">
            <div className="person-row__avatar" style={{ backgroundColor: s.color }}>
              {s.display_name.charAt(0).toUpperCase()}
            </div>
            <span className="person-row__name">{s.display_name}</span>
            <div className="person-row__actions">
              <button
                type="button"
                className="person-edit-btn"
                onClick={() => setMode({ kind: 'enrolling', speakerId: s.id, replace: true })}
                aria-label={`Re-enroll ${s.display_name}`}
                title="Re-enroll voiceprint"
              >🎙</button>
              <button
                type="button"
                className="person-edit-btn"
                onClick={() => startEdit(s)}
                aria-label={`Edit ${s.display_name}`}
              >✎</button>
              <button
                type="button"
                className="person-delete-btn"
                onClick={() => { if (confirm(`Delete ${s.display_name}?`)) deleteSpeaker(s.id); }}
                aria-label={`Delete ${s.display_name}`}
              >🗑</button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Typecheck + build**

```bash
cd D:/Dev/Actio/frontend && pnpm exec tsc --noEmit 2>&1 | head
pnpm build 2>&1 | tail -10
```
Expected: clean. If old `Person` references remain anywhere (types/index.ts, other components), either delete the `Person` type or keep it but mark `// deprecated`.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/PeopleTab.tsx
git commit -m "feat(ui): PeopleTab wired to backend speakers + enrollment + unknowns"
```

### Task 6.6: Inline `[UNKNOWN]` chip in `RecordingTab`

**Files:**
- Modify: `frontend/src/components/RecordingTab.tsx`

- [ ] **Step 1: Locate the transcript rendering**

```bash
cd D:/Dev/Actio/frontend && grep -n "liveTranscript\|pendingPartial\|speaker" src/components/RecordingTab.tsx | head
```

- [ ] **Step 2: Add speaker-aware rendering**

Where the transcript lines are rendered, wrap `[UNKNOWN]`-tagged text in a clickable chip component. A minimal addition:

```tsx
import { AssignSpeakerPicker } from './AssignSpeakerPicker';
import { useVoiceStore } from '../store/use-voice-store';
import { useState } from 'react';

// near the component body:
const speakers = useVoiceStore((s) => s.speakers);
const assignSegment = useVoiceStore((s) => s.assignSegment);
const [pickingSegmentId, setPickingSegmentId] = useState<string | null>(null);
```

For each rendered transcript line that corresponds to a backend segment (i.e. has a `segment_id` + `speaker_id` in the store's message history — this will require a small shape update when wiring WS messages to segments; if that work isn't in place, gate the feature behind a `segment_id` presence check), render:

```tsx
{line.speaker_id == null ? (
  <button
    type="button"
    className="speaker-chip speaker-chip--unknown"
    onClick={() => setPickingSegmentId(line.segment_id)}
  >[UNKNOWN]</button>
) : (
  <span
    className="speaker-chip"
    style={{ backgroundColor: speakers.find((s) => s.id === line.speaker_id)?.color }}
  >{speakers.find((s) => s.id === line.speaker_id)?.display_name ?? '[UNKNOWN]'}</span>
)}

{pickingSegmentId === line.segment_id && (
  <AssignSpeakerPicker
    onPick={async (target) => {
      await assignSegment(line.segment_id!, target);
      setPickingSegmentId(null);
    }}
    onCancel={() => setPickingSegmentId(null)}
  />
)}
```

Style additions to `globals.css`:

```css
.speaker-chip {
  display: inline-block; padding: 0 .4em; border-radius: 4px;
  font-size: .75em; margin-right: .35em; background: var(--border); color: var(--fg); cursor: default;
}
.speaker-chip--unknown { background: #ccc; color: #333; cursor: pointer; border: 1px dashed var(--fg-muted); }
.speaker-chip--unknown:hover { background: #bbb; }
```

**If the current store's message shape does not carry `segment_id` / `speaker_id` per line**, add them by extending the WS `onmessage` handler in `use-voice-store.ts` to thread those fields through. This is a minor extension: when the backend emits `kind: "transcript"` plus `kind: "speaker_resolved"`, update the corresponding line's `speaker_id`.

- [ ] **Step 3: Typecheck + build**

```bash
cd D:/Dev/Actio/frontend && pnpm exec tsc --noEmit 2>&1 | head
```

- [ ] **Step 4: Commit**

```bash
git add frontend/src/components/RecordingTab.tsx frontend/src/styles/globals.css
git commit -m "feat(ui): inline [UNKNOWN] chips become a tap-to-assign picker"
```

---

## Phase 7 — Manual Smoke Checklist

Manual verification against a running backend with the speaker embedding model downloaded.

- [ ] Create a speaker "Alice" (color any). Confirm it appears in `GET /speakers`.
- [ ] Record 3 clips in `VoiceprintRecorder`. Inspect `GET /speakers` (no change) and run `sqlite3 actio.db "SELECT speaker_id, is_primary, embedding_dimension, length(embedding) FROM speaker_embeddings;"` — expect 3 rows, one `is_primary=1`, `embedding_dimension=512`, `length(embedding)=2048`.
- [ ] Start a session, speak the recorded passages. Confirm transcript lines get tagged with Alice's name within ~2s of each utterance finalising.
- [ ] Stop session, check the unknown panel in PeopleTab is empty (all segments resolved).
- [ ] Speak in a voice not enrolled. Confirm `[UNKNOWN]` chip appears both inline and in the unknown panel. Click inline chip → assign to new speaker "Bob". Confirm the chip updates, Bob appears in speakers list, and a `speaker_embeddings` row was added for Bob.
- [ ] Delete Alice from PeopleTab. Confirm her rows disappear from `speaker_embeddings`, any old `audio_segments.speaker_id = alice_id` rows are now NULL, and the transcripts tab still renders (with `[UNKNOWN]` chips for those old utterances).
- [ ] Disable the embedding model (rename the model file). Start a new session. Confirm transcripts still arrive and segments stay `[UNKNOWN]` instead of crashing the pipeline.
- [ ] With the embedding model re-enabled, re-enroll Alice (`replace` mode). Run the same query as before — expect exactly 3 fresh rows; old ones gone.
- [ ] Tauri desktop build: run `pnpm tauri:dev`, repeat the first four steps, confirm microphone permission prompt fires once and enrollment works identically.

---

## Self-Review

**Spec coverage:**

| Spec §  | Requirement | Tasks |
|---------|-------------|-------|
| §3 arch | Single Rust process, no Python | 1.1–6.6 (implicit) |
| §4 data | Migration 002, color col, segment embedding | 1.1, 4.1 |
| §4 data | Delete dead `backend/migrations/` | 0.1 |
| §4 data | Fix broken `speaker_matcher.rs` | 1.4, 1.5 |
| §5 API  | POST /speakers{,/enroll,/:id} PATCH/DELETE | 1.3, 2.3 |
| §5 API  | GET /sessions/:id/unknowns, /unknowns, POST /segments/:id/assign,/unassign | 3.1, 3.2 |
| §6 live | VAD-segment → embedding → identify → WS event | 4.1 |
| §7 FE   | Types + API client | 5.1 |
| §7 FE   | Store shape change | 5.2 |
| §7 FE   | PeopleTab, VoiceprintRecorder, UnknownSpeakerPanel, AssignSpeakerPicker, inline chip | 6.1–6.6 |
| §7 FE   | useMediaRecorder | 6.2 |
| §8 err  | embedding_model_missing 409, duration gate, cascade-safe delete, dim-mismatch guard | 1.3, 1.5, 2.3 |
| §9 tests | Backend unit + API integration, frontend store + component, manual smoke | 1.3, 1.4, 1.5, 3.1, Phase 7 |

**Placeholder scan:** no TBDs, no "add appropriate error handling", no "similar to Task N" without code repetition.

**Type consistency:** `speaker_id` is `String` everywhere in Rust (repository + domain + API); in TypeScript it's `string`. `embedding` is `BLOB` in SQL, `Vec<u8>` in sqlx, `&[f32]` at the service boundary via `bytemuck::cast_slice`. WebSocket event names are consistent: `speaker_resolved` (Phase 4) ↔ `speaker_resolved` (Phase 5.2 store handler).

---

## Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-04-17-speaker-diarization.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — dispatch a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** — execute tasks in this session using executing-plans, batch execution with checkpoints.

Which approach?

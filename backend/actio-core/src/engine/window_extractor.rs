//! Background windowed action-item extractor.
//!
//! Replaces the legacy "end-of-session" extraction path for the always-on
//! pipeline. Every `extraction_tick_secs`, for every currently-active session
//! (`audio_sessions.ended_at IS NULL`), this module:
//!
//!   1. Enumerates rolling windows over the finalized transcript timeline
//!      (length = `window_length_ms`, step = `window_step_ms`) and upserts
//!      `extraction_windows` rows with `status='pending'`.
//!   2. Claims the next pending window atomically.
//!   3. Joins the in-window transcripts to their speaker segments and formats
//!      an attributed input for the LLM (`[HH:MM:SS • Name]: text`).
//!   4. Calls `LlmRouter::generate_action_items_with_refs`.
//!   5. Gates each returned item by `confidence`:
//!        * `"high"` → `status='open'` (lands on the Board)
//!        * `"medium"` → `status='pending'` (Needs-review queue)
//!        * anything else → dropped with a trace log
//!   6. Batch-inserts the survivors with `source_window_id` = the window we
//!      just processed, marks the window succeeded / empty / failed.
//!
//! The scheduler runs one window at a time process-wide (enforced by the
//! atomic `claim_next_pending` + the status check inside the transaction)
//! so we don't hammer the LLM backend.

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::domain::types::NewReminder;
use crate::engine::llm_router::LlmRouterError;
use crate::engine::remote_llm_client::LlmActionItem;
use crate::repository::{extraction_window as window_repo, reminder as reminder_repo};
use crate::AppState;

/// Safety margin at the live tail. We don't schedule a window whose end_ms
/// is within this distance of the last finalized transcript — late finals
/// for segments already in the window would be skipped otherwise.
pub const SAFETY_MARGIN_MS: i64 = 30_000;
/// Transcripts shorter than this (summed across the window) are classified
/// `status='empty'` without calling the LLM. Tuned so single-word wake
/// sounds or noise don't waste an LLM call.
pub const MIN_EXTRACTABLE_CHARS: usize = 60;
/// After this many claim→failed cycles, give up on a window.
pub const MAX_WINDOW_ATTEMPTS: i64 = 3;
pub const MAX_WINDOWS_PER_TICK: usize = 5;

pub async fn run_extraction_loop(state: AppState) {
    // Startup housekeeping: windows left in `running` when the process last
    // exited will never finish unless we revert them.
    match window_repo::requeue_stale_running(&state.pool).await {
        Ok(0) => {}
        Ok(n) => info!(count = n, "Requeued stale 'running' extraction windows"),
        Err(e) => warn!(error = %e, "Failed to requeue stale extraction windows"),
    }

    loop {
        let tick_secs = state
            .settings_manager
            .get()
            .await
            .audio
            .extraction_tick_secs
            .max(10) as u64;
        let sleep = Duration::from_secs(tick_secs);

        if let Err(e) = tick_once(&state).await {
            warn!(error = %e, "Extraction tick failed — will retry next interval");
        }

        tokio::time::sleep(sleep).await;
    }
}

async fn tick_once(state: &AppState) -> anyhow::Result<()> {
    let settings = state.settings_manager.get().await;
    let length_ms = settings.audio.window_length_ms as i64;
    let step_ms = settings.audio.window_step_ms as i64;

    schedule_windows_for_active_sessions(&state.pool, length_ms, step_ms).await?;
    // Drain a bounded backlog per tick;
    // bounded batches avoid unbounded LLM bursts.
    for _ in 0..MAX_WINDOWS_PER_TICK {
        if !process_next_window(state).await? {
            break;
        }
    }
    Ok(())
}

/// Walk every active session and insert any not-yet-scheduled windows
/// whose end-of-window is older than `latest_final_end_ms - SAFETY_MARGIN_MS`.
pub async fn schedule_windows_for_active_sessions(
    pool: &SqlitePool,
    length_ms: i64,
    step_ms: i64,
) -> anyhow::Result<()> {
    if length_ms <= 0 || step_ms <= 0 || step_ms > length_ms {
        return Ok(());
    }
    let sessions: Vec<(String,)> =
        sqlx::query_as("SELECT id FROM audio_sessions WHERE ended_at IS NULL")
            .fetch_all(pool)
            .await?;

    for (sid_str,) in sessions {
        let Ok(session_id) = Uuid::parse_str(&sid_str) else {
            continue;
        };
        let Some(latest) = latest_final_end_ms(pool, session_id).await? else {
            continue; // no final transcripts yet
        };
        let cutoff = latest - SAFETY_MARGIN_MS;
        if cutoff < length_ms {
            continue;
        }

        // Enumerate candidate window starts: 0, step, 2*step, …
        let mut start = 0i64;
        let mut enqueued = 0usize;
        while start + length_ms <= cutoff {
            let end = start + length_ms;
            let created = window_repo::upsert_pending_window(pool, session_id, start, end).await?;
            if created {
                enqueued += 1;
            }
            start += step_ms;
        }
        if enqueued > 0 {
            debug!(
                session = %session_id,
                count = enqueued,
                latest_ms = latest,
                "Enqueued extraction windows"
            );
        }
    }
    Ok(())
}

/// Schedule every window that can intersect finalized transcript for an ended
/// session. Unlike active scheduling, this includes the short tail window even
/// when the session ended before a full window elapsed.
pub async fn schedule_final_windows_for_session(
    pool: &SqlitePool,
    session_id: Uuid,
    length_ms: i64,
    step_ms: i64,
) -> anyhow::Result<usize> {
    if length_ms <= 0 || step_ms <= 0 || step_ms > length_ms {
        return Ok(0);
    }
    let Some(latest) = latest_final_end_ms(pool, session_id).await? else {
        return Ok(0);
    };
    if latest <= 0 {
        return Ok(0);
    }

    let mut start = 0i64;
    let mut enqueued = 0usize;
    while start < latest {
        let end = start + length_ms;
        if window_repo::upsert_pending_window(pool, session_id, start, end).await? {
            enqueued += 1;
        }
        start += step_ms;
    }
    Ok(enqueued)
}

async fn latest_final_end_ms(pool: &SqlitePool, session_id: Uuid) -> anyhow::Result<Option<i64>> {
    let row: Option<(Option<i64>,)> = sqlx::query_as(
        "SELECT MAX(end_ms) FROM transcripts WHERE session_id = ?1 AND is_final = 1",
    )
    .bind(session_id.to_string())
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|(v,)| v))
}

async fn process_next_window(state: &AppState) -> anyhow::Result<bool> {
    if state.router.read().await.is_disabled() {
        return Ok(false);
    }

    let Some(window) = window_repo::claim_next_pending(&state.pool).await? else {
        return Ok(false);
    };

    if window.attempts > MAX_WINDOW_ATTEMPTS {
        warn!(
            window_id = %window.id,
            attempts = window.attempts,
            "Window exceeded max attempts — marking failed"
        );
        window_repo::mark_failed(&state.pool, window.id, "max attempts exceeded").await?;
        return Ok(true);
    }

    info!(
        window_id = %window.id,
        session = %window.session_id,
        start_ms = window.start_ms,
        end_ms = window.end_ms,
        "Processing extraction window"
    );

    match process_window(state, &window).await {
        Ok(ProcessOutcome::Produced(n)) => {
            info!(
                window_id = %window.id,
                reminders = n,
                "Window produced reminders"
            );
            window_repo::mark_succeeded(&state.pool, window.id).await?;
        }
        Ok(ProcessOutcome::Empty) => {
            debug!(window_id = %window.id, "Window empty — no extractable content");
            window_repo::mark_empty(&state.pool, window.id).await?;
        }
        Err(ProcessError::LlmDisabled) => {
            // Not an error — the user hasn't configured an LLM yet. Revert
            // to pending so a later tick can retry if the router changes.
            window_repo::revert_to_pending(&state.pool, window.id, "llm disabled").await?;
        }
        Err(ProcessError::Transient(msg)) => {
            warn!(window_id = %window.id, error = %msg, "Transient window failure — requeued");
            window_repo::revert_to_pending(&state.pool, window.id, &msg).await?;
        }
        Err(ProcessError::Permanent(msg)) => {
            warn!(window_id = %window.id, error = %msg, "Window failed");
            window_repo::mark_failed(&state.pool, window.id, &msg).await?;
        }
    }
    Ok(true)
}

enum ProcessOutcome {
    Produced(usize),
    Empty,
}

#[derive(Debug)]
enum ProcessError {
    LlmDisabled,
    Transient(String),
    Permanent(String),
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessError::LlmDisabled => write!(f, "llm disabled"),
            ProcessError::Transient(s) | ProcessError::Permanent(s) => write!(f, "{s}"),
        }
    }
}

async fn process_window(
    state: &AppState,
    window: &window_repo::ExtractionWindow,
) -> Result<ProcessOutcome, ProcessError> {
    let _guard = state.llm_inflight.lock().await;
    let router = state.router.read().await;
    process_window_with(&state.pool, &router, window).await
}

/// Inner driver split out so integration tests can run the full claim →
/// fetch → route → gate → persist pipeline against a stub `LlmRouter`
/// without constructing a full `AppState`.
async fn process_window_with(
    pool: &SqlitePool,
    router: &crate::engine::llm_router::LlmRouter,
    window: &window_repo::ExtractionWindow,
) -> Result<ProcessOutcome, ProcessError> {
    // Fetch the session so we can turn ms offsets into wall-clock times.
    let session_started_at = fetch_session_started_at(pool, window.session_id)
        .await
        .map_err(|e| ProcessError::Permanent(format!("session lookup: {e}")))?;

    let lines = fetch_attributed_lines(
        pool,
        window.session_id,
        window.start_ms,
        window.end_ms,
    )
    .await
    .map_err(|e| ProcessError::Permanent(format!("transcript fetch: {e}")))?;

    let total_chars: usize = lines.iter().map(|l| l.text.len()).sum();
    if total_chars < MIN_EXTRACTABLE_CHARS {
        return Ok(ProcessOutcome::Empty);
    }

    let attributed = format_attributed_transcript(&lines);
    let window_local_date = chrono::Local::now().format("%Y-%m-%d %A").to_string();

    let labels =
        crate::repository::label::list_labels(pool, resolve_tenant(&session_started_at))
            .await
            .unwrap_or_default();
    let label_names: Vec<String> = labels.iter().map(|l| l.name.clone()).collect();
    let label_lookup: HashMap<String, Uuid> = labels
        .iter()
        .filter_map(|l| {
            Uuid::parse_str(&l.id)
                .ok()
                .map(|id| (l.name.to_lowercase(), id))
        })
        .collect();

    let items = match router
        .generate_action_items_with_refs(&attributed, &label_names, &window_local_date)
        .await
    {
        Ok(items) => items,
        Err(LlmRouterError::Disabled) => return Err(ProcessError::LlmDisabled),
        Err(e) => return Err(ProcessError::Transient(format!("llm error: {e}"))),
    };

    if items.is_empty() {
        return Ok(ProcessOutcome::Empty);
    }

    let tenant_id = session_started_at.tenant_id;
    let new_reminders: Vec<(NewReminder, Vec<Uuid>)> = items
        .into_iter()
        .filter_map(|item| {
            gate_action_item(
                item,
                &lines,
                tenant_id,
                window,
                &session_started_at.started_at,
                &label_lookup,
            )
        })
        .collect();

    if new_reminders.is_empty() {
        return Ok(ProcessOutcome::Empty);
    }

    let inserted = reminder_repo::create_reminders_batch_with_labels(pool, &new_reminders)
        .await
        .map_err(|e| ProcessError::Permanent(format!("insert reminders: {e}")))?;
    Ok(ProcessOutcome::Produced(inserted.len()))
}

struct SessionStub {
    tenant_id: Uuid,
    started_at: DateTime<Utc>,
}

fn resolve_tenant(stub: &SessionStub) -> Uuid {
    stub.tenant_id
}

async fn fetch_session_started_at(
    pool: &SqlitePool,
    session_id: Uuid,
) -> Result<SessionStub, sqlx::Error> {
    let row: (String, DateTime<Utc>) =
        sqlx::query_as("SELECT tenant_id, started_at FROM audio_sessions WHERE id = ?1")
            .bind(session_id.to_string())
            .fetch_one(pool)
            .await?;
    let tenant_id = Uuid::parse_str(&row.0).unwrap_or_default();
    Ok(SessionStub {
        tenant_id,
        started_at: row.1,
    })
}

/// One finalized transcript line within the window, pre-joined to its
/// speaker record (when a segment + speaker exists).
#[derive(Debug, Clone)]
pub struct AttributedLine {
    pub text: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker_id: Option<Uuid>,
    pub speaker_name: String,
}

async fn fetch_attributed_lines(
    pool: &SqlitePool,
    session_id: Uuid,
    start_ms: i64,
    end_ms: i64,
) -> Result<Vec<AttributedLine>, sqlx::Error> {
    // Overlap test: transcript in-window if its [start_ms, end_ms] intersects
    // [window_start, window_end]. Left-join to segments → speakers so lines
    // without a resolved speaker still appear with `speaker_name='Unknown'`.
    let rows: Vec<(String, i64, i64, Option<String>, Option<String>)> = sqlx::query_as(
        r#"SELECT t.text, t.start_ms, t.end_ms,
                  COALESCE(direct.speaker_id, overlap.speaker_id) AS speaker_id,
                  sp.display_name
           FROM transcripts t
           LEFT JOIN audio_segments direct ON direct.id = t.segment_id
           LEFT JOIN audio_segments overlap ON overlap.id = (
               SELECT os.id
               FROM audio_segments os
               WHERE t.segment_id IS NULL
                 AND os.session_id = t.session_id
                 AND os.start_ms < t.end_ms
                 AND os.end_ms > t.start_ms
               ORDER BY os.start_ms
               LIMIT 1
           )
           LEFT JOIN speakers sp ON sp.id = COALESCE(direct.speaker_id, overlap.speaker_id)
           WHERE t.session_id = ?1
             AND t.is_final = 1
             AND t.start_ms < ?3
             AND t.end_ms   > ?2
           ORDER BY t.start_ms"#,
    )
    .bind(session_id.to_string())
    .bind(start_ms)
    .bind(end_ms)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(text, t_start, t_end, speaker_id_str, speaker_name)| AttributedLine {
                text,
                start_ms: t_start,
                end_ms: t_end,
                speaker_id: speaker_id_str
                    .as_deref()
                    .and_then(|s| Uuid::parse_str(s).ok()),
                speaker_name: speaker_name.unwrap_or_else(|| "Unknown".to_string()),
            },
        )
        .collect())
}

/// Format a batch of attributed lines as the LLM input per `WINDOW_SYSTEM_PROMPT`.
pub fn format_attributed_transcript(lines: &[AttributedLine]) -> String {
    lines
        .iter()
        .map(|l| {
            format!(
                "[{} • {}]: {}",
                format_ts(l.start_ms),
                l.speaker_name,
                l.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_ts(ms: i64) -> String {
    let total_secs = ms / 1000;
    let h = total_secs / 3600;
    let m = (total_secs / 60) % 60;
    let s = total_secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

/// Apply confidence gating + attribution resolution to one LLM item. Returns
/// `None` when the item is dropped (low confidence, missing quote).
fn gate_action_item(
    item: LlmActionItem,
    lines: &[AttributedLine],
    tenant_id: Uuid,
    window: &window_repo::ExtractionWindow,
    session_started_at: &DateTime<Utc>,
    label_lookup: &HashMap<String, Uuid>,
) -> Option<(NewReminder, Vec<Uuid>)> {
    let confidence = item.confidence.as_deref()?;
    let status = match confidence {
        "high" => "open",
        "medium" => "pending",
        // "low" or unknown → drop. We'd rather miss a card than bury the
        // board in noise.
        _ => return None,
    };
    let evidence = item.evidence_quote.clone()?;

    // Match the quote back to a line for speaker_id + source_time.
    let evidence_lower = evidence.to_lowercase();
    let origin_line = lines
        .iter()
        .find(|l| l.text.to_lowercase().contains(&evidence_lower));

    let speaker_id = origin_line.and_then(|l| l.speaker_id).or_else(|| {
        // Fall back to matching speaker_name from the LLM output against the
        // lines we sent it (case-insensitive exact match).
        item.speaker_name
            .as_deref()
            .filter(|n| !n.eq_ignore_ascii_case("unknown"))
            .and_then(|target| {
                lines
                    .iter()
                    .find(|l| l.speaker_name.eq_ignore_ascii_case(target))
                    .and_then(|l| l.speaker_id)
            })
    });

    let source_time = origin_line
        .map(|l| session_started_at.checked_add_signed(chrono::Duration::milliseconds(l.start_ms)))
        .flatten()
        .or_else(|| {
            session_started_at.checked_add_signed(chrono::Duration::milliseconds(window.start_ms))
        });

    let due_time = item.due_time.as_deref().and_then(parse_due_time_to_utc);

    // Trim the excerpt so it fits in a card without blowing up the UI.
    let transcript_excerpt = Some(truncate_excerpt(&evidence, 280));

    let label_ids = item
        .labels
        .iter()
        .filter_map(|label| label_lookup.get(&label.to_lowercase()).copied())
        .collect();

    Some((
        NewReminder {
            session_id: Some(window.session_id),
            tenant_id,
            speaker_id,
            assigned_to: None,
            title: item.title,
            description: item.description,
            priority: item.priority,
            due_time,
            transcript_excerpt,
            context: None,
            source_time,
            status: Some(status.to_string()),
            source_window_id: Some(window.id),
        },
        label_ids,
    ))
}

fn truncate_excerpt(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut cut = max;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &s[..cut])
}

fn parse_due_time_to_utc(s: &str) -> Option<DateTime<Utc>> {
    // The remote client's validate_and_fix normalizes to "YYYY-MM-DDTHH:MM"
    // in the local timezone. Turn it into a UTC DateTime for storage.
    let naive = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .ok()?;
    let local = chrono::Local.from_local_datetime(&naive).earliest()?;
    Some(local.with_timezone(&Utc))
}

// `chrono::Local::from_local_datetime` lives on the `TimeZone` trait.
use chrono::TimeZone;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::db::run_migrations;
    use chrono::SecondsFormat;
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

    async fn mk_session(pool: &SqlitePool) -> Uuid {
        let sid = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO audio_sessions (id, tenant_id, source_type, mode, routing_policy)
               VALUES (?1, '00000000-0000-0000-0000-000000000000', 'microphone', 'realtime', 'default')"#,
        )
        .bind(sid.to_string())
        .execute(pool)
        .await
        .unwrap();
        sid
    }

    async fn mk_final_transcript(
        pool: &SqlitePool,
        session_id: Uuid,
        text: &str,
        start_ms: i64,
        end_ms: i64,
    ) {
        sqlx::query(
            "INSERT INTO transcripts (id, session_id, start_ms, end_ms, text, is_final) VALUES (?1, ?2, ?3, ?4, ?5, 1)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(session_id.to_string())
        .bind(start_ms)
        .bind(end_ms)
        .bind(text)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn mk_speaker(pool: &SqlitePool, name: &str) -> Uuid {
        let speaker_id = Uuid::new_v4();
        sqlx::query("INSERT INTO speakers (id, tenant_id, display_name) VALUES (?1, ?2, ?3)")
            .bind(speaker_id.to_string())
            .bind(Uuid::nil().to_string())
            .bind(name)
            .execute(pool)
            .await
            .unwrap();
        speaker_id
    }

    async fn mk_segment(
        pool: &SqlitePool,
        session_id: Uuid,
        speaker_id: Uuid,
        start_ms: i64,
        end_ms: i64,
    ) -> Uuid {
        let segment_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO audio_segments (id, session_id, start_ms, end_ms, speaker_id) VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(segment_id.to_string())
        .bind(session_id.to_string())
        .bind(start_ms)
        .bind(end_ms)
        .bind(speaker_id.to_string())
        .execute(pool)
        .await
        .unwrap();
        segment_id
    }

    async fn mk_final_transcript_for_segment(
        pool: &SqlitePool,
        session_id: Uuid,
        segment_id: Uuid,
        text: &str,
        start_ms: i64,
        end_ms: i64,
    ) {
        sqlx::query(
            "INSERT INTO transcripts (id, session_id, segment_id, start_ms, end_ms, text, is_final) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(session_id.to_string())
        .bind(segment_id.to_string())
        .bind(start_ms)
        .bind(end_ms)
        .bind(text)
        .execute(pool)
        .await
        .unwrap();
    }

    #[test]
    fn format_ts_handles_hours() {
        assert_eq!(format_ts(0), "00:00:00");
        assert_eq!(format_ts(65_000), "00:01:05");
        assert_eq!(format_ts(3_725_000), "01:02:05");
    }

    #[test]
    fn truncate_excerpt_respects_char_boundaries() {
        let s = "héllo world — long excerpt";
        let out = truncate_excerpt(s, 10);
        assert!(out.ends_with('…'));
        assert!(out.len() <= 15); // some slack for the ellipsis + multibyte char
    }

    #[test]
    fn gate_drops_low_and_no_quote() {
        let window = window_repo::ExtractionWindow {
            id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            start_ms: 0,
            end_ms: 300_000,
            status: "running".into(),
            attempts: 1,
            last_error: None,
            created_at: Utc::now(),
            finished_at: None,
        };
        let start = Utc::now();

        let low = LlmActionItem {
            title: None,
            description: "x".into(),
            priority: None,
            due_time: None,
            labels: vec![],
            confidence: Some("low".into()),
            evidence_quote: Some("x".into()),
            speaker_name: None,
        };
        assert!(
            gate_action_item(low, &[], Uuid::nil(), &window, &start, &HashMap::new()).is_none()
        );

        let missing_quote = LlmActionItem {
            title: None,
            description: "x".into(),
            priority: None,
            due_time: None,
            labels: vec![],
            confidence: Some("high".into()),
            evidence_quote: None,
            speaker_name: None,
        };
        assert!(gate_action_item(
            missing_quote,
            &[],
            Uuid::nil(),
            &window,
            &start,
            &HashMap::new()
        )
        .is_none());
    }

    #[test]
    fn gate_high_becomes_open_medium_becomes_pending() {
        let window = window_repo::ExtractionWindow {
            id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            start_ms: 0,
            end_ms: 300_000,
            status: "running".into(),
            attempts: 1,
            last_error: None,
            created_at: Utc::now(),
            finished_at: None,
        };
        let start = Utc::now();
        let lines = vec![AttributedLine {
            text: "remind me to email Bob tomorrow at 9".into(),
            start_ms: 10_000,
            end_ms: 13_000,
            speaker_id: None,
            speaker_name: "Alice".into(),
        }];

        let high = LlmActionItem {
            title: Some("Email Bob".into()),
            description: "Email Bob tomorrow morning".into(),
            priority: Some("medium".into()),
            due_time: None,
            labels: vec![],
            confidence: Some("high".into()),
            evidence_quote: Some("remind me to email Bob tomorrow at 9".into()),
            speaker_name: Some("Alice".into()),
        };
        let (r, _) =
            gate_action_item(high, &lines, Uuid::nil(), &window, &start, &HashMap::new()).unwrap();
        assert_eq!(r.status.as_deref(), Some("open"));
        assert_eq!(r.source_window_id, Some(window.id));
        assert!(r.transcript_excerpt.is_some());

        let medium = LlmActionItem {
            title: Some("Review budget".into()),
            description: "Review budget soon".into(),
            priority: Some("medium".into()),
            due_time: None,
            labels: vec![],
            confidence: Some("medium".into()),
            evidence_quote: Some("remind me to email Bob tomorrow at 9".into()),
            speaker_name: Some("Alice".into()),
        };
        let (r, _) = gate_action_item(
            medium,
            &lines,
            Uuid::nil(),
            &window,
            &start,
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(r.status.as_deref(), Some("pending"));
    }

    #[tokio::test]
    async fn schedule_windows_emits_rows_once_per_step() {
        let pool = fresh_pool().await;
        let sid = mk_session(&pool).await;
        // Transcript spans 0..900_000 (15 min). With length=5min step=4min
        // safety_margin=30s, cutoff = 900000-30000 = 870000.
        // Windows: [0, 300k], [240k, 540k], [480k, 780k] — three rows.
        mk_final_transcript(&pool, sid, "hello", 0, 60_000).await;
        mk_final_transcript(&pool, sid, "world", 800_000, 900_000).await;

        schedule_windows_for_active_sessions(&pool, 5 * 60 * 1000, 4 * 60 * 1000)
            .await
            .unwrap();

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM extraction_windows")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 3, "expected three windows within the cutoff");

        // Running twice must be idempotent — no extra rows.
        schedule_windows_for_active_sessions(&pool, 5 * 60 * 1000, 4 * 60 * 1000)
            .await
            .unwrap();
        let count2: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM extraction_windows")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count2.0, 3);
    }

    #[tokio::test]
    async fn fetch_attributed_lines_filters_by_overlap() {
        let pool = fresh_pool().await;
        let sid = mk_session(&pool).await;
        mk_final_transcript(&pool, sid, "out-before", 0, 50_000).await;
        mk_final_transcript(&pool, sid, "in-window", 100_000, 150_000).await;
        mk_final_transcript(&pool, sid, "out-after", 400_000, 450_000).await;

        let lines = fetch_attributed_lines(&pool, sid, 60_000, 300_000)
            .await
            .unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "in-window");
    }

    #[tokio::test]
    async fn fetch_attributed_lines_uses_actual_speaker_id_from_segment() {
        let pool = fresh_pool().await;
        let sid = mk_session(&pool).await;
        let speaker_id = mk_speaker(&pool, "Alice").await;
        let segment_id = mk_segment(&pool, sid, speaker_id, 10_000, 20_000).await;
        mk_final_transcript_for_segment(
            &pool,
            sid,
            segment_id,
            "Alice owns this line",
            11_000,
            19_000,
        )
        .await;

        let lines = fetch_attributed_lines(&pool, sid, 0, 30_000).await.unwrap();

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].speaker_id, Some(speaker_id));
        assert_eq!(lines[0].speaker_name, "Alice");
    }

    #[tokio::test]
    async fn fetch_attributed_lines_falls_back_to_overlapping_segment_without_segment_id() {
        let pool = fresh_pool().await;
        let sid = mk_session(&pool).await;
        let speaker_id = mk_speaker(&pool, "Bob").await;
        mk_segment(&pool, sid, speaker_id, 100_000, 160_000).await;
        mk_final_transcript(&pool, sid, "Bob overlaps this line", 120_000, 140_000).await;

        let lines = fetch_attributed_lines(&pool, sid, 100_000, 160_000)
            .await
            .unwrap();

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].speaker_id, Some(speaker_id));
        assert_eq!(lines[0].speaker_name, "Bob");
    }

    #[tokio::test]
    async fn schedule_final_windows_includes_short_ended_session_tail() {
        let pool = fresh_pool().await;
        let sid = mk_session(&pool).await;
        sqlx::query("UPDATE audio_sessions SET ended_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1")
            .bind(sid.to_string())
            .execute(&pool)
            .await
            .unwrap();
        mk_final_transcript(&pool, sid, "short final session", 0, 120_000).await;

        let enqueued = schedule_final_windows_for_session(&pool, sid, 300_000, 240_000)
            .await
            .unwrap();

        assert_eq!(enqueued, 1);
        let row: (i64, i64) =
            sqlx::query_as("SELECT start_ms, end_ms FROM extraction_windows WHERE session_id = ?1")
                .bind(sid.to_string())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row, (0, 300_000));
    }

    #[test]
    fn format_attributed_transcript_shape() {
        let lines = vec![AttributedLine {
            text: "hello there".into(),
            start_ms: 3_665_000,
            end_ms: 3_670_000,
            speaker_id: None,
            speaker_name: "Alice".into(),
        }];
        let s = format_attributed_transcript(&lines);
        assert!(s.contains("[01:01:05 • Alice]:"));
        assert!(s.contains("hello there"));
    }

    // Unused import silencer for the parse path.
    #[allow(dead_code)]
    fn _force_use_secs_format() -> SecondsFormat {
        SecondsFormat::Secs
    }

    // ───────────────────────────────────────────────────────────────────
    // Integration tests for the full claim → fetch → route → gate → persist
    // pipeline using the cfg-test LlmRouter::Stub variant. These cover the
    // contract `process_window` upholds against the DB without needing a
    // live LLM backend or a full AppState.
    // ───────────────────────────────────────────────────────────────────

    use crate::engine::llm_router::LlmRouter;
    use crate::engine::remote_llm_client::LlmActionItem;

    fn stub_item(
        description: &str,
        confidence: &str,
        evidence: &str,
        speaker: Option<&str>,
    ) -> LlmActionItem {
        LlmActionItem {
            title: None,
            description: description.to_string(),
            priority: None,
            due_time: None,
            labels: vec![],
            confidence: Some(confidence.to_string()),
            evidence_quote: Some(evidence.to_string()),
            speaker_name: speaker.map(|s| s.to_string()),
        }
    }

    async fn upsert_test_window(
        pool: &SqlitePool,
        session_id: Uuid,
        start_ms: i64,
        end_ms: i64,
    ) -> window_repo::ExtractionWindow {
        window_repo::upsert_pending_window(pool, session_id, start_ms, end_ms)
            .await
            .unwrap();
        window_repo::claim_next_pending(pool).await.unwrap().unwrap()
    }

    #[tokio::test]
    async fn process_window_with_persists_high_and_pending_reminders() {
        let pool = fresh_pool().await;
        let sid = mk_session(&pool).await;
        let alice = mk_speaker(&pool, "Alice").await;
        let seg1 = mk_segment(&pool, sid, alice, 1_000, 30_000).await;
        let seg2 = mk_segment(&pool, sid, alice, 30_000, 60_000).await;
        // Total length must exceed MIN_EXTRACTABLE_CHARS (60).
        mk_final_transcript_for_segment(
            &pool,
            sid,
            seg1,
            "Please draft the design review summary by Thursday morning so the team can read it.",
            1_000,
            30_000,
        )
        .await;
        mk_final_transcript_for_segment(
            &pool,
            sid,
            seg2,
            "Also remind me to email the vendor about pricing.",
            30_000,
            60_000,
        )
        .await;

        let window = upsert_test_window(&pool, sid, 0, 300_000).await;

        let router = LlmRouter::stub(vec![
            stub_item(
                "Draft design review summary",
                "high",
                "draft the design review summary by Thursday morning",
                Some("Alice"),
            ),
            stub_item(
                "Email vendor about pricing",
                "medium",
                "email the vendor about pricing",
                Some("Alice"),
            ),
        ]);

        let outcome = process_window_with(&pool, &router, &window).await.unwrap();
        assert!(matches!(outcome, ProcessOutcome::Produced(2)));

        let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
            "SELECT description, status, source_window_id FROM reminders WHERE session_id = ?1 ORDER BY description",
        )
        .bind(sid.to_string())
        .fetch_all(&pool)
        .await
        .unwrap();

        assert_eq!(rows.len(), 2);
        let (d0, s0, w0) = &rows[0];
        let (d1, s1, w1) = &rows[1];
        assert_eq!(d0, "Draft design review summary");
        assert_eq!(s0, "open"); // high → open (lands on Board)
        assert_eq!(w0.as_deref(), Some(window.id.to_string().as_str()));
        assert_eq!(d1, "Email vendor about pricing");
        assert_eq!(s1, "pending"); // medium → pending (Needs-review queue)
        assert_eq!(w1.as_deref(), Some(window.id.to_string().as_str()));
    }

    #[tokio::test]
    async fn process_window_with_returns_llm_disabled_for_disabled_router() {
        let pool = fresh_pool().await;
        let sid = mk_session(&pool).await;
        let alice = mk_speaker(&pool, "Alice").await;
        let seg = mk_segment(&pool, sid, alice, 1_000, 30_000).await;
        mk_final_transcript_for_segment(
            &pool,
            sid,
            seg,
            "Please draft the design review summary by Thursday morning so the team can read it.",
            1_000,
            30_000,
        )
        .await;

        let window = upsert_test_window(&pool, sid, 0, 300_000).await;
        let router = LlmRouter::Disabled;

        let result = process_window_with(&pool, &router, &window).await;
        assert!(matches!(result, Err(ProcessError::LlmDisabled)));

        // Disabled is a non-event — no reminders should land.
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM reminders")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 0);
    }

    #[tokio::test]
    async fn process_window_with_returns_empty_for_short_transcript() {
        let pool = fresh_pool().await;
        let sid = mk_session(&pool).await;
        let alice = mk_speaker(&pool, "Alice").await;
        let seg = mk_segment(&pool, sid, alice, 1_000, 5_000).await;
        // Below MIN_EXTRACTABLE_CHARS (60) — must short-circuit before the router.
        mk_final_transcript_for_segment(&pool, sid, seg, "ok", 1_000, 5_000).await;

        let window = upsert_test_window(&pool, sid, 0, 300_000).await;

        // Stub returns items, but they should never be requested.
        let router = LlmRouter::stub(vec![stub_item(
            "should not appear",
            "high",
            "ok",
            Some("Alice"),
        )]);

        let outcome = process_window_with(&pool, &router, &window).await.unwrap();
        assert!(matches!(outcome, ProcessOutcome::Empty));

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM reminders")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 0);
    }
}

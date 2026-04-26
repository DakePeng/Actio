use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

// sqlx::FromRow binds by column name, so field order need not match the DB column order.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct Speaker {
    pub id: String,
    pub tenant_id: String,
    pub display_name: String,
    pub color: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SpeakerEmbedding {
    pub id: String,
    pub speaker_id: String,
    pub model_name: String,
    pub model_version: String,
    pub duration_ms: f64,
    pub quality_score: Option<f64>,
    pub is_primary: bool,
    pub embedding_dimension: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct AudioSession {
    pub id: String,
    pub tenant_id: String,
    pub source_type: String,
    pub mode: String,
    pub routing_policy: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub metadata: serde_json::Value,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AudioSegment {
    pub id: String,
    pub session_id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker_id: Option<String>,
    pub speaker_score: Option<f64>,
    pub audio_ref: Option<String>,
    pub quality_score: Option<f64>,
    pub vad_confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct Transcript {
    pub id: String,
    pub session_id: String,
    pub segment_id: Option<String>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub is_final: bool,
    pub backend_type: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, ToSchema)]
#[sqlx(type_name = "varchar", rename_all = "snake_case")]
pub enum TodoStatus {
    Open,
    Completed,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, ToSchema)]
#[sqlx(type_name = "varchar", rename_all = "snake_case")]
pub enum TodoPriority {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, ToSchema)]
pub struct TodoItem {
    pub id: String,
    pub session_id: Option<String>,
    pub speaker_id: Option<String>,
    pub assigned_to: Option<String>,
    pub description: String,
    pub status: TodoStatus,
    pub priority: Option<TodoPriority>,
    // New columns from migration 007 — nullable, ignored by old callers
    pub tenant_id: String,
    pub title: Option<String>,
    pub due_time: Option<DateTime<Utc>>,
    pub archived_at: Option<DateTime<Utc>>,
    pub transcript_excerpt: Option<String>,
    pub context: Option<String>,
    pub source_time: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Input struct for inserts (no id/created_at/updated_at — DB generates them)
#[allow(dead_code)]
#[derive(Debug)]
pub struct NewTodo {
    pub session_id: Uuid,
    pub speaker_id: Option<Uuid>,
    pub assigned_to: Option<String>,
    pub description: String,
    pub priority: Option<TodoPriority>,
}

// ── Reminder ─────────────────────────────────────────────────────────────

/// Raw DB row for the reminders table (no joined labels).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ReminderRow {
    pub id: String,
    pub session_id: Option<String>,
    pub tenant_id: String,
    pub speaker_id: Option<String>,
    pub assigned_to: Option<String>,
    pub title: Option<String>,
    pub description: String,
    pub status: String,
    pub priority: Option<String>,
    pub due_time: Option<DateTime<Utc>>,
    pub archived_at: Option<DateTime<Utc>>,
    pub transcript_excerpt: Option<String>,
    pub context: Option<String>,
    pub source_time: Option<DateTime<Utc>>,
    pub source_window_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ReminderRow {
    pub fn into_reminder(self, labels: Vec<Uuid>) -> Reminder {
        Reminder {
            id: self.id,
            session_id: self.session_id,
            tenant_id: self.tenant_id,
            speaker_id: self.speaker_id,
            assigned_to: self.assigned_to,
            title: self.title,
            description: self.description,
            status: self.status,
            priority: self.priority,
            due_time: self.due_time,
            archived_at: self.archived_at,
            transcript_excerpt: self.transcript_excerpt,
            context: self.context,
            source_time: self.source_time,
            source_window_id: self.source_window_id,
            labels,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// API response type — includes joined label IDs.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Reminder {
    pub id: String,
    pub session_id: Option<String>,
    pub tenant_id: String,
    pub speaker_id: Option<String>,
    pub assigned_to: Option<String>,
    pub title: Option<String>,
    pub description: String,
    pub status: String,
    pub priority: Option<String>,
    pub due_time: Option<DateTime<Utc>>,
    pub archived_at: Option<DateTime<Utc>>,
    pub transcript_excerpt: Option<String>,
    pub context: Option<String>,
    pub source_time: Option<DateTime<Utc>>,
    /// When set, points at an `extraction_windows` row — i.e. this reminder
    /// was produced by the background windowed extractor, not by an explicit
    /// session-end or user POST. Enables the "Show context" trace inspector.
    pub source_window_id: Option<String>,
    pub labels: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Input for creating a new reminder (API or LLM generator).
#[derive(Debug, Default)]
pub struct NewReminder {
    pub session_id: Option<Uuid>,
    pub tenant_id: Uuid,
    pub speaker_id: Option<Uuid>,
    pub assigned_to: Option<String>,
    pub title: Option<String>,
    pub description: String,
    pub priority: Option<String>,
    pub due_time: Option<DateTime<Utc>>,
    pub transcript_excerpt: Option<String>,
    pub context: Option<String>,
    pub source_time: Option<DateTime<Utc>>,
    /// Override default `'open'` status. Used by the window extractor to
    /// route medium-confidence items to the `'pending'` review queue.
    pub status: Option<String>,
    pub source_window_id: Option<Uuid>,
}

/// Query parameters for GET /reminders.
#[derive(Debug, Default)]
pub struct ReminderFilter {
    pub status: Option<String>,
    pub priority: Option<String>,
    pub label_id: Option<Uuid>,
    pub search: Option<String>,
    pub session_id: Option<Uuid>,
    pub limit: i64,
    pub offset: i64,
}

/// Body for PATCH /reminders/{id}.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PatchReminderRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<String>,
    pub due_time: Option<DateTime<Utc>>,
    pub status: Option<String>,
    pub labels: Option<Vec<Uuid>>,
}

// ── Reminder trace (provenance inspector) ─────────────────────────────────

/// Compact view of one transcript line rendered in the trace inspector.
/// Includes the speaker attribution joined from audio_segments → speakers
/// so the frontend doesn't need a second query.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ReminderTraceLine {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub speaker_id: Option<String>,
    pub speaker_name: Option<String>,
    /// Set when the line came from a batch-processed clip (the new
    /// pipeline). The frontend uses (clip_id, segment_id) to fetch the
    /// per-segment WAV via `GET /clips/{clip_id}/segments/{segment_id}/audio`.
    /// Null for legacy time-window-extracted lines.
    pub clip_id: Option<String>,
    pub segment_id: Option<String>,
}

/// Response body for `GET /reminders/{id}/trace`. Carries the originating
/// window's time bounds and the finalized transcripts inside it so the UI
/// can render "where did this card come from?" with speaker + timestamp.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ReminderTrace {
    pub reminder_id: String,
    pub session_id: Option<String>,
    pub window_id: Option<String>,
    pub window_start_ms: Option<i64>,
    pub window_end_ms: Option<i64>,
    pub session_started_at: Option<DateTime<Utc>>,
    pub transcript_excerpt: Option<String>,
    pub source_time: Option<DateTime<Utc>>,
    pub lines: Vec<ReminderTraceLine>,
}

// ── Label ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, ToSchema)]
pub struct Label {
    pub id: String,
    pub tenant_id: String,
    pub name: String,
    pub color: String,
    pub bg_color: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateLabelRequest {
    pub name: String,
    pub color: String,
    pub bg_color: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PatchLabelRequest {
    pub name: Option<String>,
    pub color: Option<String>,
    pub bg_color: Option<String>,
}

// ── Session listing ───────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct ListSessionsParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// ── Audio clips (batch processing) ────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeakerKind {
    Enrolled,
    Provisional,
}

impl SpeakerKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SpeakerKind::Enrolled => "enrolled",
            SpeakerKind::Provisional => "provisional",
        }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "enrolled" => Some(SpeakerKind::Enrolled),
            "provisional" => Some(SpeakerKind::Provisional),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AudioClip {
    pub id: Uuid,
    pub session_id: Uuid,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub segment_count: i64,
    pub manifest_path: String,
    pub status: String,
    pub attempts: i64,
    pub archive_model: Option<String>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipManifestSegment {
    pub id: Uuid,
    pub start_ms: i64,
    pub end_ms: i64,
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipManifest {
    pub clip_id: Uuid,
    pub session_id: Uuid,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub segments: Vec<ClipManifestSegment>,
}

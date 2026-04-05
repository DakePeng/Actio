use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct Speaker {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub display_name: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SpeakerEmbedding {
    pub id: Uuid,
    pub speaker_id: Uuid,
    pub model_name: String,
    pub model_version: String,
    pub duration_ms: f64,
    pub quality_score: Option<f64>,
    pub is_primary: bool,
    pub embedding_dimension: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct AudioSession {
    pub id: Uuid,
    pub tenant_id: Uuid,
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
    pub id: Uuid,
    pub session_id: Uuid,
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker_id: Option<Uuid>,
    pub speaker_score: Option<f64>,
    pub audio_ref: Option<String>,
    pub quality_score: Option<f64>,
    pub vad_confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct Transcript {
    pub id: Uuid,
    pub session_id: Uuid,
    pub segment_id: Option<Uuid>,
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
    pub id: Uuid,
    pub session_id: Uuid,
    pub speaker_id: Option<Uuid>,
    pub assigned_to: Option<String>,
    pub description: String,
    pub status: TodoStatus,
    pub priority: Option<TodoPriority>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Input struct for inserts (no id/created_at/updated_at — DB generates them)
#[derive(Debug)]
pub struct NewTodo {
    pub session_id: Uuid,
    pub speaker_id: Option<Uuid>,
    pub assigned_to: Option<String>,
    pub description: String,
    pub priority: Option<TodoPriority>,
}

impl TodoPriority {
    pub fn from_llm_label(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "high" => Some(Self::High),
            "medium" => Some(Self::Medium),
            "low" => Some(Self::Low),
            _ => None,
        }
    }
}

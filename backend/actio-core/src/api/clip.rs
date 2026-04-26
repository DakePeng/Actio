//! Clip audio endpoints — serve per-VAD-segment WAVs from the new
//! batch-clip-processing pipeline so the frontend trace inspector can
//! play back the audio behind each transcript line.
//!
//!   GET /clips/:clip_id/segments/:segment_id/audio
//!     200  audio/wav body, the segment's WAV file
//!     404  clip not found, or clip's manifest missing on disk, or
//!          segment_id not in the manifest, or the WAV file has been
//!          swept by the 14-day retention task
//!
//! The path is constructed by reading `audio_clips.manifest_path`,
//! parsing the manifest, and resolving the segment's `file` field
//! against the manifest's parent directory. Path traversal is blocked
//! by limiting the WAV path to the manifest's parent and rejecting
//! filenames that contain '/' or '\\'.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use crate::domain::types::ClipManifest;
use crate::repository::audio_clip;
use crate::AppState;

pub async fn get_clip_segment_audio(
    State(state): State<AppState>,
    Path((clip_id, segment_id)): Path<(Uuid, Uuid)>,
) -> Response {
    let clip = match audio_clip::get_by_id(&state.pool, clip_id).await {
        Ok(Some(c)) => c,
        Ok(None) => return not_found("clip not found"),
        Err(e) => return internal(format!("clip lookup failed: {e}")),
    };

    let manifest_body = match std::fs::read_to_string(&clip.manifest_path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return not_found("clip manifest missing on disk (likely swept by retention)");
        }
        Err(e) => return internal(format!("manifest read failed: {e}")),
    };
    let manifest: ClipManifest = match serde_json::from_str(&manifest_body) {
        Ok(m) => m,
        Err(e) => return internal(format!("manifest parse failed: {e}")),
    };

    let seg = match manifest
        .segments
        .iter()
        .find(|s| s.id == segment_id)
    {
        Some(s) => s,
        None => return not_found("segment not in this clip's manifest"),
    };

    // Path-traversal guard. The manifest is written by the clip writer
    // and shouldn't contain malicious paths, but defending in depth here
    // costs nothing and protects against a corrupted manifest.
    if seg.file.contains('/') || seg.file.contains('\\') || seg.file.contains("..") {
        return not_found("invalid segment filename");
    }

    let manifest_dir = match std::path::Path::new(&clip.manifest_path).parent() {
        Some(p) => p,
        None => return internal("manifest_path has no parent dir".into()),
    };
    let wav_path = manifest_dir.join(&seg.file);

    let bytes = match std::fs::read(&wav_path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return not_found("segment WAV missing on disk (likely swept by retention)");
        }
        Err(e) => return internal(format!("segment WAV read failed: {e}")),
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("audio/wav"),
    );
    // Long cache because (clip_id, segment_id) is content-addressed —
    // the file never changes for a given pair, only retention can
    // delete it.
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=86400"),
    );
    (StatusCode::OK, headers, Body::from(bytes)).into_response()
}

fn not_found(msg: impl Into<String>) -> Response {
    (
        StatusCode::NOT_FOUND,
        axum::Json(serde_json::json!({ "error": msg.into() })),
    )
        .into_response()
}

fn internal(msg: String) -> Response {
    tracing::warn!(error = %msg, "clip audio endpoint internal error");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(serde_json::json!({ "error": msg })),
    )
        .into_response()
}

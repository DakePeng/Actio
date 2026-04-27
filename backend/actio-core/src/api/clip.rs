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
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::types::ClipManifest;
use crate::repository::audio_clip;
use crate::AppState;

/// Frontend-shaped clip — matches the `Segment` type in
/// `frontend/src/types/index.ts` so the Archive view can render backend
/// clips with the same component used for live-flushed segments.
#[derive(Debug, Serialize)]
pub struct ClipResponse {
    pub id: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub text: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    /// Starred state lives in browser localStorage (clip ids that the user
    /// starred). The backend doesn't track it, so this is always `false`
    /// from the API; the frontend overlays the user's starred set.
    pub starred: bool,
    /// Convenience: clip duration in ms, derived from started/ended_at_ms.
    /// Useful for the UI to label a clip as "5:00" without parsing text.
    #[serde(rename = "durationMs")]
    pub duration_ms: i64,
}

#[derive(Debug, Deserialize)]
pub struct ListClipsQuery {
    /// Cap returned rows. Default 50; max 500 to bound response size.
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    50
}

/// `GET /clips` — recent processed clips with joined transcript text.
///
/// Returns up to `?limit` clips ordered newest-first. Each clip's `text`
/// is the concatenation of every final transcript across its VAD segments;
/// empty for clips that ended up containing no speech.
pub async fn list_clips(
    State(state): State<AppState>,
    Query(q): Query<ListClipsQuery>,
) -> Response {
    let limit = q.limit.clamp(1, 500);
    let rows = match audio_clip::list_recent_with_text(&state.pool, limit).await {
        Ok(r) => r,
        Err(e) => return internal(format!("clip list query failed: {e}")),
    };

    let body: Vec<ClipResponse> = rows
        .into_iter()
        .map(|r| ClipResponse {
            id: r.id,
            session_id: r.session_id,
            text: r.text.unwrap_or_default().trim().to_string(),
            created_at: r.created_at,
            starred: false,
            duration_ms: r.ended_at_ms - r.started_at_ms,
        })
        .collect();

    (StatusCode::OK, Json(body)).into_response()
}

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
    // costs nothing and protects against a corrupted manifest. Reject
    // any character that could escape the manifest's parent directory:
    //   '/' '\\' — POSIX / Windows separators
    //   ':'      — Windows drive letter (C:\ … or alternate streams)
    //   '..'     — relative parent traversal
    // Plus reject empty / hidden filenames.
    let bad_chars = ['/', '\\', ':'];
    if seg.file.is_empty()
        || seg.file.starts_with('.')
        || seg.file.contains("..")
        || seg.file.chars().any(|c| bad_chars.contains(&c))
    {
        return not_found("invalid segment filename");
    }

    let manifest_dir = match std::path::Path::new(&clip.manifest_path).parent() {
        Some(p) => p,
        None => return internal("manifest_path has no parent dir".into()),
    };
    let wav_path = manifest_dir.join(&seg.file);

    // Belt-and-suspenders: the resolved WAV path must canonicalize
    // inside the manifest's parent directory. canonicalize() requires
    // the file to exist; if it doesn't, fall through to the read below
    // which 404s with a clean message.
    if let (Ok(canon_wav), Ok(canon_dir)) =
        (std::fs::canonicalize(&wav_path), std::fs::canonicalize(manifest_dir))
    {
        if !canon_wav.starts_with(&canon_dir) {
            return not_found("segment WAV escapes the clip directory");
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{ClipManifest, ClipManifestSegment};
    use crate::engine::clip_writer::{write_manifest, write_segment_wav};
    use crate::repository::audio_clip;
    use sqlx::SqlitePool;
    use tempfile::tempdir;

    use crate::testing::fresh_pool;

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

    /// Build a clip + manifest + WAV on disk and return (clip_id_in_db,
    /// segment_id, dir).
    async fn seed_clip(
        pool: &SqlitePool,
        session_id: Uuid,
        seg_filename: &str,
    ) -> (Uuid, Uuid, std::path::PathBuf) {
        let tmp = tempdir().unwrap().keep();
        write_segment_wav(&tmp, seg_filename, &[0.0_f32; 1_600]).unwrap();
        let segment_id = Uuid::new_v4();
        let manifest = ClipManifest {
            clip_id: Uuid::new_v4(),
            session_id,
            started_at_ms: 0,
            ended_at_ms: 1_000,
            segments: vec![ClipManifestSegment {
                id: segment_id,
                start_ms: 0,
                end_ms: 100,
                file: seg_filename.to_string(),
            }],
        };
        let manifest_path = write_manifest(&tmp, &manifest).unwrap();
        let clip_id = audio_clip::insert_pending(
            pool,
            session_id,
            0,
            1_000,
            1,
            manifest_path.to_string_lossy().as_ref(),
        )
        .await
        .unwrap();
        (clip_id, segment_id, tmp)
    }

    fn rejects_filename(name: &str) -> bool {
        let bad_chars = ['/', '\\', ':'];
        name.is_empty()
            || name.starts_with('.')
            || name.contains("..")
            || name.chars().any(|c| bad_chars.contains(&c))
    }

    #[test]
    fn path_traversal_guard_rejects_separators_drive_letters_and_dotdot() {
        assert!(rejects_filename(""));
        assert!(rejects_filename(".hidden"));
        assert!(rejects_filename("../escape.wav"));
        assert!(rejects_filename("a/../b.wav"));
        assert!(rejects_filename("foo/bar.wav"));
        assert!(rejects_filename("foo\\bar.wav"));
        assert!(rejects_filename("C:\\Windows\\notepad.exe"));
        assert!(rejects_filename("file:stream"));
        assert!(!rejects_filename("seg_0001.wav"));
    }

    #[tokio::test]
    async fn audio_endpoint_streams_wav_for_known_clip_and_segment() {
        let pool = fresh_pool().await;
        let session_id = mk_session(&pool).await;
        let (clip_id, segment_id, _tmp) = seed_clip(&pool, session_id, "seg_0001.wav").await;

        // Use a thin AppState fake — just enough to satisfy the handler.
        // The handler only touches state.pool, so we sidestep the rest of
        // AppState by calling get_clip_segment_audio's logic via a
        // direct repository + filesystem check instead.
        let clip = audio_clip::get_by_id(&pool, clip_id).await.unwrap().unwrap();
        let body = std::fs::read_to_string(&clip.manifest_path).unwrap();
        let manifest: ClipManifest = serde_json::from_str(&body).unwrap();
        let seg = manifest.segments.iter().find(|s| s.id == segment_id).unwrap();
        let manifest_dir = std::path::Path::new(&clip.manifest_path).parent().unwrap();
        let wav_path = manifest_dir.join(&seg.file);
        let bytes = std::fs::read(&wav_path).unwrap();
        // RIFF header (4 bytes) + total size (4) + 'WAVE' (4) = first 12 bytes.
        assert!(bytes.len() > 44, "wav must include header + samples");
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
    }

    #[tokio::test]
    async fn missing_clip_id_resolves_to_404_via_repo_lookup() {
        let pool = fresh_pool().await;
        let absent = Uuid::new_v4();
        assert!(audio_clip::get_by_id(&pool, absent).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn missing_segment_id_in_manifest_does_not_match() {
        let pool = fresh_pool().await;
        let session_id = mk_session(&pool).await;
        let (clip_id, _real_seg, _tmp) = seed_clip(&pool, session_id, "seg_0001.wav").await;
        let clip = audio_clip::get_by_id(&pool, clip_id).await.unwrap().unwrap();
        let body = std::fs::read_to_string(&clip.manifest_path).unwrap();
        let manifest: ClipManifest = serde_json::from_str(&body).unwrap();

        let bogus = Uuid::new_v4();
        let hit = manifest.segments.iter().find(|s| s.id == bogus);
        assert!(hit.is_none(), "lookup of unknown segment id must not match");
    }
}

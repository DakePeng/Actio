use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tracing::{debug, info, warn};

/// Phase A voiceprint-candidate retention.
///
/// Writes 16 kHz mono f32 PCM to a WAV file under `dir/{segment_id}.wav`.
/// Returns the file name (not the full path) so the caller can store a
/// relative reference in `audio_segments.audio_ref` — the full path is
/// reconstructed at read time by combining with the configured clips dir.
pub fn write_clip(dir: &Path, segment_id: &str, audio: &[f32]) -> anyhow::Result<String> {
    std::fs::create_dir_all(dir)?;
    let file_name = format!("{segment_id}.wav");
    let path = dir.join(&file_name);
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&path, spec)?;
    for s in audio {
        writer.write_sample(*s)?;
    }
    writer.finalize()?;
    debug!(
        ?path,
        samples = audio.len(),
        "voiceprint candidate clip retained"
    );
    Ok(file_name)
}

/// Spawn a background task that periodically deletes WAV files older than
/// `retention_days`. Runs once at startup then once per hour.
///
/// Designed to be called from `start_server` — the task outlives the
/// caller and is never awaited; it continues until the process exits.
pub fn start_cleanup_task(dir: PathBuf, retention_days: u32) {
    tokio::spawn(async move {
        // Run once immediately so stale clips from a prior run with a
        // higher retention setting get pruned quickly.
        sweep(&dir, retention_days);
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            sweep(&dir, retention_days);
        }
    });
}

fn sweep(dir: &Path, retention_days: u32) {
    let cutoff =
        match SystemTime::now().checked_sub(Duration::from_secs(retention_days as u64 * 86_400)) {
            Some(t) => t,
            None => {
                warn!("clip cleanup cutoff computation underflowed — skipping sweep");
                return;
            }
        };
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
        Err(err) => {
            warn!(?err, ?dir, "clip cleanup could not read dir");
            return;
        }
    };
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("wav") {
            continue;
        }
        let modified = entry.metadata().ok().and_then(|m| m.modified().ok());
        let Some(modified) = modified else { continue };
        if modified < cutoff {
            match std::fs::remove_file(&path) {
                Ok(_) => removed += 1,
                Err(err) => warn!(?path, ?err, "failed to delete stale clip"),
            }
        }
    }
    if removed > 0 {
        info!(
            removed,
            retention_days, "pruned stale voiceprint-candidate clips"
        );
    }
}

// ── Nested clip-dir sweep (batch clip processing) ────────────────────────

/// Spawn a background task that periodically removes per-clip directories
/// older than `retention_days` under `<clips_dir>/<session_id>/<clip_id>/`.
/// Distinct from `start_cleanup_task` (flat-dir voiceprint candidate
/// retention) because the new layout is two levels deep and removes the
/// entire clip directory rather than individual WAVs.
pub fn start_clip_dir_cleanup_task(dir: PathBuf, retention_days: u32) {
    tokio::spawn(async move {
        sweep_clip_dirs(&dir, retention_days);
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            sweep_clip_dirs(&dir, retention_days);
        }
    });
}

fn sweep_clip_dirs(dir: &Path, retention_days: u32) {
    let cutoff = match SystemTime::now()
        .checked_sub(Duration::from_secs(retention_days as u64 * 86_400))
    {
        Some(t) => t,
        None => {
            warn!("clip dir sweep cutoff underflow — skipping");
            return;
        }
    };
    let sessions = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
        Err(err) => {
            warn!(?err, ?dir, "clip dir sweep could not read root");
            return;
        }
    };
    let mut removed_clips = 0usize;
    for session_entry in sessions.flatten() {
        let session_path = session_entry.path();
        if !session_path.is_dir() {
            continue;
        }
        let clips = match std::fs::read_dir(&session_path) {
            Ok(e) => e,
            Err(err) => {
                warn!(?err, ?session_path, "clip dir sweep could not read session");
                continue;
            }
        };
        for clip_entry in clips.flatten() {
            let clip_path = clip_entry.path();
            if !clip_path.is_dir() {
                continue;
            }
            let modified = clip_entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or_else(|_| SystemTime::now());
            if modified < cutoff {
                match std::fs::remove_dir_all(&clip_path) {
                    Ok(_) => {
                        removed_clips += 1;
                        debug!(?clip_path, "removed stale clip dir");
                    }
                    Err(err) => warn!(?clip_path, ?err, "failed to remove stale clip dir"),
                }
            }
        }
    }
    if removed_clips > 0 {
        info!(
            removed_clips,
            retention_days, "pruned stale audio clip directories"
        );
    }
}

/// Spawn a daily-cadence task that calls `gc_stale_provisionals` so
/// provisional speakers untouched for `gc_days` are removed and their
/// segments' speaker_id reverts to NULL. Backs the spec's "30 days
/// unmatched → auto-GC" decision.
pub fn start_provisional_speaker_gc_task(pool: sqlx::SqlitePool, gc_days: i64) {
    tokio::spawn(async move {
        loop {
            match crate::repository::speaker::gc_stale_provisionals(&pool, gc_days).await {
                Ok(0) => {}
                Ok(n) => info!(count = n, "GC'd stale provisional speakers"),
                Err(e) => warn!(error = %e, "provisional speaker GC failed"),
            }
            tokio::time::sleep(Duration::from_secs(86_400)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn write_clip_produces_valid_wav() {
        let tmp = tempfile::tempdir().unwrap();
        let audio: Vec<f32> = (0..16_000)
            .map(|i| (i as f32 / 100.0).sin() * 0.2)
            .collect();
        let name = write_clip(tmp.path(), "abc-123", &audio).unwrap();
        assert_eq!(name, "abc-123.wav");

        let path = tmp.path().join(&name);
        let bytes = std::fs::read(&path).unwrap();
        let reader = hound::WavReader::new(Cursor::new(bytes)).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.sample_rate, 16_000);
        assert_eq!(spec.channels, 1);
    }

    #[test]
    fn sweep_leaves_fresh_files_alone() {
        // Can't portably back-date a file's mtime without adding a dev
        // dependency, so we only cover the "don't delete fresh files" branch.
        // The cutoff math is simple arithmetic — code review covers the rest.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("fresh.wav");
        std::fs::write(&path, b"content").unwrap();
        sweep(tmp.path(), 3);
        assert!(path.exists());
    }

    #[test]
    fn sweep_tolerates_missing_dir() {
        // Never-been-created clips dir must not panic the cleanup task.
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist");
        sweep(&missing, 3);
    }

    #[test]
    fn sweep_clip_dirs_leaves_fresh_dirs_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let session = tmp.path().join("sess-A");
        let clip = session.join("clip-1");
        std::fs::create_dir_all(&clip).unwrap();
        std::fs::write(clip.join("seg_0001.wav"), b"x").unwrap();
        sweep_clip_dirs(tmp.path(), 14);
        assert!(clip.exists(), "fresh clip dir must not be removed");
    }

    #[test]
    fn sweep_clip_dirs_tolerates_missing_root() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("never-created");
        sweep_clip_dirs(&missing, 14);
    }

    #[tokio::test]
    async fn provisional_gc_removes_old_unmatched() {
        use crate::repository::db::run_migrations;
        use crate::repository::speaker::{
            gc_stale_provisionals, insert_provisional, list_provisional,
        };
        use sqlx::sqlite::SqlitePoolOptions;
        use uuid::Uuid;

        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();

        let tenant = Uuid::nil();
        let id_old = Uuid::new_v4();
        insert_provisional(&pool, id_old, tenant, "old", "#aaa")
            .await
            .unwrap();
        // Back-date the old row's last-match timestamp far past the cutoff.
        sqlx::query(
            "UPDATE speakers SET provisional_last_matched_at = '2020-01-01T00:00:00.000Z' WHERE id = ?1",
        )
        .bind(id_old.to_string())
        .execute(&pool)
        .await
        .unwrap();

        let id_new = Uuid::new_v4();
        insert_provisional(&pool, id_new, tenant, "new", "#bbb")
            .await
            .unwrap();

        let removed = gc_stale_provisionals(&pool, 30).await.unwrap();
        assert_eq!(removed, 1);

        let remaining = list_provisional(&pool).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, id_new.to_string());
    }
}

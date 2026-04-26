//! Drains CaptureDaemon events into per-clip on-disk artifacts (segment
//! WAVs + manifest) and inserts the matching `audio_clips` row.
//!
//! Owns the `BoundaryWatcher`. On `Decision::CloseClip` it finalizes the
//! manifest, inserts the row, resets the watcher, and continues. The
//! daemon's `archive_enabled` flag gates persistence — when off, speech
//! events flow past untouched (live streaming still gets them).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlx::SqlitePool;
use tokio::sync::broadcast;
use tracing::{info, warn};
use uuid::Uuid;

use crate::domain::types::{ClipManifest, ClipManifestSegment};
use crate::engine::capture_daemon::{CaptureDaemon, CaptureEvent};
use crate::engine::clip_boundary::{BoundaryConfig, BoundaryEvent, BoundaryWatcher, Decision};
use crate::repository::audio_clip;

/// Write a single VAD speech segment as a 16 kHz mono f32 WAV under `dir`.
pub fn write_segment_wav(dir: &Path, name: &str, samples: &[f32]) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(name);
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(&path, spec)?;
    for s in samples {
        w.write_sample(*s)?;
    }
    w.finalize()?;
    Ok(())
}

/// Serialize the clip's manifest to `<dir>/manifest.json`.
pub fn write_manifest(dir: &Path, manifest: &ClipManifest) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("manifest.json");
    let body = serde_json::to_string_pretty(manifest)?;
    std::fs::write(&path, body)?;
    Ok(path)
}

#[derive(Clone)]
pub struct ClipWriterConfig {
    pub clips_dir: PathBuf,
    pub boundary: BoundaryConfig,
}

/// Long-running consumer that turns capture events into clips. Returns
/// when the broadcast sender is dropped (daemon shutdown) or the receiver
/// closes for any reason.
pub async fn run_clip_writer_loop(
    pool: SqlitePool,
    session_id: Uuid,
    cfg: ClipWriterConfig,
    daemon: Arc<CaptureDaemon>,
    mut events: broadcast::Receiver<CaptureEvent>,
) {
    let mut watcher = BoundaryWatcher::new(cfg.boundary);
    let mut current_clip_id: Option<Uuid> = None;
    let mut current_dir: Option<PathBuf> = None;
    let mut clip_started_ms: Option<i64> = None;
    let mut next_seg_idx: usize = 0;
    let mut segments: Vec<ClipManifestSegment> = Vec::new();

    loop {
        let ev = match events.recv().await {
            Ok(ev) => ev,
            Err(broadcast::error::RecvError::Lagged(n)) => {
                warn!(skipped = n, "ClipWriter lagged on broadcast channel");
                continue;
            }
            Err(broadcast::error::RecvError::Closed) => break,
        };

        if !daemon.archive_enabled().await {
            // Privacy mode — drop archive work but still let mute events
            // close any straggler clip we somehow opened before the flag
            // flipped.
            if matches!(ev, CaptureEvent::Muted) && current_clip_id.is_some() {
                close_current_clip(
                    &pool,
                    session_id,
                    &mut current_clip_id,
                    &mut current_dir,
                    &mut clip_started_ms,
                    &mut segments,
                )
                .await;
                watcher.reset_after_close();
            }
            continue;
        }

        let boundary_event = match &ev {
            CaptureEvent::Speech(seg) => Some(BoundaryEvent::Speech {
                start_ms: ms_from_sample(seg.start_sample),
                end_ms: ms_from_sample(seg.end_sample),
            }),
            CaptureEvent::Muted => Some(BoundaryEvent::Mute),
            CaptureEvent::Pcm(_) | CaptureEvent::Unmuted => None,
        };

        if let CaptureEvent::Speech(seg) = &ev {
            let seg_start_ms = ms_from_sample(seg.start_sample);
            let seg_end_ms = ms_from_sample(seg.end_sample);

            if current_clip_id.is_none() {
                let clip_id = Uuid::new_v4();
                let dir = cfg
                    .clips_dir
                    .join(session_id.to_string())
                    .join(clip_id.to_string());
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    warn!(error=%e, "ClipWriter could not create clip dir");
                    continue;
                }
                current_clip_id = Some(clip_id);
                current_dir = Some(dir);
                clip_started_ms = Some(seg_start_ms);
                next_seg_idx = 0;
                segments.clear();
            }

            let dir = current_dir.as_ref().unwrap().clone();
            next_seg_idx += 1;
            let name = format!("seg_{:04}.wav", next_seg_idx);
            if let Err(e) = write_segment_wav(&dir, &name, &seg.audio) {
                warn!(error=%e, "Failed to write segment WAV");
                continue;
            }
            segments.push(ClipManifestSegment {
                id: seg.segment_id,
                start_ms: seg_start_ms,
                end_ms: seg_end_ms,
                file: name,
            });
        }

        if let Some(be) = boundary_event {
            if matches!(watcher.observe(be), Decision::CloseClip) {
                close_current_clip(
                    &pool,
                    session_id,
                    &mut current_clip_id,
                    &mut current_dir,
                    &mut clip_started_ms,
                    &mut segments,
                )
                .await;
                watcher.reset_after_close();
            }
        }
    }
    info!(%session_id, "ClipWriter loop exited");
}

#[allow(clippy::too_many_arguments)]
async fn close_current_clip(
    pool: &SqlitePool,
    session_id: Uuid,
    clip_id_slot: &mut Option<Uuid>,
    dir_slot: &mut Option<PathBuf>,
    started_slot: &mut Option<i64>,
    segments: &mut Vec<ClipManifestSegment>,
) {
    let (clip_id, dir, started) =
        match (clip_id_slot.take(), dir_slot.take(), started_slot.take()) {
            (Some(c), Some(d), Some(s)) => (c, d, s),
            _ => {
                segments.clear();
                return;
            }
        };
    let ended_at_ms = segments.last().map(|s| s.end_ms).unwrap_or(started);
    let manifest = ClipManifest {
        clip_id,
        session_id,
        started_at_ms: started,
        ended_at_ms,
        segments: std::mem::take(segments),
    };
    let manifest_path = match write_manifest(&dir, &manifest) {
        Ok(p) => p,
        Err(e) => {
            warn!(error=%e, "Failed to write manifest");
            return;
        }
    };
    if let Err(e) = audio_clip::insert_pending(
        pool,
        session_id,
        manifest.started_at_ms,
        manifest.ended_at_ms,
        manifest.segments.len() as i64,
        manifest_path.to_string_lossy().as_ref(),
    )
    .await
    {
        warn!(error=%e, "Failed to insert audio_clips row");
    } else {
        info!(%clip_id, "audio clip closed and queued");
    }
}

/// Convert a 16 kHz sample index to an absolute ms offset within the
/// session. The VAD uses sample indices; the rest of the pipeline (DB,
/// boundary watcher, manifest) uses ms — convert at the boundary.
fn ms_from_sample(sample: usize) -> i64 {
    (sample as i64 * 1_000) / 16_000
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn manifest_round_trips_through_disk() {
        let tmp = tempdir().unwrap();
        let m = ClipManifest {
            clip_id: Uuid::nil(),
            session_id: Uuid::nil(),
            started_at_ms: 0,
            ended_at_ms: 300_000,
            segments: vec![ClipManifestSegment {
                id: Uuid::nil(),
                start_ms: 1_000,
                end_ms: 4_000,
                file: "seg_0001.wav".into(),
            }],
        };
        let path = write_manifest(tmp.path(), &m).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        let back: ClipManifest = serde_json::from_str(&body).unwrap();
        assert_eq!(back.segments.len(), 1);
        assert_eq!(back.segments[0].file, "seg_0001.wav");
        assert_eq!(back.started_at_ms, 0);
        assert_eq!(back.ended_at_ms, 300_000);
    }

    #[test]
    fn write_segment_wav_round_trips_samples() {
        let tmp = tempdir().unwrap();
        let samples: Vec<f32> = (0..1024).map(|i| (i as f32 / 1024.0) - 0.5).collect();
        write_segment_wav(tmp.path(), "seg_0001.wav", &samples).unwrap();

        let mut reader = hound::WavReader::open(tmp.path().join("seg_0001.wav")).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 16_000);
        assert_eq!(spec.bits_per_sample, 32);
        let back: Vec<f32> = reader
            .samples::<f32>()
            .filter_map(|s| s.ok())
            .collect();
        assert_eq!(back.len(), samples.len());
        // Identity for f32 round-trip.
        for (a, b) in samples.iter().zip(back.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn ms_from_sample_at_16khz() {
        assert_eq!(ms_from_sample(0), 0);
        assert_eq!(ms_from_sample(16_000), 1_000);
        assert_eq!(ms_from_sample(8_000), 500);
    }

    use crate::engine::capture_daemon::{CaptureDaemon, CaptureEvent};
    use crate::engine::vad::SpeechSegment;
    use crate::repository::db::run_migrations;
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::SqlitePool;

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

    fn synth_speech(start_sample: usize, end_sample: usize) -> SpeechSegment {
        let n = end_sample.saturating_sub(start_sample);
        let audio: Vec<f32> = (0..n).map(|i| (i as f32 / 1024.0).sin() * 0.1).collect();
        SpeechSegment {
            segment_id: Uuid::new_v4(),
            start_sample,
            end_sample,
            audio,
        }
    }

    /// Drive the clip writer loop with synthetic Speech events spanning
    /// past the 5-min target, then a Mute. Verify a clip closes, segment
    /// WAVs land on disk, and an audio_clips row appears.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn loop_writes_clip_to_disk_and_inserts_audio_clips_row() {
        let pool = fresh_pool().await;
        let session_id = mk_session(&pool).await;

        let tmp = tempdir().unwrap();
        let daemon = Arc::new(CaptureDaemon::new(
            None,
            std::path::PathBuf::from("nonexistent_silero.onnx"),
        ));
        let cfg = ClipWriterConfig {
            clips_dir: tmp.path().to_path_buf(),
            // Tight target so a single short event passes the threshold.
            boundary: BoundaryConfig {
                target_secs: 1,
                max_secs: 5,
                silence_close_ms: 100,
            },
        };
        let events = daemon.subscribe();
        let pool_clone = pool.clone();
        let daemon_clone = daemon.clone();
        let writer_task = tokio::spawn(async move {
            run_clip_writer_loop(pool_clone, session_id, cfg, daemon_clone, events).await;
        });

        // Two speech segments, then a Mute to force-close.
        // sample 0 → ms 0; 1.5s of speech then 1s gap then more speech.
        daemon.test_push(CaptureEvent::Speech(Arc::new(synth_speech(0, 24_000))));
        // Tiny pause so the writer task can flush this event before the
        // next one races in (broadcast is lossy under lag).
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        daemon.test_push(CaptureEvent::Speech(Arc::new(synth_speech(
            24_000 + 16_000,
            24_000 + 16_000 + 24_000,
        ))));
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        // Mute closes the in-flight clip immediately.
        daemon.test_push(CaptureEvent::Muted);

        // Give the writer task time to consume + write to disk + DB.
        for _ in 0..50 {
            let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM audio_clips")
                .fetch_one(&pool)
                .await
                .unwrap();
            if count.0 >= 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        }

        // Verify: one clip row exists, manifest + WAVs on disk. Note that
        // audio_clips.id is a fresh UUID generated by insert_pending — it
        // is NOT the manifest's clip_id (which names the on-disk dir).
        // Use manifest_path to locate the dir.
        let clips: Vec<(String, String, i64, i64, i64, String)> = sqlx::query_as(
            "SELECT id, status, started_at_ms, ended_at_ms, segment_count, manifest_path \
             FROM audio_clips",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(clips.len(), 1, "expected exactly one clip row");
        let (_id, status, _started, _ended, seg_count, manifest_path) = &clips[0];
        assert_eq!(status, "pending");
        assert_eq!(*seg_count, 2);

        let manifest_pb = std::path::PathBuf::from(manifest_path);
        let clip_dir = manifest_pb.parent().expect("manifest path has parent");
        assert!(clip_dir.exists(), "clip dir {:?} must exist", clip_dir);
        assert!(manifest_pb.exists(), "manifest must exist at {:?}", manifest_pb);
        assert!(
            clip_dir.join("seg_0001.wav").exists(),
            "first segment WAV must exist in {:?}",
            clip_dir
        );
        assert!(
            clip_dir.join("seg_0002.wav").exists(),
            "second segment WAV must exist in {:?}",
            clip_dir
        );

        writer_task.abort();
    }

    /// archive_enabled=false should make speech events bypass the writer
    /// entirely — no WAVs, no audio_clips row.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn loop_drops_events_when_archive_disabled() {
        let pool = fresh_pool().await;
        let session_id = mk_session(&pool).await;

        let tmp = tempdir().unwrap();
        let daemon = Arc::new(CaptureDaemon::new(
            None,
            std::path::PathBuf::from("nonexistent_silero.onnx"),
        ));
        daemon.set_archive_enabled(false).await;

        let cfg = ClipWriterConfig {
            clips_dir: tmp.path().to_path_buf(),
            boundary: BoundaryConfig {
                target_secs: 1,
                max_secs: 5,
                silence_close_ms: 100,
            },
        };
        let events = daemon.subscribe();
        let pool_clone = pool.clone();
        let daemon_clone = daemon.clone();
        let writer_task = tokio::spawn(async move {
            run_clip_writer_loop(pool_clone, session_id, cfg, daemon_clone, events).await;
        });

        daemon.test_push(CaptureEvent::Speech(Arc::new(synth_speech(0, 24_000))));
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM audio_clips")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 0, "privacy mode must produce no clips");

        // Filesystem should also be untouched (no session dir created).
        assert!(!tmp.path().join(session_id.to_string()).exists());

        writer_task.abort();
    }
}

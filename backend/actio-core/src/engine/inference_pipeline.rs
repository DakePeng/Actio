use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};
use uuid::Uuid;

use crate::engine::asr;
use crate::engine::audio_capture::{self, AudioCaptureHandle};
use crate::engine::model_manager::ModelPaths;
use crate::engine::transcript_aggregator::TranscriptAggregator;
use crate::engine::vad::{self, SpeechSegment, VadConfig};

pub struct InferencePipeline {
    /// The capture handle — dropping it stops capture
    capture_handle: Option<AudioCaptureHandle>,
    /// Cancel token for the pipeline tasks
    cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

// SAFETY: InferencePipeline is always accessed behind a Mutex, so it will never
// be sent across threads concurrently. cpal::Stream contains *mut () which is
// !Send by default, but the stream is created and destroyed on the same thread
// via the Mutex guard.
unsafe impl Send for InferencePipeline {}

impl InferencePipeline {
    pub fn new() -> Self {
        Self {
            capture_handle: None,
            cancel_tx: None,
        }
    }

    /// Start the pipeline for a session. Returns error if models aren't ready.
    pub fn start_session(
        &mut self,
        session_id: Uuid,
        model_paths: &ModelPaths,
        aggregator: Arc<TranscriptAggregator>,
        device_name: Option<&str>,
        asr_model: Option<&str>,
    ) -> anyhow::Result<()> {
        // 1. Start audio capture
        let (capture_handle, audio_rx) = audio_capture::start_capture(device_name)?;

        // 2. Select ASR model based on user setting and route audio to the
        //    appropriate recognizer.
        //    - Zipformer transducers (zh/en): direct streaming ASR, no VAD.
        //    - SenseVoice: VAD segments fed into offline ASR.
        //    - Other catalog entries are not yet wired.
        let chosen = asr_model.unwrap_or("auto");
        info!(model = chosen, "Selected ASR model");

        if model_paths.speaker_embedding.is_none() {
            info!("Speaker embedding model not loaded — segments will stay [UNKNOWN]");
        }

        // Helper: start VAD, then fan each speech segment into (a) a per-segment
        // speaker-identification task and (b) the downstream offline ASR
        // consumer. Streaming ASR paths do not go through this helper — they
        // consume raw audio directly and skip per-segment identification.
        let embedding_model = model_paths.speaker_embedding.clone();
        let pool = aggregator.pool();
        let start_vad_pipeline = |audio_rx: mpsc::Receiver<Vec<f32>>| -> anyhow::Result<_> {
            if !model_paths.silero_vad.exists() {
                return Err(anyhow::anyhow!(
                    "Silero VAD not found — required for offline models"
                ));
            }
            let upstream =
                vad::start_vad(&model_paths.silero_vad, VadConfig::default(), audio_rx)?;
            Ok(split_segments_for_speaker_id(
                upstream,
                session_id,
                Uuid::nil(),
                pool.clone(),
                embedding_model.clone(),
            ))
        };

        let transcript_rx = if let Some(files) = model_paths.transducers.get(chosen) {
            // ── Streaming transducer (any model in the table) ────────
            asr::start_streaming_asr(files, audio_rx)?
        } else {
            match chosen {
                // ── Offline models (individual routing) ──────────────
                "whisper_base" => {
                    let files = model_paths
                        .whisper_base
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("Whisper base model not downloaded"))?;
                    let seg_rx = start_vad_pipeline(audio_rx)?;
                    asr::start_whisper_asr(files, seg_rx)?
                }
                "whisper_turbo" => {
                    let files = model_paths
                        .whisper_turbo
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("Whisper turbo model not downloaded"))?;
                    let seg_rx = start_vad_pipeline(audio_rx)?;
                    asr::start_whisper_asr(files, seg_rx)?
                }
                "zipformer_ctc_zh_small" => {
                    let files = model_paths
                        .zipformer_ctc_zh_small
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("Zipformer CTC zh-small not downloaded"))?;
                    let seg_rx = start_vad_pipeline(audio_rx)?;
                    asr::start_zipformer_ctc_asr(files, seg_rx)?
                }
                "paraformer_zh_small" => {
                    let files = model_paths
                        .paraformer_zh_small
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("Paraformer zh-small not downloaded"))?;
                    let seg_rx = start_vad_pipeline(audio_rx)?;
                    asr::start_paraformer_offline_asr(files, seg_rx)?
                }
                "sense_voice_multi" => {
                    let files = model_paths
                        .sense_voice
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("SenseVoice not downloaded"))?;
                    let seg_rx = start_vad_pipeline(audio_rx)?;
                    asr::start_sense_voice_asr(files, seg_rx)?
                }
                "moonshine_tiny_en" => {
                    let files = model_paths
                        .moonshine_en
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("Moonshine not downloaded"))?;
                    let seg_rx = start_vad_pipeline(audio_rx)?;
                    asr::start_moonshine_asr(files, seg_rx)?
                }
                "funasr_nano" => {
                    let files = model_paths
                        .funasr_nano
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("FunASR Nano not downloaded"))?;
                    let seg_rx = start_vad_pipeline(audio_rx)?;
                    asr::start_funasr_nano_asr(files, seg_rx)?
                }
                // ── Fallback: first available streaming transducer ───
                _ => {
                    let files = model_paths.transducers.values().next().ok_or_else(|| {
                        anyhow::anyhow!("No streaming transducer model available for '{}'", chosen)
                    })?;
                    asr::start_streaming_asr(files, audio_rx)?
                }
            }
        };

        // 4. Spawn task to persist transcripts
        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();

        tokio::spawn(async move {
            let mut transcript_rx = transcript_rx;
            loop {
                tokio::select! {
                    result = transcript_rx.recv() => {
                        match result {
                            Some(t) => {
                                if t.text.trim().is_empty() {
                                    continue;
                                }
                                let start_ms = (t.start_sample as i64 * 1000) / 16000;
                                let end_ms = (t.end_sample as i64 * 1000) / 16000;

                                if t.is_final {
                                    if let Err(e) = aggregator.add_final(
                                        session_id, &t.text, start_ms, end_ms, None,
                                    ).await {
                                        warn!(%session_id, error = %e, "Failed to persist final transcript");
                                    }
                                } else {
                                    // Partials: broadcast to WS only, no DB persist
                                    aggregator.broadcast_partial(&t.text, start_ms, end_ms);
                                }
                            }
                            None => break, // ASR channel closed
                        }
                    }
                    _ = &mut cancel_rx => {
                        info!(%session_id, "Pipeline cancelled");
                        break;
                    }
                }
            }
            info!(%session_id, "Transcript persistence task ended");
        });

        self.capture_handle = Some(capture_handle);
        self.cancel_tx = Some(cancel_tx);

        info!(%session_id, "Inference pipeline started");
        Ok(())
    }

    /// Stop the pipeline (called on session end)
    pub fn stop(&mut self) {
        if let Some(cancel) = self.cancel_tx.take() {
            let _ = cancel.send(());
        }
        self.capture_handle.take(); // Drop stops the stream
    }

    pub fn is_running(&self) -> bool {
        self.capture_handle.is_some()
    }
}

impl Default for InferencePipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for InferencePipeline {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Interpose between VAD output and the downstream (offline-ASR) consumer.
/// Each `SpeechSegment` is forwarded unchanged to the returned receiver AND
/// a detached task is spawned to extract a speaker embedding, identify the
/// speaker, and persist an `audio_segments` row.
///
/// When the embedding model is absent, the task still writes a segment row
/// with a NULL speaker so retroactive tagging has something to point at.
fn split_segments_for_speaker_id(
    mut upstream: mpsc::Receiver<SpeechSegment>,
    session_id: Uuid,
    tenant_id: Uuid,
    pool: sqlx::SqlitePool,
    embedding_model: Option<PathBuf>,
) -> mpsc::Receiver<SpeechSegment> {
    let (tx, rx) = mpsc::channel::<SpeechSegment>(32);
    tokio::spawn(async move {
        while let Some(seg) = upstream.recv().await {
            let start_ms = (seg.start_sample as i64 * 1000) / 16000;
            let end_ms = (seg.end_sample as i64 * 1000) / 16000;
            let audio_clone = seg.audio.clone();
            let pool_c = pool.clone();
            let emb_c = embedding_model.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_segment_embedding(
                    &pool_c, emb_c, session_id, tenant_id, start_ms, end_ms, audio_clone,
                )
                .await
                {
                    warn!(%session_id, error = %e, "segment speaker-id hook failed");
                }
            });

            if tx.send(seg).await.is_err() {
                break; // downstream ASR consumer went away
            }
        }
    });
    rx
}

/// Persist a single VAD segment along with its speaker identification.
///
/// Order of operations:
/// 1. If no embedding model is loaded: insert the row with NULL speaker + NULL
///    embedding so the user can still retroactively tag it.
/// 2. Extract the embedding; on failure, insert the row with NULL speaker and
///    NULL embedding (same outcome as step 1 for this segment).
/// 3. Run `identify_speaker` against the tenant's enrolled voiceprints. If
///    `accepted`, write the matched speaker id; otherwise NULL. In both cases
///    the embedding is persisted on the row so it can be promoted later.
async fn handle_segment_embedding(
    pool: &sqlx::SqlitePool,
    embedding_model: Option<PathBuf>,
    session_id: Uuid,
    tenant_id: Uuid,
    start_ms: i64,
    end_ms: i64,
    audio: Vec<f32>,
) -> anyhow::Result<Option<String>> {
    let Some(model_path) = embedding_model else {
        crate::repository::segment::insert_segment(
            pool, session_id, start_ms, end_ms, None, None, None,
        )
        .await?;
        return Ok(None);
    };

    let emb = match crate::engine::diarization::extract_embedding(&model_path, &audio).await {
        Ok(e) => e,
        Err(err) => {
            warn!(?err, "speaker embedding failed; segment marked UNKNOWN");
            crate::repository::segment::insert_segment(
                pool, session_id, start_ms, end_ms, None, None, None,
            )
            .await?;
            return Ok(None);
        }
    };

    let result =
        crate::domain::speaker_matcher::identify_speaker(pool, &emb.values, tenant_id, 5)
            .await
            .unwrap_or(crate::domain::speaker_matcher::SpeakerMatchResult {
                speaker_id: None,
                similarity_score: 0.0,
                z_norm_score: 0.0,
                accepted: false,
            });

    let speaker_id = result
        .speaker_id
        .as_ref()
        .and_then(|s| Uuid::parse_str(s).ok());

    crate::repository::segment::insert_segment(
        pool,
        session_id,
        start_ms,
        end_ms,
        speaker_id,
        Some(result.similarity_score),
        Some(&emb.values),
    )
    .await?;

    Ok(result.speaker_id)
}

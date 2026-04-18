# Speaker Diarization & Voiceprint Enrollment — Design

Date: 2026-04-17
Status: Draft (awaiting user review)
Scope: backend + frontend + one schema migration

## 1. Goal

Let users add people in the People tab and have each person's voiceprint stored in the local database so that, during live sessions, transcripts can be tagged with the right speaker's name. Support two enrollment paths:

- **A. Record-now enrollment** — at "Add person" time, capture three short clips, extract one embedding per clip, persist them.
- **C. Retroactive tagging** — when a session produced `[UNKNOWN]` segments, the user assigns those segments to an existing or new speaker later; the segment's already-extracted embedding becomes that speaker's voiceprint with no re-recording.

Out of scope:

- Cloud ASR fallback.
- Real-time overlapping-speaker detection (the design leans on single-speaker-at-a-time VAD segments).
- Multi-tenant auth (tenant plumbing exists; no auth layer is added here).
- Manage-voiceprints UI (per-embedding delete / set-primary). Voiceprint library management stays minimal in v1.

## 2. Current Reality (what we build on)

- All ML inference is Rust-native via `sherpa-onnx` — no Python, no gRPC.
- Live pipeline (`engine/inference_pipeline.rs`) is `audio_capture → VAD → ASR → transcript_aggregator`. **No per-segment speaker embedding is extracted today.**
- Two offline helpers exist in `engine/diarization.rs`:
  - `diarize_audio(seg_model, emb_model, audio, num_speakers)` — full-buffer pyannote segmentation + embedding + clustering. Returns cluster-labelled segments (0, 1, 2…), not identities.
  - `extract_embedding(emb_model, audio)` — single-clip embedding extraction. Comment: `// Used for enrollment`. This is the enrollment primitive.
- Active DB schema: `backend/actio-core/migrations/001_initial_schema.sql` (SQLite). The separate `backend/migrations/` directory (10 Postgres-era files) is dead and gets deleted as housekeeping.
- `domain/speaker_matcher.rs` is currently broken — its SQL references a non-existent `e.embedding_distance` column, and `save_embedding` writes a stringified vector into a `BLOB` column with a hardcoded 192 dimension. It needs a real rewrite.

## 3. Architecture

```
Frontend                           Backend                         Storage
━━━━━━━━                           ━━━━━━━                         ━━━━━━━
PeopleTab.tsx           ─────┐
VoiceprintRecorder.tsx  ─────┤
UnknownSpeakerPanel.tsx ─────┼──► POST   /speakers/{id}/enroll ─► speaker_embeddings
AssignSpeakerPicker.tsx ─────┤    POST/GET/PATCH/DELETE /speakers     (BLOB f32 LE)
RecordingTab.tsx        ─────┤    GET    /sessions/{id}/unknowns
                             │    GET    /unknowns              ─► audio_segments
use-voice-store.ts      ─────┤    POST   /segments/{id}/assign      (.speaker_id,
use-media-recorder.ts   ─────┤    POST   /segments/{id}/unassign     .embedding BLOB)
                             │
                             │            │
                             │            ▼
                             │    engine::diarization::
                             │    extract_embedding
                             │    (sherpa-onnx ERes2Net,
                             │     ~100–300ms per clip)
                             │            │
                             │            ▼
                             │    domain::speaker_matcher
                             │    (cosine + Z-Norm in Rust)

                 speakers(+color col)  ──  speaker_embeddings  ──  audio_segments(+embedding col)
```

Key invariants:

- Enrollment calls `extract_embedding` once per uploaded clip. No clustering.
- Live session identification is a **new** hook: after each VAD segment completes, call `extract_embedding` on the segment audio, store the embedding on the segment row, run `speaker_matcher::identify_speaker`, and update the transcript's `speaker_id`. Unknown segments retain the embedding for later retroactive tagging.
- Retroactive tagging is a pure DB operation: copy the segment's embedding to `speaker_embeddings` under the chosen speaker; update the FK.

## 4. Data Model

New migration `backend/actio-core/migrations/002_speaker_diarization.sql`:

```sql
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

No FK behavior change (SQLite can't ALTER FK). Cascade/SET-NULL semantics on speaker delete are enforced in application code inside the `DELETE /speakers/{id}` handler.

Dead-code removal in the same branch:

- Delete `backend/migrations/` (10 Postgres-era files, unreferenced).

## 5. Backend API

### Existing routes (semantics finalised)

```
POST   /speakers
  Body: { display_name, color }
  → 201 Speaker

GET    /speakers
  → [Speaker, ...] filtered by tenant

PATCH  /speakers/{id}
  Body: { display_name?, color? }
  → 200 Speaker

DELETE /speakers/{id}
  Steps:
    1. UPDATE audio_segments SET speaker_id = NULL WHERE speaker_id = ?
    2. DELETE FROM speakers WHERE id = ?  (cascades to speaker_embeddings)
  → 204
```

### `POST /speakers/{id}/enroll`

```
Content-Type: multipart/form-data
Query: ?mode=replace   (v1 default and only supported mode)
Parts:  clip_0, clip_1, ... each a WAV (16 kHz mono preferred; backend resamples
        and downmixes as needed)

Flow:
  1. For each clip (no DB writes yet):
     - Decode → f32 PCM 16 kHz mono
     - Validate 3s ≤ duration ≤ 30s (below 3s → skip with warning)
     - engine::diarization::extract_embedding
     - quality_score = f(RMS, SNR, duration) in [0, 1]
  2. If no valid embeddings produced → 400 no_valid_clips (existing rows untouched).
  3. In a single transaction:
     - If mode=replace: DELETE FROM speaker_embeddings WHERE speaker_id = ?
     - INSERT all new embeddings (first → is_primary)
     - COMMIT (or ROLLBACK together on any DB error)

This ordering ensures a failed extraction never destroys the user's existing
voiceprint.

Response 201:
  {
    "speaker_id": "...",
    "embeddings": [
      { "id": "...", "duration_ms": 7420, "quality_score": 0.82,
        "is_primary": true }
    ],
    "warnings": ["Clip 2 skipped: duration 1.1s < 3s minimum"]
  }

Errors:
  400 no_valid_clips          — all parts failed validation
  409 embedding_model_missing — user needs to download ERes2Net model
  500 extraction_failure      — sherpa-onnx returned None
```

### Unknown / retroactive tagging

```
GET /sessions/{id}/unknowns
  → [
      { segment_id, start_ms, end_ms,
        transcript_excerpt: "first ~100 chars",
        cluster_label?: int }
    ]

GET /unknowns?limit=50
  → same shape + session_id, started_at

POST /segments/{id}/assign
  Body: { speaker_id: "<uuid>" }
      | { new_speaker: { display_name, color } }
  Flow:
    1. Resolve or create speaker
    2. UPDATE audio_segments SET speaker_id = ? WHERE id = ?
    3. If audio_segments.embedding IS NOT NULL:
         speaker_matcher::save_embedding(
           speaker_id, embedding, duration_ms, quality_score,
           is_primary = (no existing primary for this speaker)
         )
    4. (optional) UPDATE transcripts text rewriting [UNKNOWN] → display_name
       for that segment
  → 200 { speaker_id, segment_id, embedding_added: bool }

POST /segments/{id}/unassign
  → 200 — sets audio_segments.speaker_id = NULL
```

`ApiDoc` in `api/mod.rs` grows to cover the new routes and schemas (`EnrollResponse`, `UnknownSegment`, `AssignSegmentRequest`).

## 6. Live-session Hook (new work)

In `engine/inference_pipeline.rs`, after each VAD segment completes:

1. Clone the segment's audio buffer.
2. Spawn a task: `extract_embedding(model, audio)` + `speaker_matcher::identify_speaker(pool, &emb, tenant_id, k=5)`.
3. Persist the segment row with `speaker_id` (or NULL), `embedding` BLOB, `embedding_dim`.
4. Push a WebSocket event `{ kind: "speaker_resolved", segment_id, speaker_id }` to the client so inline transcript tags update.

This hook is gated by the embedding model being available. If not, the pipeline runs unchanged and all segments stay `[UNKNOWN]` — no errors.

## 7. Frontend

### Files

```
frontend/src/
├── api/speakers.ts                 NEW — typed client
├── hooks/use-media-recorder.ts     NEW — MediaRecorder + AudioWorklet wrapper
├── components/
│   ├── PeopleTab.tsx               MAJOR — backend-backed
│   ├── RecordingTab.tsx            MINOR — unknown-chip click → picker
│   ├── VoiceprintRecorder.tsx      NEW
│   ├── UnknownSpeakerPanel.tsx     NEW
│   └── AssignSpeakerPicker.tsx     NEW
├── store/use-voice-store.ts        CHANGED — speakers[] + unknowns[]
└── types/speaker.ts                NEW
```

### `useVoiceStore` shape

```ts
interface VoiceState {
  speakers: Speaker[];
  speakersStatus: 'idle' | 'loading' | 'ready' | 'error';
  unknowns: UnknownSegment[];
  dismissedUnknowns: Set<string>;

  fetchSpeakers(): Promise<void>;
  createSpeaker(input): Promise<Speaker>;
  updateSpeaker(id, patch): Promise<void>;
  deleteSpeaker(id): Promise<void>;
  enrollSpeaker(id, clips: Blob[]): Promise<EnrollResult>;

  fetchUnknowns(): Promise<void>;
  assignSegment(segmentId, target): Promise<void>;
  dismissUnknown(segmentId): void;
}
```

The old `people[]` / `addPerson` / `updatePerson` / `deletePerson` are removed. Any remaining references become compile errors, which is how we find all the call sites.

### PeopleTab states

1. **Loading** — skeleton rows.
2. **Empty** — `Add person` button.
3. **Normal** — SpeakerRow list + `+ Add person` + `Unidentified voices (N)` collapsible panel.
4. **Backend unreachable** — "Backend required to manage speakers".

### Add / edit / re-enroll

- "Add person" → inline name+color form → save (no voiceprint yet, "Needs voiceprint" badge) → auto-prompt `<VoiceprintRecorder>`.
- Edit row → same form, PATCH. Separate "Re-enroll voiceprint" button opens `<VoiceprintRecorder mode="replace" />`.
- Re-enroll always replaces (mode A from Q5).

### `<VoiceprintRecorder>`

Three-clip capture. Cycles three short passages (static, chosen for phonetic coverage). Per clip: target 8–12s, hard max 20s, min 3s. Visual RMS meter + countdown. After clip 3 or "Done" with ≥1 clip, POST all clips as one multipart request.

### Audio pipeline

```
getUserMedia { sampleRate: 16000, channelCount: 1,
               echoCancellation, noiseSuppression }
  → AudioContext
  → AudioWorkletNode (custom pcm-capture worklet)
  → ring buffer of f32 samples
  → on stop: resample to 16 kHz if needed (linear interp)
  → 16-bit mono WAV encode (tiny inline writer)
  → Blob (audio/wav)
```

Tauri desktop uses the same webview path; no native recording code. Microphone capability is already configured for the dictation feature (see commit `d3dbb0b`).

### `<UnknownSpeakerPanel>` + `<AssignSpeakerPicker>`

- Panel inside PeopleTab, populated from `GET /unknowns?limit=50`.
- Each row: playback (if `audio_ref` available, else text-only), transcript excerpt, session context, "Assign to…" / "Not a person" actions.
- "Assign to…" opens the shared picker (search existing speakers, or "Create new person" at the top). On pick, calls `POST /segments/{id}/assign`. On success, optimistic removal; error reverts.
- "Not a person" is a client-side soft-hide (`dismissedUnknowns`) — survives session but no backend action.

### Inline tagging

In `<RecordingTab>`, transcript lines are classified by their `speaker_id` field (the source of truth), not by parsing the text column — which may still contain a literal `[UNKNOWN]` prefix written at creation time. A `null` `speaker_id` renders as a `[UNKNOWN]` chip. Tap → same picker, prefiltered by `cluster_label` if present. A non-null `speaker_id` looks up the speaker in the store and renders name + color.

## 8. Error Handling

| Scenario | Behavior |
|----------|----------|
| Embedding model not downloaded | 409 `embedding_model_missing`; frontend shows one-click download via `/settings/models/download`. |
| All clips < 3s | 400 `no_valid_clips` with per-clip warnings. Speaker stays flagged "Needs voiceprint". |
| One clip low quality | Saved anyway; response `warnings[]` renders "poor quality" pill. |
| Duplicate names | Allowed. Disambiguated by id in UI. |
| Speaker deleted mid-session | App-level UPDATE nulls `audio_segments.speaker_id` before DELETE. Prior transcript text (which may already contain the name) is left alone. |
| Concurrent assign on same segment | Idempotent overwrite; no 409. |
| Backend restart mid-enroll | Write happens after all extractions succeed; partial state impossible. |
| Frontend cache stale | v1: 10s polling of `/unknowns` while panel is visible. WS event `speaker_assigned` is a flagged future enhancement. |
| Embedding dimension changes | `identify_speaker` filters candidates by matching dim; old rows at stale dim are ignored. UI banner: "Re-enroll required". |
| Tenant mismatch | API returns 404 if target speaker's tenant ≠ request tenant. |

No circuit breaker. No retries. All failure modes are user-recoverable or logged.

## 9. Test Plan

### Backend (Rust)

- `save_embedding` f32 → BLOB → f32 round-trip is byte-exact (bytemuck).
- `identify_speaker` returns correct id when cosine > Z-Norm threshold; returns None when below; ignores rows with mismatched `embedding_dimension`.
- `enroll_speaker` integration (in-memory SQLite + stubbed `extract_embedding`):
  - 3 valid clips → 3 rows, first `is_primary`.
  - 2 valid + 1 short → 2 rows + warning.
  - `mode=replace` deletes prior embeddings.
- `DELETE /speakers/{id}` leaves segments with `speaker_id IS NULL`; embeddings gone.
- `POST /segments/{id}/assign` with embedding → speaker gains the embedding; without embedding → FK only.
- Migration `002` applies against `001`-populated DB; re-apply is a no-op.

### Frontend (Vitest)

- `use-voice-store`: fetch/create/delete with optimistic updates and error rollback (MSW-style mocks).
- `PeopleTab` renders each state; add-person flow end-to-end with mocked API.
- `VoiceprintRecorder` with a fake `MediaRecorder`; FormData has 3 named parts after 3 clips; graceful handling of `getUserMedia` rejection.
- `UnknownSpeakerPanel` renders from mock `/unknowns`; "Assign to…" triggers `assignSegment`; optimistic removal; error revert.
- `AssignSpeakerPicker` keyboard selection, inline "Create new person".

### Manual smoke

- Tauri: create speaker → enroll 3 clips → start session → speak → transcript tagged.
- Mic disconnect mid-enroll.
- Delete speaker with attributed segments.
- Inline `[UNKNOWN]` tag assignment.
- Regression pass: dictation and reminder extraction unaffected.

### Out of scope for CI

- Accuracy of speaker identification (benchmark artefact, not unit test).
- OS permission dialog flows.

## 10. Rollout / Risks

- **Model download dependency.** The `ERes2Net` embedding model is ~25 MB (per typical sherpa-onnx packs). First-time enrollment will require a download gated by the existing `/settings/models` UI. Spec assumes this is acceptable UX.
- **Live-identification CPU cost.** Per-segment embedding extraction adds ~100–300 ms on a mid-range CPU. Acceptable inside the existing `spawn_blocking` pool.
- **Broken matcher rewrite.** `speaker_matcher.rs` needs a full rewrite as part of this feature — not a tidy-up. Tracked as a first-stage task in the implementation plan.
- **Dead migrations deletion.** `backend/migrations/` removal is contained in this feature branch as a separate commit to keep the review clean.
- **No auth.** Tenants still nil-UUID everywhere. Cross-tenant leakage is prevented by query filtering but not by authz.

## 11. Follow-ups (explicit non-goals, flagged for later)

- WebSocket `speaker_assigned` event to replace `/unknowns` polling.
- Per-embedding management UI (list, delete, set-primary).
- Append-mode enrollment (`?mode=append`) for robustness to environment changes.
- Post-hoc cluster-assist on the unknown panel (run `diarize_audio` on the full session to group unknowns that are plausibly the same person, and let the user assign a whole cluster at once).
- Voiceprint quality threshold tuning + environment-aware calibration.

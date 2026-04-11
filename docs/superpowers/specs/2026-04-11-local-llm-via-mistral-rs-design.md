# Local LLM via mistral.rs — Design

**Date:** 2026-04-11
**Status:** Approved (pending user review of written spec)
**Authors:** Brainstormed with Claude (superpowers:brainstorming)

## Summary

Add a one-click local LLM deployment feature to Actio's settings. Users can download a small Qwen3.5 GGUF model and run it in-process via the [`mistral.rs`](https://github.com/EricLBuehler/mistral.rs) Rust crate, with the engine also exposed as an OpenAI-compatible HTTP endpoint that other local tools (Cursor, Zed, scripts) can hit. The local LLM coexists with the existing remote OpenAI-compatible LLM path, and the user picks which backend `todo_generator` uses via a settings radio.

## Motivation

Today, action-item extraction in Actio depends on a remote OpenAI-compatible API configured via `LLM_BASE_URL` / `LLM_API_KEY` environment variables. This means:

- The user must already have an API key from someone (OpenAI, Anthropic-via-proxy, a self-hosted server) before todo generation works at all.
- Their meeting transcripts get sent to a third-party server.
- There's no out-of-the-box experience.

A local LLM solves all three: zero-config after one click, no data leaves the machine, no API key needed. The remote path is preserved for users who want frontier-model quality.

## Locked-in decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Engine | `mistralrs` Rust crate, embedded in-process |
| Models in v1 catalog | `Qwen/Qwen3.5-0.8B` (default), `Qwen/Qwen3.5-2B` |
| Quantization | Q4_K_M GGUF, fixed (no user-facing knob) |
| Backends | CPU + Metal only (no CUDA / Vulkan / ROCm in v1) |
| Local vs remote | Coexist, user picks via radio in settings |
| HTTP endpoint | OpenAI-compatible `/v1/chat/completions` + `/v1/models`, localhost-only, no auth |
| Endpoint port | User-configurable, default 3000 (separate axum listener when port differs from actio backend) |
| Lifecycle | Lazy + sticky (load on first call, stay until app exit or model swap) |
| UI placement | New "Language Models" section in settings, distinct from existing "Speech Models" |
| Remote LLM config | Settings UI fields (base URL, API key, model name); env vars become bootstrap fallback for first launch only |
| Failure mode | Surface errors with clear next-action; no auto-fallback between local ↔ remote |
| Default selection | `Disabled` — user must explicitly opt in (no silent 500 MB download) |
| API key storage | Plain `settings.json` for v1 (OS keychain is v2) |

## Architecture

**One process, one engine, two consumers.**

```
                          ┌─────────────────────────────────────────────┐
                          │                actio-core process            │
                          │                                              │
   meeting transcript ──▶ │  todo_generator                              │
                          │       │                                      │
                          │       ▼                                      │
                          │  LlmRouter (new) ──┐                         │
                          │       │             │                        │
                          │       │             ├─▶ LocalLlmEngine (new) │
                          │       │             │     │                  │
                          │       │             │     │  mistralrs::Model│
                          │       │             │     │  (Qwen3.5 GGUF)  │
                          │       │             │     │  loaded once,    │
                          │       │             │     │  stays in RAM    │
                          │       │             │     │                  │
                          │       │             └─▶ RemoteLlmClient      │
                          │       │                  (existing, edited)  │
                          │       │                                      │
                          │  axum HTTP server (existing, port 3000)      │
                          │       │                                      │
                          │       ├─ /settings/...           (existing)  │
                          │       ├─ /settings/llm/...       (NEW)       │
                          │       └─ /v1/chat/completions    (NEW) ──────┼──▶ external clients
                          │          /v1/models                          │     (Cursor, Zed,
                          │          (handlers call LocalLlmEngine       │      curl, scripts)
                          │           directly — same instance as        │
                          │           todo_generator uses)               │
                          └─────────────────────────────────────────────┘
```

### Key properties

1. **`LocalLlmEngine` is owned by an `Arc<RwLock<Option<LocalLlmEngine>>>` (`EngineSlot`) held by app state.** `None` until first use. Lazy + sticky lifecycle: first call triggers `EngineSlot::get_or_load(model_id)`, which mmaps the GGUF and initializes mistral.rs. Subsequent calls reuse the loaded instance for the rest of the process lifetime. Switching models in settings drops the old engine and loads the new one.

2. **`LlmRouter` is a thin enum dispatcher**, not a trait object: `enum LlmBackend { Local { slot, model_id }, Remote(RemoteLlmClient), Disabled }`. `todo_generator` calls `router.generate_todos(transcript)`; the router picks the right path based on the active selection.

3. **The `/v1/chat/completions` HTTP handler also goes through `LocalLlmEngine` directly** — it does **not** round-trip through `LlmRouter` and cannot be used to reach the user's configured remote LLM. External clients hitting `/v1` always use the local engine. If the local engine isn't loaded, `/v1` returns `503 Service Unavailable` with a clear message.

4. **`mistralrs` is gated behind a `local-llm` Cargo feature**, on by default. When the feature is off, `LocalLlmEngine` is a stub that always returns `LocalLlmError::FeatureDisabled` and the settings UI hides the local rows.

5. **Backend selection (CPU vs Metal) is build-time only** via Cargo features (`local-llm-metal` enables `mistralrs/metal` on macOS builds). No runtime backend probing — Tauri builds per platform, so the macOS installer ships Metal-enabled and Windows/Linux installers ship CPU-only.

## Data model

### New types

```rust
// engine/llm_catalog.rs (new)

#[derive(Debug, Clone, Serialize)]
pub struct LocalLlmInfo {
    pub id: String,                 // "qwen3.5-0.8b-q4km"
    pub name: String,               // "Qwen3.5 0.8B (Q4_K_M)"
    pub hf_repo: String,            // "Qwen/Qwen3.5-0.8B" (or "...-GGUF" — verify at impl time)
    pub gguf_filename: String,      // "qwen3.5-0.8b-q4_k_m.gguf"
    pub sha256: &'static str,       // for download integrity check
    pub size_mb: u32,               // ~500
    pub ram_mb: u32,                // ~700
    pub recommended_ram_gb: u32,    // 8
    pub context_window: u32,        // 32768
    pub description: String,
    pub downloaded: bool,           // filled at runtime
    pub runtime_supported: bool,
}

pub fn available_local_llms() -> Vec<LocalLlmInfo>;   // hardcoded catalog

// engine/llm_router.rs (new)

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LlmSelection {
    Disabled,
    Local { id: String },
    Remote,
}

impl Default for LlmSelection {
    fn default() -> Self { LlmSelection::Disabled }
}
```

### v1 catalog entries

| `id` | `hf_repo` | `gguf_filename` | size | RAM | rec'd RAM |
|---|---|---|---|---|---|
| `qwen3.5-0.8b-q4km` | `Qwen/Qwen3.5-0.8B` | `qwen3.5-0.8b-q4_k_m.gguf` | ~500 MB | ~700 MB | 8 GB |
| `qwen3.5-2b-q4km` | `Qwen/Qwen3.5-2B` | `qwen3.5-2b-q4_k_m.gguf` | ~1.4 GB | ~2.5 GB | 16 GB |

**Numbers are placeholders.** The first task in the implementation plan is a `WebFetch` against `huggingface.co/Qwen/Qwen3.5-0.8B`, `huggingface.co/Qwen/Qwen3.5-0.8B-GGUF`, and the 2B equivalents to confirm:

- The actual repo that hosts the Q4_K_M GGUF file (base repo or `*-GGUF` sibling repo).
- The exact GGUF filename and SHA256.
- The actual on-disk and RAM footprint.

These get patched into the catalog before any other implementation work begins. The brainstorming session deliberately did not commit to URLs because Qwen3.5 postdates Claude's training cutoff and any URLs in this spec would be guesses.

### Settings schema additions

```rust
// In actio-core/src/engine/app_settings.rs Settings struct

pub struct LlmSettings {
    pub selection: LlmSelection,         // default: Disabled
    pub remote: RemoteLlmSettings,
    pub local_endpoint_port: u16,        // default: 3000
}

pub struct RemoteLlmSettings {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
}
```

### Defaults and migration

- `selection: Disabled` — no silent downloads, no mystery RAM use until the user explicitly picks a backend.
- `remote.{base_url,api_key,model}`: read from `LLM_BASE_URL` / `LLM_API_KEY` / `LLM_MODEL` env vars **once at first launch** (when the LLM section of settings.json is empty) and seeded into settings. After that, `settings.json` is the source of truth and env vars are ignored.
- `local_endpoint_port: 3000` — matches the actio HTTP server port; no separate listener is created until the user picks a different port (see "HTTP endpoint" below).

### Disk layout

```
{model_dir}/
  llms/
    qwen3.5-0.8b-q4km/
      qwen3.5-0.8b-q4_k_m.gguf
      tokenizer.json            (only if not embedded in GGUF)
    qwen3.5-2b-q4km/
      qwen3.5-2b-q4_k_m.gguf
      tokenizer.json
```

Same `{model_dir}` root as the existing ASR models, but a separate `llms/` subdirectory. The existing ASR `model_manager.rs` does not need to know about LLMs — `LocalLlmEngine` and `LlmDownloader` manage their own subtree.

## Catalog & download flow

### `LlmDownloader` (`engine/llm_downloader.rs` — new)

Mirrors the shape of `model_manager.rs` for consistency, but is a separate module — LLM downloads are not coupled to ASR `DownloadTarget` / `ModelStatus` enums.

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LlmDownloadStatus {
    Idle,
    Downloading {
        llm_id: String,
        progress: f32,
        bytes_downloaded: u64,
        bytes_total: u64,
    },
    Error { llm_id: String, message: String },
}

pub struct LlmDownloader {
    model_dir: PathBuf,
    status: Arc<RwLock<LlmDownloadStatus>>,
    /// Single-slot semaphore — one download at a time, app-wide.
    /// Shared with the ASR downloader so the user can't accidentally
    /// kick off two large downloads at once.
    download_lock: Arc<tokio::sync::Mutex<()>>,
}
```

### Behaviors

1. **One download at a time, app-wide.** The same `Mutex` is shared with the ASR downloader. Both downloaders take a reference to the same `Arc<Mutex<()>>` constructed at app startup. UI buttons display "Another download in progress…" in either direction. Matches the existing pattern at `frontend/src/components/settings/ModelSetup.tsx:231`.
2. **Atomic file writes.** Download to `*.gguf.partial`, fsync, rename to final name. Crash mid-download leaves a `.partial` file, which is detected on next launch and **deleted**. No HTTP `Range` resume in v1 — restart from scratch. (Listed as v2 candidate.)
3. **Hash verification after download.** Catalog `sha256` is checked before the file is marked `downloaded: true`. Mismatch → delete + report `LlmDownloadError::HashMismatch`.
4. **Progress updates** are written to `Arc<RwLock<LlmDownloadStatus>>` and polled by the frontend at 1 Hz (same pattern as `ModelSetup.tsx:70-75`).
5. **HF URL pattern:** `https://huggingface.co/{hf_repo}/resolve/main/{gguf_filename}`. The `hf_repo` is whatever repo actually hosts the Q4_K_M GGUF, which may be a `*-GGUF` sibling rather than the base model repo. Confirmed at impl time.

### Routes added (mounted on the actio backend listener, port 3000)

```
GET    /settings/llm                     → LlmSettings
PATCH  /settings/llm                     → update LlmSettings (selection / remote / port)
GET    /settings/llm/models              → Vec<LocalLlmInfo> with downloaded flags
POST   /settings/llm/models/download     → body: { llm_id }   starts download
DELETE /settings/llm/models/{id}         → deletes GGUF + dir, clears active selection if needed
GET    /settings/llm/download-status     → LlmDownloadStatus
POST   /settings/llm/test                → tests current selection (see "Test connection")
```

These mirror the existing `/settings/models/...` ASR routes.

## Engine wrapper & lifecycle

### `LocalLlmEngine` (`engine/local_llm_engine.rs` — new)

```rust
pub struct LocalLlmEngine {
    loaded_id: String,
    /// Behind a Mutex because mistral.rs's generation API is &mut self
    /// for KV-cache reasons. Serializes generation calls.
    inner: tokio::sync::Mutex<mistralrs::Model>,
    metadata: LocalLlmInfo,
}

impl LocalLlmEngine {
    /// Cold-load a GGUF model from disk into RAM.
    pub async fn load(model_dir: &Path, info: &LocalLlmInfo)
        -> Result<Self, LocalLlmError>;

    /// Single-shot completion. Used by todo_generator.
    pub async fn chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        max_tokens: usize,
        temperature: f32,
        json_mode: bool,
    ) -> Result<String, LocalLlmError>;

    /// Streaming completion. Used by /v1/chat/completions when stream:true.
    pub async fn chat_completion_stream(
        &self,
        messages: Vec<ChatMessage>,
        max_tokens: usize,
        temperature: f32,
    ) -> impl Stream<Item = Result<TokenChunk, LocalLlmError>>;

    pub fn loaded_id(&self) -> &str { &self.loaded_id }
    pub fn metadata(&self) -> &LocalLlmInfo { &self.metadata }
}
```

### `EngineSlot` (lazy-sticky lifecycle holder)

```rust
pub struct EngineSlot {
    model_dir: PathBuf,
    catalog: Vec<LocalLlmInfo>,
    /// None until first successful load. Some(engine) once loaded.
    /// Outer Mutex protects load/swap; inner engine has its own
    /// Mutex for generation calls.
    current: tokio::sync::Mutex<Option<Arc<LocalLlmEngine>>>,
}

impl EngineSlot {
    /// Returns the engine for `desired_id`, loading it if needed,
    /// or swapping the loaded model if a different one is desired.
    pub async fn get_or_load(&self, desired_id: &str)
        -> Result<Arc<LocalLlmEngine>, LocalLlmError>;

    /// Explicitly drops the loaded engine. Called on settings change,
    /// model deletion, or app shutdown.
    pub async fn unload(&self);
}
```

**Two non-obvious choices:**

1. **Inner `Mutex<mistralrs::Model>` serializes generation calls.** If two requests arrive simultaneously (e.g. `todo_generator` and an external `/v1` client), the second waits. This is correct — mistral.rs's KV cache is per-instance and interleaving would corrupt state. For v1, single-flight is fine. If concurrent generation becomes a real need, the answer is multiple `LocalLlmEngine` instances, **not** multiplexing one.

2. **Drop-old-before-load-new on model swap.** When the user switches from 0.8B to 2B, `EngineSlot::get_or_load` sets `current = None` (releasing ~700 MB) *before* loading the new model (allocating ~2.5 GB). The alternative — load new, then drop old — would briefly require ~3.2 GB peak. On an 8 GB laptop that could OOM. The cost of drop-first is 1–3 s during which the engine is unavailable, which is invisible because nothing is calling it during a settings change.

### Loading runs on `tokio::task::spawn_blocking`

mmap fault-in plus KV cache allocation is up to ~3 s on a mid-tier laptop (per warm-up estimates from brainstorming). Doing this on the async runtime would block other tasks. `EngineSlot::get_or_load` puts the heavy work on a blocking thread.

### Integration into `todo_generator`

Today's `engine/llm_client.rs` is renamed to `engine/remote_llm_client.rs`; its `LlmClient` becomes `RemoteLlmClient` and its `LlmError` becomes `RemoteLlmError`. The system prompt and message-building logic move into a shared `engine/llm_prompt.rs::build_todo_messages(transcript) -> Vec<ChatMessage>` so both backends format prompts identically. This is a small but important refactor: **without it, behavior could drift between local and remote paths.**

```rust
// engine/llm_router.rs

pub enum LlmRouter {
    Disabled,
    Local { slot: Arc<EngineSlot>, model_id: String },
    Remote(RemoteLlmClient),
}

impl LlmRouter {
    pub async fn generate_todos(&self, transcript: &str)
        -> Result<Vec<LlmTodoItem>, LlmRouterError>
    {
        match self {
            LlmRouter::Disabled => Ok(vec![]),
            LlmRouter::Remote(client) => client.generate_todos(transcript).await
                .map_err(LlmRouterError::Remote),
            LlmRouter::Local { slot, model_id } => {
                let engine = slot.get_or_load(model_id).await
                    .map_err(LlmRouterError::Local)?;
                let messages = build_todo_messages(transcript);
                let json = engine.chat_completion(
                    messages, 2000, 0.1, true,
                ).await.map_err(LlmRouterError::Local)?;
                let parsed: LlmTodoResponse = serde_json::from_str(&json)
                    .map_err(|e| LlmRouterError::Parse(e.to_string()))?;
                Ok(parsed.todos)
            }
        }
    }
}
```

`LlmRouter` is constructed once at app startup from the current `LlmSettings.selection` and rebuilt whenever settings change. **When the rebuild transitions away from `Local` (i.e. to `Remote` or `Disabled`), the settings-change handler also calls `EngineSlot::unload()` to release the loaded model from RAM** — otherwise a 2.5 GB engine would sit warm forever after the user opted out.

### Lifecycle event sequence (happy path)

1. App starts. `EngineSlot::current = None`. No mistral.rs in RAM.
2. User opens Settings → Language Models. UI calls `GET /settings/llm/models`. Catalog returned, both entries `downloaded: false`.
3. User clicks Download on Qwen3.5-0.8B. UI calls `POST /settings/llm/models/download { llm_id }`. Download starts; UI polls `/settings/llm/download-status` at 1 Hz. File is hashed and atomically renamed.
4. User picks the downloaded model via radio. UI calls `PATCH /settings/llm { selection: { kind: "local", id: "..." } }`. `LlmRouter` rebuilt. **Engine still not loaded.**
5. User finishes a meeting. `todo_generator` calls `router.generate_todos(transcript)`. Router calls `slot.get_or_load("qwen3.5-0.8b-q4km")`. Cold load on a blocking thread, ~1–3 s. Engine returned.
6. Engine generates JSON, ~25–50 s for a typical 3000-token transcript. Todos saved.
7. Second meeting in the same session. Step 5 short-circuits — engine already loaded. No reload.
8. User picks Qwen3.5-2B in settings. `LlmRouter` rebuilt. Next `generate_todos` call: `get_or_load("qwen3.5-2b-q4km")` sees a different `loaded_id`, drops the old engine, loads the new one, proceeds.
9. App exits. `EngineSlot` dropped. mistral.rs releases everything.

## HTTP endpoint & user-configurable port

### Two listeners, two ports (when port differs)

The actio HTTP server stays on a fixed port (3000) for `/settings/...` and other internal routes. A **second** axum listener is bound to `127.0.0.1:{local_endpoint_port}` and serves only `/v1/chat/completions` and `/v1/models`, sharing the same `EngineSlot`. The user-configurable port is the second listener.

**Why not single-listener?** Collapsing "LLM port" into "actio backend port" would technically honor the configurable-port request but defeat the actual use case (pointing tools like Cursor at a port like `11434` for Ollama-compat detection). The extra listener is small in absolute terms.

**When `local_endpoint_port == backend_port` (the default):** the second listener is **not** started. Instead, the `/v1/*` routes are mounted on the backend listener so curl on `:3000/v1/...` still works. When the ports differ, the backend listener removes its `/v1/*` routes and the second listener gets them — the contract "the LLM endpoint is at the configured port, period" is preserved.

### Routes

**On the actio backend listener (port 3000, internal):**
```
GET    /settings/llm
PATCH  /settings/llm
GET    /settings/llm/models
POST   /settings/llm/models/download
DELETE /settings/llm/models/{id}
GET    /settings/llm/download-status
POST   /settings/llm/test
```

**On the LLM endpoint listener (default 3000, changeable):**
```
GET   /v1/models
POST  /v1/chat/completions
```

### OpenAI-compat scope for v1

**Supported on `/v1/chat/completions`:**
- Request body: `messages`, `model`, `max_tokens`, `temperature`, `stream` (bool), `response_format: { type: "json_object" }`
- SSE streaming (`stream: true`) with `data: {...}\n\n` + `data: [DONE]\n\n` framing
- `Authorization: Bearer ...` header is **accepted but ignored** — no auth enforcement, but tools that always send a key don't error out

**Supported on `/v1/models`:**
- Lists the **currently-loaded** model only, or empty if nothing is loaded. **Not** all catalog entries — only what mistral.rs is actually serving.

**Not supported in v1 (return `400 Not Implemented` or appropriate error):**
- `tools` / function calling
- `logprobs`, `n > 1`, `logit_bias`, `presence_penalty`, `frequency_penalty`
- `/v1/completions` (legacy non-chat endpoint)
- `/v1/embeddings` (different model, different runtime path)

If a client sends a `model` field that doesn't match the loaded model id, return `404 { "error": { "message": "model X not loaded; loaded model is Y" } }`. **We never hot-swap models in response to client requests** — that would let an external tool clobber the user's settings choice.

### Frontend impact

`API_BASE` in the frontend stays `http://127.0.0.1:3000` and is **not** affected by the LLM endpoint port. The frontend never talks to `/v1/*`. It only reads/writes `/settings/llm/*`. The configurable port is purely for external clients.

## UI changes

A new `LanguageModelSetup.tsx` component sits in the settings page beneath the existing `ModelSetup.tsx` (Speech Models) section. **No reuse of `ModelSetup.tsx`** — the data shapes (`AsrModelInfo` vs `LocalLlmInfo`), radio semantics (per-language vs one-of), and error displays are different enough that a shared `<ModelList>` generic would cost more than it saves. Keep them parallel and copy the small bits that overlap (progress bar markup, delete confirmation dialog).

### Layout

```
┌─ Language Models ─────────────────────────────────────────────────┐
│                                                                    │
│  Backend                                                           │
│  ○ Disabled — Action item extraction is off                        │
│  ○ Local — Run a model on this machine                             │
│  ○ Remote — Use an OpenAI-compatible API                           │
│                                                                    │
│  ─── (only if Local selected) ───────────────────────────────────  │
│                                                                    │
│  Local model                                                       │
│  ○ Qwen3.5 0.8B (Q4_K_M)              ✓ Downloaded   [Delete]     │
│    ~500 MB on disk · ~700 MB RAM · 8 GB+ recommended               │
│    Smallest, fastest. Recommended for most laptops.                │
│                                                                    │
│  ○ Qwen3.5 2B (Q4_K_M)                          [Download 1.4 GB]  │
│    ~1.4 GB on disk · ~2.5 GB RAM · 16 GB+ recommended              │
│    Better quality. Recommended for 16+ GB RAM.                     │
│                                                                    │
│  Endpoint                                                          │
│  Port: [3000   ]   Apply                                           │
│  Other tools can reach this at:  http://127.0.0.1:3000/v1          │
│  ⓘ Currently sharing the actio backend port. Pick a different      │
│    port to expose the LLM separately.                              │
│                                                                    │
│  ─── (only if Remote selected) ──────────────────────────────────  │
│                                                                    │
│  Base URL  [https://api.openai.com/v1            ]                 │
│  API key   [••••••••••••••••••••••••••••••••    ] [Show]          │
│  Model     [gpt-4o-mini                          ]                 │
│  [Test connection]   ✓ Connected · responded in 412 ms             │
│                                                                    │
└────────────────────────────────────────────────────────────────────┘
```

### Component state

Four state slices:

1. **`settings: LlmSettings`** — fetched from `GET /settings/llm`, mutated by inputs, written back via `PATCH /settings/llm`. Optimistic updates with rollback on PATCH failure.
2. **`models: LocalLlmInfo[]`** — fetched from `GET /settings/llm/models`. Refreshed after every download / delete.
3. **`downloadStatus: LlmDownloadStatus`** — polled at 1 Hz while `state === 'downloading'`.
4. **`testResult: null | { ok: true; ms } | { ok: false; error }`** — set by clicking "Test connection".

### Specific UI behaviors

1. **Backend radio is the master toggle.** Picking "Disabled" hides the local-model list and remote-config form. Selection is persisted on click via `PATCH /settings/llm` — no save button.
2. **Local model radio is disabled when not downloaded.** Same pattern as `ModelSetup.tsx:188`. The user clicks `[Download]` first; the radio becomes selectable once `downloaded: true`.
3. **Delete on the currently-selected model warns and clears selection.** "Delete X? It is currently selected — action item extraction will be disabled until you pick another model." On confirm: `DELETE /settings/llm/models/{id}`, then `PATCH /settings/llm { selection: { kind: 'disabled' } }`. Mirrors `ModelSetup.tsx:124-135`.
4. **Port input is debounced + has an explicit Apply button.** Auto-applying on every keystroke would tear down and rebind the listener constantly. Validation: integer 1024–65535. On Apply: `PATCH /settings/llm { local_endpoint_port: N }`. Backend tries to bind; success returns 200, failure returns `400 { error: "port N is already in use by another process" }` and the input goes red.
5. **The endpoint URL string updates live based on the port input** (not waiting for Apply). Pure display, no requests fired.
6. **The "currently sharing the actio backend port" hint is conditional.** Shown only when `local_endpoint_port == 3000`. When the ports differ, replaced with: "✓ LLM endpoint is on a separate port. The actio backend remains on port 3000."
7. **API key input uses `type="password"` with a Show toggle.** No special storage — settings.json plaintext per the locked-in decisions. The masking is shoulder-surfing courtesy.
8. **`[Test connection]`** button POSTs to `/settings/llm/test`:
   - For `selection: local` — calls `EngineSlot::get_or_load` (triggering cold load if needed) and runs a tiny prompt like `"Reply with the single word 'ok'"`. Returns round-trip ms and response text.
   - For `selection: remote` — calls the configured remote endpoint with the same tiny prompt. Returns ms + response.
   - For `selection: disabled` — button is hidden.

### Out of scope for the UI

- No streaming preview / chat interface (this is a settings page, not a chat UI).
- No quantization picker (locked at Q4_K_M).
- No advanced expander for context length / temperature / system prompt.
- No download-resume UI (per the v1 simplification).

### CSS

Reuse the existing `settings-section`, `settings-field`, `model-list`, `model-progress`, `model-list__row`, `model-list__row--disabled`, `model-list__check`, `model-list__download-btn`, `model-list__delete-btn` classes from `globals.css`. New elements (backend radio group, port input, endpoint URL display, test-connection result line) get a small set of new classes prefixed `language-model-*`. No restyling of existing elements.

## Errors and failure modes

### Error taxonomy

```rust
#[derive(Debug, thiserror::Error)]
pub enum LocalLlmError {
    #[error("model {0} is not in the catalog")]
    UnknownModel(String),
    #[error("model {0} is not downloaded")]
    NotDownloaded,
    #[error("failed to load model: {0}")]
    LoadFailed(String),
    #[error("model file is corrupt or truncated: {0}")]
    CorruptModelFile(String),
    #[error("out of memory loading {model_id} (needed ~{needed_mb} MB)")]
    OutOfMemory { model_id: String, needed_mb: u32 },
    #[error("CPU does not support required instructions: {0}")]
    UnsupportedCpu(String),
    #[error("inference failed: {0}")]
    InferenceFailed(String),
    #[error("local LLM feature not compiled into this build")]
    FeatureDisabled,
}

#[derive(Debug, thiserror::Error)]
pub enum LlmRouterError {
    #[error("no LLM backend selected")]
    Disabled,
    #[error(transparent)]
    Local(#[from] LocalLlmError),
    #[error(transparent)]
    Remote(#[from] RemoteLlmError),
    #[error("failed to parse LLM response as JSON: {0}")]
    Parse(String),
}

#[derive(Debug, thiserror::Error)]
pub enum LlmDownloadError {
    #[error("another download is already in progress")]
    AlreadyInProgress,
    #[error("network error: {0}")]
    Network(String),
    #[error("hash mismatch — file corrupt, deleted")]
    HashMismatch,
    #[error("disk full or write failed: {0}")]
    DiskWrite(String),
    #[error("unknown model id {0}")]
    UnknownModel(String),
}
```

### HTTP status code mapping (settings routes)

| Error | Status | Body shape |
|---|---|---|
| `UnknownModel` | 404 | `{ "error": "..." }` |
| `NotDownloaded` | 409 | `{ "error": "model not downloaded — POST /settings/llm/models/download first" }` |
| `OutOfMemory` | 507 | `{ "error": "..." }` |
| `CorruptModelFile` | 500 | `{ "error": "..." }` (file is auto-deleted) |
| `UnsupportedCpu` | 500 | `{ "error": "..." }` |
| `LoadFailed` (other) | 500 | `{ "error": "..." }` |
| `InferenceFailed` | 500 | `{ "error": "..." }` |
| `FeatureDisabled` | 501 | `{ "error": "this build does not include local LLM support" }` |
| `Disabled` (router) | 503 | `{ "error": "no LLM backend selected" }` |
| `AlreadyInProgress` | 409 | `{ "error": "..." }` |

**The `/v1/chat/completions` route maps these errors to OpenAI-shaped error bodies — `{"error": {"message": "...", "type": "..."}}` — not the actio shape**, because OpenAI clients parse them. Settings routes use the actio `{"error": "..."}` shape.

### Failure-mode behaviors (user-visible)

1. **Cold load fails because GGUF is corrupt.** → `CorruptModelFile`. **Downloader auto-deletes the corrupt file** so the user can re-download without manually clicking Delete first. Settings UI red banner: *"Qwen3.5 0.8B failed to load — file appears corrupt and was deleted. Click Download to re-download."* Selection stays as-is.
2. **Cold load OOM.** → `OutOfMemory`. UI banner: *"Not enough memory to load Qwen3.5 2B (needed ~2500 MB). Try Qwen3.5 0.8B or close other applications."* No auto-switch.
3. **Cold load — unsupported CPU.** → `UnsupportedCpu`. UI banner: *"Your CPU lacks instructions required by the local LLM (AVX2). Use the Remote option instead."* Permanent failure.
4. **Inference fails mid-generation.** → `InferenceFailed`. Router propagates up to `todo_generator`, which logs and returns empty `Vec<LlmTodoItem>`. **Transcript is preserved.** Toast in meeting view: *"Action item extraction failed for this meeting. Check Settings → Language Models."* Same semantics as the existing remote LLM failure path.
5. **Inference returns invalid JSON.** **First-line defense:** mistral.rs supports constrained generation against a JSON schema — pass the `LlmTodoResponse` schema as a constraint so output is forced parseable. **Second-line defense (fallback if constrained generation isn't available for the loaded model):** one retry with a stricter prompt suffix ("Respond with valid JSON only. No prose."). If retry also fails, return empty todos and log the raw response. Same toast as #4.
6. **Download fails partway.** Network drop, server 5xx, disk full. Downloader deletes the `.partial`, surfaces error in UI status area. User clicks Download again.
7. **Download succeeds but hash mismatches.** Auto-delete file, set `HashMismatch`, surface in UI: *"Downloaded file failed integrity check. Try again."* Two consecutive mismatches → likely stale catalog SHA256, file a bug.
8. **User changes endpoint port to one in use.** PATCH fails at the bind step. Settings keep their old value (bind happens **before** persisting). UI shows `400 { error: "port N is already in use by another process" }` and input goes red.
9. **User changes endpoint port from 3000 to something else.** Tear down second listener if it exists, bind a new one, mount `/v1/*` routes on it. The actio backend listener removes its `/v1/*` routes since they now live on the new listener. **If teardown or rebind fails halfway, restore the previous state** — never end up with neither listener serving `/v1`.
10. **User deletes a model that's currently loaded in RAM.** DELETE handler calls `EngineSlot::unload()` first (drops mmap'd file handles), then removes the file. Without the explicit unload, deletion would fail on Windows (file in use) or succeed on Unix while the engine points at an orphaned inode.

### Logging (no new dependencies)

All of the above goes through the existing `tracing` infrastructure. New spans:

- `local_llm.load` — `model_id`, `duration_ms`, success/error
- `local_llm.inference` — `model_id`, `prompt_tokens`, `completion_tokens`, `duration_ms`, success/error
- `llm_download` — `llm_id`, `bytes_total`, `duration_ms`, success/error
- `llm_endpoint.request` — `route`, `status_code`, `duration_ms`

No metrics export, no telemetry beacon, no Sentry. Failures are diagnosed by reading the local log file.

## Testing strategy

### Unit tests (`actio-core`)

- `LlmRouter::generate_todos` dispatch — assert each `LlmSelection` variant routes correctly using mocked `LocalLlmEngine` and `RemoteLlmClient`. Mocks are fine here because the routing logic is pure dispatch with no I/O.
- `llm_catalog::available_local_llms` returns expected entries with expected ids.
- `LlmDownloader` happy path against a mocked HTTP server (`wiremock` or similar) — including hash verification, atomic rename, partial-file cleanup.
- Error mapping — every `LocalLlmError` / `LlmDownloadError` variant produces the expected HTTP status code via the route handlers.
- JSON parsing fallback — feed malformed JSON to the response parser and assert the retry-with-stricter-prompt path runs and a final empty result is returned without panicking.
- Settings migration — first launch with `LLM_BASE_URL` env var set seeds `LlmSettings.remote.base_url` exactly once; subsequent launches ignore the env var.

### Integration tests (`actio-core`, gated `#[ignore]`)

- Cold-load Qwen3.5-0.8B from a checked-in tiny GGUF fixture (NOT the real 500 MB file — use a small test model from mistral.rs's own test suite if available, otherwise skip in CI).
- End-to-end download → load → inference → unload cycle. Asserts no file handles leak.
- Engine swap: load 0.8B, swap to 2B, swap back. Manual smoke (RSS measurement is fragile in tests).

### Manual smoke checklist (lives in the implementation plan)

- Download Qwen3.5-0.8B on a clean install. Verify file appears in `{model_dir}/llms/qwen3.5-0.8b-q4km/` with correct size and hash.
- Pick it as active. Run a meeting. Verify todos are generated.
- Switch to Qwen3.5-2B. Verify previous model unloads before new one loads.
- `curl http://127.0.0.1:3000/v1/chat/completions` from the command line — verify OpenAI-shaped response.
- Change LLM endpoint port to 11434. Verify second listener starts on 11434 and `curl :11434/v1/...` works while `curl :3000/v1/...` returns 404.
- Curl with `stream: true` and verify SSE framing.
- Configure remote: paste OpenAI API key, click Test Connection, verify ✓.
- Configure remote with wrong API key, click Test, verify clear error message.
- Pull the network cable mid-download, verify partial file is cleaned up and error is shown.
- Rapidly toggle the backend radio between Disabled / Local / Remote — no crashes, settings always reflect the last click.
- Quit and relaunch — verify lazy load: nothing in RAM until the first todo generation call.

## Non-goals (explicit, for v1)

These are deliberately out of scope so the implementation plan doesn't drift into them.

1. **OS keychain storage for the remote API key.** Plain `settings.json` for v1. v2 candidate.
2. **Download resume via HTTP Range requests.** Restart from scratch on crash. v2 if users complain.
3. **Quantization picker.** Locked at Q4_K_M per model. Power users can use the Remote option with their own server.
4. **Custom system prompt / temperature / context length knobs.** System prompt is hardcoded for action-item extraction. v2 if actio gains other LLM-driven features.
5. **Concurrent generation on a single engine instance.** Calls are serialized via `Mutex<mistralrs::Model>`. v2 only if real demand emerges; the answer is multiple engine instances.
6. **CUDA / Vulkan / ROCm backends.** CPU + Metal only for v1. CUDA is a v2 candidate as a separate installer variant.
7. **`/v1/embeddings` endpoint.** Different model, different runtime path.
8. **`/v1/completions` (legacy non-chat).** Every modern client uses `/v1/chat/completions`.
9. **Function calling / `tools` parameter.** Cut from v1 OpenAI-compat surface. v2 if a real consumer needs it.
10. **Auto-fallback from local to remote on failure.** Explicit error per locked-in decision.
11. **Idle-unload after N minutes.** Sticky load per locked-in decision. v2 if RAM pressure becomes a complaint.
12. **Background pre-load at app startup.** Lazy load per locked-in decision. v2 if first-call latency becomes a complaint.
13. **Auth on `/v1/*`.** Localhost-only binding, no API key check. v2 if LAN exposure is requested.
14. **CORS headers on `/v1/*`.** No browser clients expected. Add only if a real consumer needs it.
15. **Multiple concurrent local models loaded simultaneously.** One slot, one engine.
16. **Model upload / sideloading custom GGUFs.** Catalog only. v2 candidate — needs UX for trust and metadata.
17. **Streaming todo extraction in the meeting UI.** `todo_generator` calls non-streaming `chat_completion`. The streaming path exists only for `/v1/*` external clients.

## Open items the implementation plan must resolve

These are deliberately unanswered in the spec because they require either external lookup or in-codebase verification:

1. **Exact `mistralrs` crate version and API surface.** Pinning the dependency and verifying the `Model::load_gguf` / `chat_completion` shape against the latest published version.
2. **Confirm Qwen3.5-0.8B and Qwen3.5-2B repos host Q4_K_M GGUF files** (or their `*-GGUF` siblings do), and capture exact filenames + SHA256s. WebFetch against `huggingface.co` is the first task.
3. **Confirm mistral.rs supports constrained generation against a JSON schema for Qwen3.5 GGUF.** If not, failure-mode 5's first-line defense becomes the second-line retry-with-stricter-prompt.
4. **Confirm `actio-core/src/lib.rs` startup wiring** — where `LlmRouter` and `EngineSlot` get constructed, and how settings-change broadcasts trigger router rebuilds. (Today's `LlmClient` instantiation point is the natural starting place.)
5. **Confirm tokenizer handling.** Some GGUFs embed the tokenizer; some require a separate `tokenizer.json`. The catalog `gguf_filename` field assumes the former; the `llms/{id}/` directory layout supports the latter as a fallback if needed.

These resolve at the start of implementation, not before.

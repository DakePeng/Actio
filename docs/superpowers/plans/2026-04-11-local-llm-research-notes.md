# Local LLM research notes (consumed by plan tasks 2, 5, 7)

## Catalog values (used in Task 5)

### qwen3.5-0.8b-q4km
- hf_repo: unsloth/Qwen3.5-0.8B-GGUF
- gguf_filename: Qwen3.5-0.8B-Q4_K_M.gguf
- size_bytes: 535171328
- size_mb: 510
- sha256: e5926ccfef0c54aebf5d8bda01b2fb6c12ceff4f02a490bc108c5fabf60b334e
- tokenizer_embedded: assumed true (modern GGUF convention; verify at load time)
- tok_model_id: Qwen/Qwen3.5-0.8B (for tokenizer fallback if not embedded)
- context_window: 262144

### qwen3.5-2b-q4km
- hf_repo: unsloth/Qwen3.5-2B-GGUF
- gguf_filename: Qwen3.5-2B-Q4_K_M.gguf
- size_bytes: 1280835840
- size_mb: 1221
- sha256: aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223
- tokenizer_embedded: assumed true
- tok_model_id: Qwen/Qwen3.5-2B (for tokenizer fallback if not embedded)
- context_window: 262144

## RAM estimates (rough: ~1.3-1.5x disk size for Q4_K_M)
- 0.8B Q4_K_M: ~700 MB RAM
- 2B Q4_K_M: ~1700 MB RAM

## mistralrs (used in Task 2 and Task 7)

- crate_version: 0.8.1 (latest on crates.io, confirmed Apr 2 2026)
- gguf_load_method: GgufModelBuilder::new(hf_repo, vec![gguf_filename]).with_tok_model_id(tok_id).build().await
- chat_method: model.send_chat_request(TextMessages::new().add_message(TextMessageRole::User, "...")).await
- simple_chat: model.chat("prompt").await (convenience wrapper)
- supports_json_schema_constraint: true (Model::generate_structured, examples/advanced/json_schema/)
- metal_feature_name: metal (maps to mistralrs-core/metal)
- cuda_feature_name: cuda (maps to mistralrs-core/cuda)
- cpu_default: true (pure Rust, no C compiler needed)
- qwen3.5_support: CONFIRMED — GitHub README explicitly lists "Qwen 3.5" under supported multimodal models
- key_types: GgufModelBuilder, TextMessages, TextMessageRole, Model (trait with send_chat_request)

## Cargo feature mapping for our Cargo.toml

```toml
[dependencies.mistralrs]
version = "0.8"
default-features = false
optional = true

[features]
default = ["local-llm"]
local-llm = ["dep:mistralrs"]
local-llm-metal = ["local-llm", "mistralrs/metal"]
```

## Download URLs

- 0.8B: https://huggingface.co/unsloth/Qwen3.5-0.8B-GGUF/resolve/main/Qwen3.5-0.8B-Q4_K_M.gguf
- 2B: https://huggingface.co/unsloth/Qwen3.5-2B-GGUF/resolve/main/Qwen3.5-2B-Q4_K_M.gguf

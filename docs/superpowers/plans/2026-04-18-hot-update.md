# Hot Update Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a Windows auto-updater for Actio that pulls signed installers from GitHub Releases and installs them on next app launch.

**Architecture:** `tauri-plugin-updater` on the client checks a `latest.json` manifest on GitHub, verifies Ed25519 signatures against a baked-in pubkey, then runs the NSIS installer in passive mode which closes the app, installs, and relaunches. Releases are produced by a CI workflow triggered on `v*.*.*` tag push using `tauri-apps/tauri-action`.

**Tech Stack:** Tauri v2, `tauri-plugin-updater`, `tauri-plugin-process`, GitHub Actions, `tauri-apps/tauri-action@v0`, NSIS installer.

**Note on TDD:** Most of this plan is infrastructure (config, deps, CI). Unit tests aren't meaningful for plugin wiring or workflow YAML — the behavior is validated end-to-end against a real GitHub release on a clean Windows VM (Task 7). Where a Rust function with actual logic exists (the update-check task's error handling), we add a unit test.

**Spec:** `docs/superpowers/specs/2026-04-18-hot-update-design.md`

---

## File Structure

Files created:
- `.github/workflows/release.yml` — CI release workflow
- `backend/src-tauri/src/updater.rs` — update-check task (isolates updater wiring from `main.rs`)
- `RELEASING.md` — release runbook for contributors

Files modified:
- `backend/src-tauri/Cargo.toml` — add `tauri-plugin-updater`, `tauri-plugin-process`
- `backend/src-tauri/tauri.conf.json` — add updater plugin config, bump version
- `backend/src-tauri/src/main.rs` — register plugins, spawn update-check
- `backend/src-tauri/capabilities/default.json` — add updater + process permissions
- `frontend/package.json` — keep `version` in sync

Not modified (intentionally): `actio-core` crate. The updater lives entirely in the Tauri shell.

---

## Task 1: Generate signing keypair (one-time, local)

**Files:**
- Create: `~/.tauri/actio-updater.key` (password-protected Ed25519 private key — DO NOT commit)
- Create: `~/.tauri/actio-updater.key.pub` (public key — safe to commit)

This is a manual, one-time setup step. It produces the keys used by every subsequent release.

- [ ] **Step 1: Generate the keypair**

Run from anywhere:

```bash
pnpm dlx @tauri-apps/cli@latest signer generate -w ~/.tauri/actio-updater.key
```

The CLI prompts for a password. Pick a strong one and **store it in your password manager**.

Expected output: two files — `~/.tauri/actio-updater.key` (private) and `~/.tauri/actio-updater.key.pub` (public). The private key file starts with `untrusted comment: minisign encrypted secret key` or similar.

- [ ] **Step 2: Capture the public key string**

Run:

```bash
cat ~/.tauri/actio-updater.key.pub
```

Copy the full content (a single long base64-ish string, usually prefixed with a minisign header). Save it somewhere — Task 4 pastes it into `tauri.conf.json`.

- [ ] **Step 3: Back up the private key offline**

Copy `~/.tauri/actio-updater.key` and its password into your password manager.

**Why this matters:** If this key is lost, no existing installation of Actio can ever receive another update. Every user would have to manually reinstall a new build signed with a fresh pubkey. There is no recovery path that preserves the install base.

- [ ] **Step 4: Add private key to GitHub repo secrets**

1. Go to GitHub repo → Settings → Secrets and variables → Actions → New repository secret.
2. Create `TAURI_SIGNING_PRIVATE_KEY`: paste the **full contents** of `~/.tauri/actio-updater.key` (including the `untrusted comment:` header line).
3. Create `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: the password you chose.

No commit yet — nothing in the repo has changed.

---

## Task 2: Add plugin dependencies

**Files:**
- Modify: `backend/src-tauri/Cargo.toml:10-19`

- [ ] **Step 1: Add the two plugin crates**

In `backend/src-tauri/Cargo.toml`, extend the `[dependencies]` block:

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tauri = { version = "2", features = [] }
tauri-plugin-clipboard-manager = "2"
tauri-plugin-global-shortcut = "2"
tauri-plugin-notification = "2"
tauri-plugin-updater = "2"
tauri-plugin-process = "2"
enigo = "0.3"
tracing = "0.1"
actio-core = { path = "../actio-core", default-features = false }
```

- [ ] **Step 2: Verify the crates resolve**

Run:

```bash
cd backend/src-tauri && cargo check
```

Expected: clean build, no errors. New crates download on first run.

- [ ] **Step 3: Commit**

```bash
git add backend/src-tauri/Cargo.toml backend/src-tauri/Cargo.lock
git commit -m "chore(desktop): add tauri-plugin-updater and tauri-plugin-process deps"
```

---

## Task 3: Grant updater + process capabilities

**Files:**
- Modify: `backend/src-tauri/capabilities/default.json`

- [ ] **Step 1: Add the permissions**

Replace the `permissions` array in `backend/src-tauri/capabilities/default.json` with:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Default capability for the Actio desktop shell",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "notification:default",
    "core:window:allow-start-dragging",
    "core:event:default",
    "global-shortcut:default",
    "clipboard-manager:default",
    "updater:default",
    "process:default"
  ]
}
```

- [ ] **Step 2: Verify the config parses**

Run:

```bash
cd backend/src-tauri && cargo check
```

Expected: clean build. Tauri's build script validates the capability file.

- [ ] **Step 3: Commit**

```bash
git add backend/src-tauri/capabilities/default.json
git commit -m "chore(desktop): grant updater and process capabilities"
```

---

## Task 4: Configure updater plugin in `tauri.conf.json`

**Files:**
- Modify: `backend/src-tauri/tauri.conf.json`

Replace `<owner>/<repo>` with your actual GitHub path (e.g., `dakepeng/actio`). Replace `<PUBKEY_STRING>` with the pubkey string captured in Task 1 Step 2.

- [ ] **Step 1: Add the `plugins` block**

Edit `backend/src-tauri/tauri.conf.json` to add a top-level `plugins` key alongside `bundle`:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Actio",
  "version": "0.1.0",
  "identifier": "com.actio.desktop",
  "build": {
    "beforeDevCommand": "cd ../frontend && pnpm dev",
    "beforeBuildCommand": "cd ../frontend && pnpm build",
    "devUrl": "http://localhost:1420",
    "frontendDist": "../../frontend/dist"
  },
  "app": {
    "windows": [
      {
        "title": "Actio",
        "label": "main",
        "width": 320,
        "height": 78,
        "resizable": false,
        "fullscreen": false,
        "decorations": false,
        "transparent": true,
        "alwaysOnTop": true,
        "skipTaskbar": true,
        "shadow": false,
        "backgroundThrottling": "disabled"
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/icon.png",
      "icons/icon.ico"
    ],
    "resources": {
      "../target/release/onnxruntime.dll": "onnxruntime.dll",
      "../target/release/onnxruntime_providers_shared.dll": "onnxruntime_providers_shared.dll",
      "../target/release/sherpa-onnx-c-api.dll": "sherpa-onnx-c-api.dll",
      "../target/release/sherpa-onnx-cxx-api.dll": "sherpa-onnx-cxx-api.dll"
    },
    "createUpdaterArtifacts": true
  },
  "plugins": {
    "updater": {
      "active": true,
      "endpoints": [
        "https://github.com/<owner>/<repo>/releases/latest/download/latest.json"
      ],
      "pubkey": "<PUBKEY_STRING>",
      "windows": {
        "installMode": "passive"
      }
    }
  }
}
```

Two changes to note:
- Added `"createUpdaterArtifacts": true` inside `bundle` — this tells `tauri build` to produce the `.sig` sidecar file.
- Added the entire `plugins.updater` block.

- [ ] **Step 2: Verify config parses**

Run:

```bash
cd backend/src-tauri && cargo check
```

Expected: clean build.

- [ ] **Step 3: Commit**

```bash
git add backend/src-tauri/tauri.conf.json
git commit -m "feat(desktop): configure updater plugin with GitHub release endpoint"
```

---

## Task 5: Create the update-check module

**Files:**
- Create: `backend/src-tauri/src/updater.rs`

- [ ] **Step 1: Write the module**

Create `backend/src-tauri/src/updater.rs` with:

```rust
//! Background update check: on app launch, look at the configured `latest.json`
//! endpoint, and if a newer version is published, download + install it. The
//! plugin verifies the Ed25519 signature before execution; on any failure we
//! log and keep running the current version.

use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

/// Spawn a detached task that checks once for an update and installs it if found.
/// Never blocks app startup. All errors are logged, never surfaced to the user.
pub fn spawn_update_check(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = run_update_check(app).await {
            tracing::warn!(error = %e, "update check failed");
        }
    });
}

async fn run_update_check(app: AppHandle) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let updater = app.updater()?;
    match updater.check().await? {
        Some(update) => {
            tracing::info!(
                current = %update.current_version,
                new = %update.version,
                "update available, downloading and installing"
            );
            update
                .download_and_install(
                    |chunk_len, total| {
                        tracing::debug!(chunk_len, ?total, "update download progress");
                    },
                    || {
                        tracing::info!("update download complete, launching installer");
                    },
                )
                .await?;
            Ok(())
        }
        None => {
            tracing::debug!("no update available");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    // Integration-only: the updater flow depends on a live HTTP endpoint and
    // the Tauri runtime. End-to-end validation lives in Task 9 (VM test).
    // This placeholder keeps the module's test module wired for future use.
    #[test]
    fn module_compiles() {}
}
```

- [ ] **Step 2: Verify the module compiles**

Run:

```bash
cd backend/src-tauri && cargo check
```

Expected: clean build. The module is defined but not yet used by `main.rs`, so you'll see an `unused` warning — that's fine for this step.

- [ ] **Step 3: Commit**

```bash
git add backend/src-tauri/src/updater.rs
git commit -m "feat(desktop): add updater module with background check task"
```

---

## Task 6: Wire the updater into `main.rs`

**Files:**
- Modify: `backend/src-tauri/src/main.rs`

- [ ] **Step 1: Declare the module**

At the top of `backend/src-tauri/src/main.rs`, after the `#![cfg_attr(...)]` line, add:

```rust
mod updater;
```

It goes immediately after line 1 and before the `use actio_core::...` line.

- [ ] **Step 2: Register the plugins in `main()`**

In `main()` (line 563), extend the plugin chain. Find:

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_notification::init())
    .plugin(tauri_plugin_clipboard_manager::init())
    .plugin(tauri_plugin_global_shortcut::Builder::new().build())
```

Replace with:

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_notification::init())
    .plugin(tauri_plugin_clipboard_manager::init())
    .plugin(tauri_plugin_global_shortcut::Builder::new().build())
    .plugin(tauri_plugin_updater::Builder::new().build())
    .plugin(tauri_plugin_process::init())
```

- [ ] **Step 3: Spawn the update check in the setup hook**

In the `.setup(|app| { ... })` block, find this line near the end (around line 605):

```rust
            configure_startup_window(app)?;
            Ok(())
```

Change it to:

```rust
            configure_startup_window(app)?;
            updater::spawn_update_check(app.handle().clone());
            Ok(())
```

- [ ] **Step 4: Build and run the dev app**

Run:

```bash
cd backend/src-tauri && cargo build
```

Expected: clean build.

Then:

```bash
cd backend && pnpm --prefix ../frontend install && pnpm --prefix ../frontend tauri dev
```

(If you already run `pnpm tauri dev` from elsewhere, use that.)

Expected: app launches normally. In the console, look for `update check failed` warning (expected — the `latest.json` URL 404s because no release exists yet) OR `no update available` (also fine). No crash, no UI change.

- [ ] **Step 5: Commit**

```bash
git add backend/src-tauri/src/main.rs
git commit -m "feat(desktop): check for updates on app launch"
```

---

## Task 7: Create the release CI workflow

**Files:**
- Create: `.github/workflows/release.yml`

- [ ] **Step 1: Write the workflow**

Create `.github/workflows/release.yml` with:

```yaml
name: Release

on:
  push:
    tags:
      - 'v*.*.*'

jobs:
  release:
    runs-on: windows-latest
    permissions:
      contents: write
    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Set up pnpm
        uses: pnpm/action-setup@v3
        with:
          version: 9

      - name: Set up Node.js
        uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: pnpm
          cache-dependency-path: frontend/pnpm-lock.yaml

      - name: Set up Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: x86_64-pc-windows-msvc

      - name: Cache cargo registry and target
        uses: Swatinem/rust-cache@v2
        with:
          workspaces: |
            backend
            backend/src-tauri

      - name: Install frontend deps
        working-directory: frontend
        run: pnpm install --frozen-lockfile

      - name: Build and release
        uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        with:
          projectPath: backend/src-tauri
          tagName: ${{ github.ref_name }}
          releaseName: 'Actio ${{ github.ref_name }}'
          releaseDraft: false
          prerelease: false
          includeUpdaterJson: true
          args: '--target x86_64-pc-windows-msvc'
```

- [ ] **Step 2: Lint the workflow**

If you have `actionlint` installed, run:

```bash
actionlint .github/workflows/release.yml
```

Expected: no errors. If you don't have it, skip — the workflow will fail fast on push if YAML is malformed.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: add release workflow that builds, signs, and publishes to GitHub"
```

Do **not** push a tag yet. Task 9 does the end-to-end test.

---

## Task 8: Write the release runbook

**Files:**
- Create: `RELEASING.md`

- [ ] **Step 1: Write the runbook**

Create `RELEASING.md` at the repo root:

````markdown
# Releasing Actio

## Cutting a release

1. Bump the version in all three places (they must match):
   - `backend/src-tauri/tauri.conf.json` → top-level `version`
   - `backend/src-tauri/Cargo.toml` → `[package] version`
   - `frontend/package.json` → `version`
2. Commit the bump: `git commit -am "chore: bump to v<version>"`.
3. Tag: `git tag v<version>` (e.g., `git tag v0.2.0`).
4. Push: `git push && git push --tags`.
5. Watch the **Release** workflow on GitHub Actions. When it completes, a release appears at `https://github.com/<owner>/<repo>/releases/tag/v<version>` with three assets:
   - `Actio_<version>_x64-setup.exe`
   - `Actio_<version>_x64-setup.exe.sig`
   - `latest.json`

Existing installations will pick up the update on their next launch.

## Recovering from a bad release

If a release is broken:

1. Go to GitHub → Releases → the bad release → **Delete release**. Also delete the underlying git tag (`git push --delete origin v<bad>` and `git tag -d v<bad>`).
2. Clients that haven't updated yet will stop seeing the update (the `latest.json` URL now resolves to the previous release).
3. Fix the bug, bump to the **next** version (you cannot reuse a tag), and cut a new release.

Users who already installed the bad version are not rolled back automatically — they'll receive the fix release when they next launch.

## Signing key custody

- The Ed25519 private key is stored as GitHub secret `TAURI_SIGNING_PRIVATE_KEY`. The password is `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
- A copy of the private key file and password lives in the project owner's password manager.
- The public key is baked into `backend/src-tauri/tauri.conf.json` under `plugins.updater.pubkey`.
- **If the private key is lost**, every existing installation becomes un-updateable. Recovery path: generate a new keypair, bake the new pubkey into a fresh release, ask users to manually reinstall. Treat this as a serious incident.

## First-time setup (already done)

For reference, the keypair was generated with:

```bash
pnpm dlx @tauri-apps/cli@latest signer generate -w ~/.tauri/actio-updater.key
```

The public key was copied from `~/.tauri/actio-updater.key.pub` into `tauri.conf.json`. The private key file contents and password were added to GitHub repo secrets.
````

- [ ] **Step 2: Commit**

```bash
git add RELEASING.md
git commit -m "docs: add release runbook covering tagging, recovery, key custody"
```

---

## Task 9: End-to-end test on a clean Windows VM

This is the single real validation — it exercises the full chain: CI build → GitHub release → signed download → signature verification → install → relaunch.

**Prerequisite:** a clean Windows 10 or 11 VM (VirtualBox, Hyper-V, or a spare machine). "Clean" means no prior Actio install.

- [ ] **Step 1: Cut a staging pre-release (v0.1.1)**

On your dev machine:

```bash
# 1. Bump version
# Edit backend/src-tauri/tauri.conf.json      → "version": "0.1.1"
# Edit backend/src-tauri/Cargo.toml           → version = "0.1.1"
# Edit frontend/package.json                  → "version": "0.1.1"

# 2. Commit + tag
git commit -am "chore: bump to v0.1.1"
git tag v0.1.1
git push && git push --tags
```

Wait for the **Release** workflow to complete (typically 10–20 minutes for a cold cache). Confirm the release page has `Actio_0.1.1_x64-setup.exe`, `Actio_0.1.1_x64-setup.exe.sig`, and `latest.json`.

- [ ] **Step 2: Install v0.1.1 on the VM**

Download `Actio_0.1.1_x64-setup.exe` from the GitHub release page, run it on the clean VM. Launch Actio. It should behave normally. Leave it installed and quit the app.

- [ ] **Step 3: Cut v0.1.2 with a visible change**

On the dev machine, make a tiny visible change — e.g., change the window `title` in `tauri.conf.json` from `"Actio"` to `"Actio (updated)"`. Bump to `0.1.2` in all three places, commit, tag, push.

Wait for the CI workflow to complete.

- [ ] **Step 4: Trigger the update on the VM**

On the VM, launch Actio (which is still v0.1.1). Wait ~10 seconds — the background update-check should find v0.1.2, download it, and run the passive NSIS installer. You should see a progress bar, then the app closes and relaunches.

Expected: the relaunched app shows the updated title. Confirm by looking at the taskbar entry or window title bar (even though the window is decorationless, the OS taskbar shows the title).

- [ ] **Step 5: Verify persistence**

On the VM, before running Step 4, create some app state: add a couple of reminders, record a short dictation, enroll a speaker. After the update in Step 4, confirm all three survive — the reminders are still in the board, the speaker is still enrolled, the settings are still set.

- [ ] **Step 6: Verify offline resilience**

On the VM, disable the network, launch Actio. Expected: app starts normally, no hang, no error dialog. Re-enable network, confirm a later launch picks up any newer update.

- [ ] **Step 7: Verify signature enforcement (paranoia test)**

Optional but recommended. On your dev machine, generate a second Ed25519 key (`signer generate -w /tmp/bogus.key`), build a fake `v0.1.3` signed with the bogus key, upload it to a test release, and point a second test build of the app (with the real pubkey) at that manifest. Confirm the client refuses to install — log should show a signature verification failure.

If that's too much setup, skip — the plugin's signature check is well-tested upstream.

- [ ] **Step 8: Document results**

If all steps pass, the updater is live. No code change needed.

If any step fails:
- Capture logs from the VM (`%LOCALAPPDATA%\com.actio.desktop\logs\` or wherever the tracing subscriber writes).
- File the failure mode against `docs/superpowers/specs/2026-04-18-hot-update-design.md`.
- Do **not** patch symptoms — return to the design to understand the root cause.

---

## Verification

After Task 9 passes:

- [ ] Released v0.1.1 and v0.1.2 exist on GitHub with the three expected artifacts each.
- [ ] A v0.1.1 VM successfully auto-updated to v0.1.2 without user interaction beyond launching the app.
- [ ] App state (reminders, speakers, settings, downloaded ASR models) persisted across the update.
- [ ] No network: app launches cleanly.
- [ ] The public key in `tauri.conf.json` matches the private key used by CI (established by the successful signature verification in Task 9 Step 4).
- [ ] `RELEASING.md` reflects the actual flow used.
- [ ] Private key + password are in the password manager and in GitHub secrets — and only those two places.

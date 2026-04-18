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

Existing installations pick up the update on their next launch.

## Recovering from a bad release

If a release is broken:

1. Go to GitHub → Releases → the bad release → **Delete release**. Also delete the underlying git tag (`git push --delete origin v<bad>` and `git tag -d v<bad>`).
2. Clients that haven't updated yet stop seeing the update (the `latest.json` URL now resolves to the previous release).
3. Fix the bug, bump to the **next** version (you cannot reuse a tag), and cut a new release.

Users who already installed the bad version are not rolled back automatically — they receive the fix release on their next launch.

## Signing key custody

- The Ed25519 private key is stored as GitHub secret `TAURI_SIGNING_PRIVATE_KEY`. Its password is `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
- A copy of the private key file and password lives in the project owner's password manager.
- The public key is baked into `backend/src-tauri/tauri.conf.json` under `plugins.updater.pubkey`.
- **If the private key is lost**, every existing installation becomes un-updateable. Recovery path: generate a new keypair, bake the new pubkey into a fresh release, ask users to manually reinstall. Treat this as a serious incident.

## First-time setup

Before the first real release can be cut, the following one-time steps must be done on a trusted machine:

1. Generate the signing keypair:

   ```bash
   pnpm dlx @tauri-apps/cli@latest signer generate -w ~/.tauri/actio-updater.key
   ```

   Pick a strong password when prompted. Store the password in a password manager.

2. Copy the public key string from `~/.tauri/actio-updater.key.pub` into `backend/src-tauri/tauri.conf.json`, replacing the `REPLACE_WITH_ED25519_PUBKEY_FROM_TAURI_SIGNER_GENERATE` placeholder.

3. In the same file, replace the `REPLACE_OWNER/REPLACE_REPO` placeholders in the `endpoints` URL with the actual GitHub owner and repo name (e.g., `dakepeng/actio`).

4. Add two GitHub repo secrets (Settings → Secrets and variables → Actions):
   - `TAURI_SIGNING_PRIVATE_KEY` — full contents of `~/.tauri/actio-updater.key`, including the `untrusted comment:` header.
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — the password chosen above.

5. Commit the pubkey and URL changes to `tauri.conf.json`.

6. Back up the private key file and password offline (password manager).

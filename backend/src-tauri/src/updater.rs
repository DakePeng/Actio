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
    // the Tauri runtime. End-to-end validation lives in the VM test task.
    // This placeholder keeps the module's test module wired for future use.
    #[test]
    fn module_compiles() {}
}

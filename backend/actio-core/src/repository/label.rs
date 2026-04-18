use sqlx::SqlitePool;
use uuid::Uuid;

use crate::domain::types::{CreateLabelRequest, Label, PatchLabelRequest};

/// Default labels seeded on first app launch. Each tuple is (name, fg, bg).
/// Colors mirror the existing priority palette for visual consistency.
pub const DEFAULT_LABELS: &[(&str, &str, &str)] = &[
    ("Work", "#1d4ed8", "#dbeafe"),
    ("Personal", "#6d28d9", "#ede9fe"),
    ("Urgent", "#b91c1c", "#fee2e2"),
    ("Idea", "#a16207", "#fef3c7"),
    ("Follow-up", "#15803d", "#dcfce7"),
    ("Meeting", "#0e7490", "#cffafe"),
];

/// Insert the default label set for the given tenant on first launch.
///
/// Self-gating: skips entirely if the tenant already has any labels at all
/// (custom or default). This means a user who explicitly deletes the seeded
/// presets won't have them resurrected on the next app start. Safe to call
/// unconditionally on every startup.
///
/// Returns the number of rows actually inserted (0 if skipped).
pub async fn seed_default_labels(pool: &SqlitePool, tenant_id: Uuid) -> Result<u64, sqlx::Error> {
    let tenant = tenant_id.to_string();

    // Skip if any labels already exist for this tenant.
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM labels WHERE tenant_id = ?1")
        .bind(&tenant)
        .fetch_one(pool)
        .await?;
    if existing > 0 {
        return Ok(0);
    }

    let mut inserted: u64 = 0;
    for (name, color, bg_color) in DEFAULT_LABELS {
        let id = Uuid::new_v4().to_string();
        let result = sqlx::query(
            "INSERT OR IGNORE INTO labels (id, tenant_id, name, color, bg_color) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(&id)
        .bind(&tenant)
        .bind(name)
        .bind(color)
        .bind(bg_color)
        .execute(pool)
        .await?;
        inserted += result.rows_affected();
    }
    Ok(inserted)
}

pub async fn list_labels(pool: &SqlitePool, tenant_id: Uuid) -> Result<Vec<Label>, sqlx::Error> {
    sqlx::query_as::<_, Label>("SELECT * FROM labels WHERE tenant_id = ?1 ORDER BY name ASC")
        .bind(tenant_id.to_string())
        .fetch_all(pool)
        .await
}

pub async fn create_label(
    pool: &SqlitePool,
    tenant_id: Uuid,
    req: &CreateLabelRequest,
) -> Result<Label, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    sqlx::query_as::<_, Label>(
        "INSERT INTO labels (id, tenant_id, name, color, bg_color) VALUES (?1, ?2, ?3, ?4, ?5) RETURNING *",
    )
    .bind(&id)
    .bind(tenant_id.to_string())
    .bind(&req.name)
    .bind(&req.color)
    .bind(&req.bg_color)
    .fetch_one(pool)
    .await
}

pub async fn patch_label(
    pool: &SqlitePool,
    id: Uuid,
    req: &PatchLabelRequest,
) -> Result<Option<Label>, sqlx::Error> {
    sqlx::query_as::<_, Label>(
        r#"UPDATE labels SET
               name     = COALESCE(?1, name),
               color    = COALESCE(?2, color),
               bg_color = COALESCE(?3, bg_color)
           WHERE id = ?4
           RETURNING *"#,
    )
    .bind(&req.name)
    .bind(&req.color)
    .bind(&req.bg_color)
    .bind(id.to_string())
    .fetch_optional(pool)
    .await
}

pub async fn delete_label(pool: &SqlitePool, id: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM labels WHERE id = ?1")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

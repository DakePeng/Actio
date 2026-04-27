//! tenant_profile repository: per-tenant identity (display_name, aliases, bio).
//!
//! Aliases are stored as a JSON array in a TEXT column; the table CHECK
//! ensures shape. The repo handles JSON ↔ Vec<String> conversion.

use sqlx::SqlitePool;
use uuid::Uuid;

use crate::domain::types::TenantProfile;

pub async fn get_for_tenant(
    pool: &SqlitePool,
    tenant_id: Uuid,
) -> sqlx::Result<Option<TenantProfile>> {
    let row: Option<(String, Option<String>, String, Option<String>)> = sqlx::query_as(
        "SELECT tenant_id, display_name, aliases, bio FROM tenant_profile WHERE tenant_id = ?1",
    )
    .bind(tenant_id.to_string())
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(tid, name, aliases_json, bio)| TenantProfile {
        tenant_id: Uuid::parse_str(&tid).unwrap_or(tenant_id),
        display_name: name,
        aliases: serde_json::from_str(&aliases_json).unwrap_or_default(),
        bio,
    }))
}

pub async fn upsert(pool: &SqlitePool, profile: &TenantProfile) -> sqlx::Result<()> {
    let aliases_json =
        serde_json::to_string(&profile.aliases).unwrap_or_else(|_| "[]".to_string());
    sqlx::query(
        r#"INSERT INTO tenant_profile (tenant_id, display_name, aliases, bio, updated_at)
           VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
           ON CONFLICT(tenant_id) DO UPDATE SET
             display_name = excluded.display_name,
             aliases      = excluded.aliases,
             bio          = excluded.bio,
             updated_at   = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')"#,
    )
    .bind(profile.tenant_id.to_string())
    .bind(&profile.display_name)
    .bind(aliases_json)
    .bind(&profile.bio)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::testing::fresh_pool;

    #[tokio::test]
    async fn upsert_then_get_round_trips_unicode_aliases() {
        let pool = fresh_pool().await;
        let tenant_id = Uuid::new_v4();
        let profile = TenantProfile {
            tenant_id,
            display_name: Some("Dake Peng".into()),
            aliases: vec!["Dake".into(), "DK".into(), "彭大可".into()],
            bio: Some("Solo dev building Actio.".into()),
        };

        upsert(&pool, &profile).await.unwrap();

        let got = get_for_tenant(&pool, tenant_id).await.unwrap().unwrap();
        assert_eq!(got.display_name.as_deref(), Some("Dake Peng"));
        assert_eq!(got.aliases, vec!["Dake", "DK", "彭大可"]);
        assert_eq!(got.bio.as_deref(), Some("Solo dev building Actio."));
    }

    #[tokio::test]
    async fn upsert_is_idempotent_and_overwrites() {
        let pool = fresh_pool().await;
        let tenant_id = Uuid::new_v4();
        let p1 = TenantProfile {
            tenant_id,
            display_name: Some("Old".into()),
            aliases: vec!["a".into()],
            bio: None,
        };
        let p2 = TenantProfile {
            tenant_id,
            display_name: Some("New".into()),
            aliases: vec!["b".into(), "c".into()],
            bio: Some("now with bio".into()),
        };
        upsert(&pool, &p1).await.unwrap();
        upsert(&pool, &p2).await.unwrap();

        let got = get_for_tenant(&pool, tenant_id).await.unwrap().unwrap();
        assert_eq!(got.display_name.as_deref(), Some("New"));
        assert_eq!(got.aliases, vec!["b", "c"]);
        assert_eq!(got.bio.as_deref(), Some("now with bio"));

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tenant_profile")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count.0, 1, "upsert must not insert duplicate rows");
    }

    #[tokio::test]
    async fn aliases_check_rejects_non_array_json() {
        let pool = fresh_pool().await;
        let tenant_id = Uuid::new_v4();
        let result = sqlx::query(
            "INSERT INTO tenant_profile (tenant_id, aliases) VALUES (?1, ?2)",
        )
        .bind(tenant_id.to_string())
        .bind(r#""not an array""#)
        .execute(&pool)
        .await;
        assert!(result.is_err(), "expected CHECK constraint violation");
    }
}

mod common;

use actio_asr::domain::types::{NewTodo, TodoPriority, TodoStatus};
use actio_asr::repository::{session, speaker, todo, transcript};

#[tokio::test]
async fn session_repository_persists_and_ends_sessions() {
    let pool = common::test_pool().await;
    let tenant_id = common::new_tenant_id();

    let created = session::create_session(&pool, tenant_id, "upload", "batch")
        .await
        .expect("session should be created");

    assert_eq!(created.tenant_id, tenant_id);
    assert_eq!(created.source_type, "upload");
    assert_eq!(created.mode, "batch");
    assert_eq!(created.routing_policy, "local_first");
    assert!(created.ended_at.is_none());

    let fetched = session::get_session(&pool, created.id)
        .await
        .expect("session should be fetched");
    assert_eq!(fetched.id, created.id);
    assert!(fetched.ended_at.is_none());

    session::end_session(&pool, created.id)
        .await
        .expect("session should end");

    let ended = session::get_session(&pool, created.id)
        .await
        .expect("ended session should still be fetched");
    assert!(ended.ended_at.is_some());
}

#[tokio::test]
async fn transcript_repository_creates_lists_and_finalizes_transcripts() {
    let pool = common::test_pool().await;
    let session = common::create_test_session(&pool, common::new_tenant_id()).await;

    let later = transcript::create_transcript(
        &pool,
        session.id,
        "later partial",
        2000,
        2600,
        false,
        None,
    )
    .await
    .expect("later transcript should be created");

    let earlier = transcript::create_transcript(
        &pool,
        session.id,
        "earlier final",
        1000,
        1600,
        true,
        None,
    )
    .await
    .expect("earlier transcript should be created");

    let listed = transcript::get_transcripts_for_session(&pool, session.id)
        .await
        .expect("transcripts should be listed");
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].id, earlier.id);
    assert_eq!(listed[1].id, later.id);

    let finalized = transcript::finalize_transcript(&pool, later.id, "later final")
        .await
        .expect("transcript should finalize");
    assert_eq!(finalized.text, "later final");
    assert!(finalized.is_final);

    let refreshed = transcript::get_transcripts_for_session(&pool, session.id)
        .await
        .expect("finalized transcripts should still be listed");
    let refreshed_later = refreshed
        .into_iter()
        .find(|item| item.id == later.id)
        .expect("finalized transcript should be present");
    assert_eq!(refreshed_later.text, "later final");
    assert!(refreshed_later.is_final);
}

#[tokio::test]
async fn speaker_repository_lists_only_active_speakers_for_the_requested_tenant() {
    let pool = common::test_pool().await;
    let tenant_id = common::new_tenant_id();
    let other_tenant_id = common::new_tenant_id();

    let active = speaker::create_speaker(&pool, "Active Speaker", tenant_id)
        .await
        .expect("active speaker should be created");
    let inactive = speaker::create_speaker(&pool, "Inactive Speaker", tenant_id)
        .await
        .expect("inactive speaker should be created");
    let other_tenant = speaker::create_speaker(&pool, "Other Tenant Speaker", other_tenant_id)
        .await
        .expect("other tenant speaker should be created");

    sqlx::query("UPDATE speakers SET status = 'inactive' WHERE id = $1")
        .bind(inactive.id)
        .execute(&pool)
        .await
        .expect("inactive speaker update should succeed");

    let listed = speaker::list_speakers(&pool, tenant_id)
        .await
        .expect("speakers should be listed");

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, active.id);
    assert_eq!(listed[0].status, "active");
    assert!(listed.iter().all(|item| item.tenant_id == tenant_id));
    assert!(listed.iter().all(|item| item.id != inactive.id));
    assert!(listed.iter().all(|item| item.id != other_tenant.id));
}

#[tokio::test]
async fn todo_repository_creates_lists_and_deduplicates_items_per_session() {
    let pool = common::test_pool().await;
    let tenant_id = common::new_tenant_id();
    let session = common::create_test_session(&pool, tenant_id).await;
    let speaker = speaker::create_speaker(&pool, "Owner", tenant_id)
        .await
        .expect("speaker should be created");

    assert!(
        !todo::has_todos(&pool, session.id)
            .await
            .expect("todo presence check should succeed")
    );

    let first_batch = todo::create_todos(
        &pool,
        &[
            NewTodo {
                session_id: session.id,
                speaker_id: Some(speaker.id),
                assigned_to: Some("Owner".to_string()),
                description: "Prepare the draft".to_string(),
                priority: Some(TodoPriority::High),
            },
            NewTodo {
                session_id: session.id,
                speaker_id: None,
                assigned_to: None,
                description: "Share meeting notes".to_string(),
                priority: Some(TodoPriority::Low),
            },
        ],
    )
    .await
    .expect("todos should be created");

    assert_eq!(first_batch.len(), 2);
    assert!(
        todo::has_todos(&pool, session.id)
            .await
            .expect("todo presence should be true after insert")
    );

    let duplicate_batch = todo::create_todos(
        &pool,
        &[NewTodo {
            session_id: session.id,
            speaker_id: Some(speaker.id),
            assigned_to: Some("Owner".to_string()),
            description: "Prepare the draft".to_string(),
            priority: Some(TodoPriority::Medium),
        }],
    )
    .await
    .expect("duplicate insert should not fail");
    assert!(duplicate_batch.is_empty());

    let listed = todo::get_todos_for_session(&pool, session.id)
        .await
        .expect("todos should be listed");
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].description, "Prepare the draft");
    assert_eq!(listed[0].assigned_to.as_deref(), Some("Owner"));
    assert!(matches!(listed[0].priority, Some(TodoPriority::High)));
    assert!(matches!(listed[0].status, TodoStatus::Open));
    assert_eq!(listed[1].description, "Share meeting notes");
}

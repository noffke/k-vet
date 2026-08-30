//! Textbausteine: the library the vet assembles a Vorbericht or a Therapie from (issues.md 11).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test]
async fn a_text_block_is_a_draft_until_it_has_a_name_and_a_text(pool: PgPool) {
    let app = TestApp::new(pool).await;

    let created = app.post_empty("/api/text-blocks").await;
    assert_eq!(created.status, StatusCode::OK);
    let id = created.id();
    let record = created.json();
    assert_eq!(record["draft"], true);
    assert_eq!(record["missing_fields"], json!(["name", "content"]));

    // A name alone inserts nothing, so it does not complete the record.
    let named = app
        .patch(
            &format!("/api/text-blocks/{id}"),
            json!({ "name": "Impfung" }),
        )
        .await
        .json();
    assert_eq!(named["draft"], true);
    assert_eq!(named["missing_fields"], json!(["content"]));

    let filled = app
        .patch(
            &format!("/api/text-blocks/{id}"),
            json!({ "content": "Impfung nach Schema, Tier zeigte keine Auffälligkeiten." }),
        )
        .await
        .json();
    assert_eq!(filled["draft"], false);
    assert_eq!(filled["missing_fields"], json!([]));
}

/// The transition is one-way (datamodel.md): a complete record cannot be emptied back out.
#[sqlx::test]
async fn clearing_the_text_of_a_finished_block_is_rejected(pool: PgPool) {
    let app = TestApp::new(pool).await;
    let id = app.post_empty("/api/text-blocks").await.id();
    app.patch(
        &format!("/api/text-blocks/{id}"),
        json!({ "name": "Impfung", "content": "Impfung nach Schema." }),
    )
    .await;

    let cleared = app
        .patch(
            &format!("/api/text-blocks/{id}"),
            json!({ "content": null }),
        )
        .await;
    assert_eq!(cleared.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(cleared.error_fields(), vec!["content"]);

    let still_there = app.get(&format!("/api/text-blocks/{id}")).await.json();
    assert_eq!(still_there["content"], "Impfung nach Schema.");
}

/// The vet remembers the wording long before they remember what they called it.
#[sqlx::test]
async fn search_covers_the_text_as_well_as_the_name(pool: PgPool) {
    let app = TestApp::new(pool).await;
    let id = app.post_empty("/api/text-blocks").await.id();
    app.patch(
        &format!("/api/text-blocks/{id}"),
        json!({ "name": "Impfung", "content": "Tier zeigte keine Auffälligkeiten." }),
    )
    .await;

    let by_name = app.get("/api/text-blocks?q=Impf").await.json();
    assert_eq!(by_name.as_array().map(Vec::len), Some(1));

    let by_content = app
        .get("/api/text-blocks?q=Auff%C3%A4lligkeiten")
        .await
        .json();
    assert_eq!(
        by_content.as_array().map(Vec::len),
        Some(1),
        "the content is searchable"
    );

    let nothing = app.get("/api/text-blocks?q=Kastration").await.json();
    assert_eq!(nothing.as_array().map(Vec::len), Some(0));
}

#[sqlx::test]
async fn archiving_hides_a_block_until_it_is_asked_for(pool: PgPool) {
    let app = TestApp::new(pool).await;
    let id = app.post_empty("/api/text-blocks").await.id();
    app.patch(
        &format!("/api/text-blocks/{id}"),
        json!({ "name": "Impfung", "content": "Impfung nach Schema." }),
    )
    .await;

    app.post_empty(&format!("/api/text-blocks/{id}/archive"))
        .await;
    let visible = app.get("/api/text-blocks").await.json();
    assert_eq!(visible.as_array().map(Vec::len), Some(0));

    let with_archived = app.get("/api/text-blocks?archived=true").await.json();
    assert_eq!(with_archived.as_array().map(Vec::len), Some(1));
    assert_eq!(with_archived[0]["archived"], true);

    app.post_empty(&format!("/api/text-blocks/{id}/unarchive"))
        .await;
    let restored = app.get("/api/text-blocks").await.json();
    assert_eq!(restored.as_array().map(Vec::len), Some(1));
}

#[sqlx::test]
async fn an_unknown_text_block_is_a_404(pool: PgPool) {
    let app = TestApp::new(pool).await;
    assert_eq!(
        app.get("/api/text-blocks/999999").await.status,
        StatusCode::NOT_FOUND
    );
}

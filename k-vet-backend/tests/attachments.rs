//! Attachment upload, delivery, thumbnails and dedup (T018).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::{StatusCode, header};
use common::TestApp;
use sqlx::PgPool;

/// A small in-memory PNG so the thumbnail path is exercised for real.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    let image = image::RgbImage::from_pixel(width, height, image::Rgb([20, 90, 70]));
    image::DynamicImage::ImageRgb8(image)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encodes png");
    bytes
}

#[sqlx::test]
async fn uploads_a_patient_file_with_reference_date_and_note(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;

    let response = app
        .post_multipart(
            "/api/attachments",
            "befund.png",
            "image/png",
            &png(600, 400),
            &[
                ("kind", "patient_file"),
                ("patient_id", &patient_id.to_string()),
                ("reference_date", "2026-05-04"),
                ("note", "Röntgenbild vom Vorbehandler"),
            ],
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&response.body)
    );
    let body = response.json();
    assert_eq!(body["kind"], "patient_file");
    assert_eq!(body["patient_id"], patient_id);
    assert_eq!(body["reference_date"], "2026-05-04");
    assert_eq!(body["note"], "Röntgenbild vom Vorbehandler");
    assert_eq!(
        body["has_thumbnail"], true,
        "images get a thumbnail at upload time"
    );
    assert_eq!(body["orig_name"], "befund.png");
    assert_eq!(body["mime_type"], "image/png");

    let sha256 = body["sha256"].as_str().expect("hash").to_owned();
    assert_eq!(sha256.len(), 64);
    let stored = app
        .attachments
        .path()
        .join(&sha256[0..2])
        .join(&sha256[2..4])
        .join(&sha256);
    assert!(
        stored.exists(),
        "content-addressed file exists at {}",
        stored.display()
    );
}

#[sqlx::test]
async fn streams_content_and_thumbnail(pool: PgPool) {
    let app = TestApp::new(pool).await;
    let content = png(800, 600);

    let uploaded = app
        .post_multipart("/api/attachments", "logo.png", "image/png", &content, &[])
        .await;
    let id = uploaded.id();

    let download = app.get(&format!("/api/attachments/{id}")).await;
    assert_eq!(download.status, StatusCode::OK);
    assert_eq!(download.body, content, "bytes come back unchanged");
    assert_eq!(
        download.header(header::CACHE_CONTROL),
        Some("private, max-age=31536000, immutable"),
        "content-addressed files are immutable"
    );
    assert!(download.header(header::ETAG).is_some());

    let thumbnail = app.get(&format!("/api/attachments/{id}/thumbnail")).await;
    assert_eq!(thumbnail.status, StatusCode::OK);
    assert_eq!(thumbnail.content_type.as_deref(), Some("image/jpeg"));
    let decoded = image::load_from_memory(&thumbnail.body).expect("thumbnail decodes");
    assert!(decoded.width() <= 320 && decoded.height() <= 320);
}

#[sqlx::test]
async fn identical_content_is_stored_once(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let content = png(120, 120);

    let first = app
        .post_multipart("/api/attachments", "a.png", "image/png", &content, &[])
        .await;
    let second = app
        .post_multipart("/api/attachments", "b.png", "image/png", &content, &[])
        .await;

    assert_eq!(first.json()["sha256"], second.json()["sha256"]);
    assert_ne!(
        first.id(),
        second.id(),
        "both uploads keep their own metadata row"
    );

    let distinct_hashes: i64 = sqlx::query_scalar("SELECT count(DISTINCT sha256) FROM attachment")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(distinct_hashes, 1, "dedup for free");
}

#[sqlx::test]
async fn non_image_uploads_have_no_thumbnail(pool: PgPool) {
    let app = TestApp::new(pool).await;

    let uploaded = app
        .post_multipart(
            "/api/attachments",
            "laborbericht.pdf",
            "application/pdf",
            b"%PDF-1.7 not really a pdf",
            &[],
        )
        .await;

    assert_eq!(uploaded.json()["has_thumbnail"], false);
    let thumbnail = app
        .get(&format!("/api/attachments/{}/thumbnail", uploaded.id()))
        .await;
    assert_eq!(thumbnail.status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn upload_without_a_file_part_is_rejected(pool: PgPool) {
    let app = TestApp::new(pool).await;

    let response = app
        .send_raw(
            axum::http::Method::POST,
            "/api/attachments",
            "multipart/form-data; boundary=b",
            b"--b\r\nContent-Disposition: form-data; name=\"kind\"\r\n\r\nreferenced\r\n--b--\r\n"
                .to_vec(),
        )
        .await;

    assert_eq!(response.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn attachment_kind_shape_is_enforced_by_the_database(pool: PgPool) {
    let app = TestApp::new(pool).await;

    // `patient_file` without a patient violates the kind-bound CHECK.
    let response = app
        .post_multipart(
            "/api/attachments",
            "x.png",
            "image/png",
            &png(10, 10),
            &[("kind", "patient_file")],
        )
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test]
async fn unknown_attachment_is_a_404(pool: PgPool) {
    let app = TestApp::new(pool).await;
    assert_eq!(
        app.get("/api/attachments/999").await.status,
        StatusCode::NOT_FOUND
    );
}

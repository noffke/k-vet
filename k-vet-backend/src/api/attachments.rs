//! Attachment upload and delivery (T018).

use axum::body::Body;
use axum::extract::multipart::Field;
use axum::extract::{Multipart, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use tokio_util::io::ReaderStream;
use utoipa::ToSchema;

use crate::AppState;
use crate::domain::enums::AttachmentKind;
use crate::domain::files::ChunkSource;
use crate::error::{AppError, AppResult};

/// Stored file metadata as returned by the API.
#[derive(Debug, Serialize, ToSchema)]
pub struct Attachment {
    pub id: i64,
    /// Content hash — the file's identity.
    pub sha256: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub orig_name: String,
    pub kind: AttachmentKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patient_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patient_treatment_id: Option<i64>,
    /// Date the document refers to (patient files), independent of the upload time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_date: Option<NaiveDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub has_thumbnail: bool,
    pub created_at: DateTime<Utc>,
}

/// Wraps a multipart field so it can stream into the content-addressed store.
struct FieldSource<'a> {
    field: Field<'a>,
}

impl ChunkSource for FieldSource<'_> {
    async fn next_chunk(&mut self) -> AppResult<Option<Vec<u8>>> {
        match self.field.chunk().await {
            Ok(Some(bytes)) => Ok(Some(bytes.to_vec())),
            Ok(None) => Ok(None),
            Err(error) => Err(AppError::BadRequest(format!("upload failed: {error}"))),
        }
    }
}

/// Metadata collected from the non-file parts of the upload.
#[derive(Default)]
struct UploadMeta {
    kind: Option<AttachmentKind>,
    patient_id: Option<i64>,
    patient_treatment_id: Option<i64>,
    reference_date: Option<NaiveDate>,
    note: Option<String>,
}

#[utoipa::path(
    post,
    operation_id = "uploadAttachment",
    path = "/api/attachments",
    tag = "attachments",
    request_body(content = String, description = "multipart/form-data with a `file` part \
        plus optional `kind`, `patient_id`, `patient_treatment_id`, `reference_date` and `note` parts",
        content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "Stored file", body = Attachment),
        (status = 413, description = "Upload exceeds the configured limit")
    )
)]
pub async fn upload(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> AppResult<Json<Attachment>> {
    let mut meta = UploadMeta::default();
    let mut stored = None;
    let mut orig_name = String::from("upload");
    let mut mime_type = String::from("application/octet-stream");

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| AppError::BadRequest(format!("malformed upload: {error}")))?
    {
        let name = field.name().unwrap_or_default().to_owned();
        match name.as_str() {
            "file" => {
                orig_name = field.file_name().unwrap_or("upload").to_owned();
                mime_type = field
                    .content_type()
                    .map(str::to_owned)
                    .unwrap_or_else(|| guess_mime(&orig_name));
                let source = FieldSource { field };
                let max_bytes = state
                    .config
                    .storage
                    .max_upload_mb
                    .saturating_mul(1024 * 1024);
                stored = Some(state.files.store(source, max_bytes).await?);
            }
            other => {
                let value = field
                    .text()
                    .await
                    .map_err(|error| AppError::BadRequest(format!("malformed upload: {error}")))?;
                apply_meta(&mut meta, other, &value)?;
            }
        }
    }

    let stored =
        stored.ok_or_else(|| AppError::BadRequest("no `file` part in upload".to_owned()))?;
    let kind = meta.kind.unwrap_or(AttachmentKind::Referenced);
    let has_thumbnail = state
        .files
        .generate_thumbnail(&stored.sha256, &mime_type)
        .await?;

    let attachment = sqlx::query_as!(
        Attachment,
        r#"INSERT INTO attachment
               (sha256, mime_type, size_bytes, orig_name, kind, patient_id, patient_treatment_id,
                reference_date, note, has_thumbnail)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
           RETURNING id, sha256, mime_type, size_bytes, orig_name,
                     kind AS "kind: AttachmentKind", patient_id, patient_treatment_id,
                     reference_date, note, has_thumbnail, created_at"#,
        stored.sha256,
        mime_type,
        stored.size_bytes,
        orig_name,
        kind as AttachmentKind,
        meta.patient_id,
        meta.patient_treatment_id,
        meta.reference_date,
        meta.note,
        has_thumbnail,
    )
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(attachment))
}

fn apply_meta(meta: &mut UploadMeta, field: &str, value: &str) -> AppResult<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    match field {
        "kind" => {
            meta.kind = Some(match trimmed {
                "patient_file" => AttachmentKind::PatientFile,
                "treatment_file" => AttachmentKind::TreatmentFile,
                "referenced" => AttachmentKind::Referenced,
                other => return Err(AppError::field("kind", format!("unknown kind `{other}`"))),
            });
        }
        "patient_id" => {
            meta.patient_id = Some(
                trimmed
                    .parse()
                    .map_err(|_| AppError::field("patient_id", "value.notANumber"))?,
            );
        }
        "patient_treatment_id" => {
            meta.patient_treatment_id = Some(
                trimmed
                    .parse()
                    .map_err(|_| AppError::field("patient_treatment_id", "value.notANumber"))?,
            );
        }
        "reference_date" => {
            meta.reference_date = Some(
                trimmed
                    .parse()
                    .map_err(|_| AppError::field("reference_date", "value.notADate"))?,
            );
        }
        "note" => meta.note = Some(trimmed.to_owned()),
        _ => {}
    }
    Ok(())
}

/// Content type from a file name; shared with [`crate::static_assets`].
pub(crate) fn guess_mime(file_name: &str) -> String {
    mime_guess::from_path(file_name)
        .first_or_octet_stream()
        .to_string()
}

#[utoipa::path(
    get,
    operation_id = "downloadAttachment",
    path = "/api/attachments/{id}",
    tag = "attachments",
    params(("id" = i64, Path, description = "Attachment id")),
    responses(
        (status = 200, description = "File content"),
        (status = 404, description = "Unknown attachment")
    )
)]
pub async fn download(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Response> {
    let row = sqlx::query!(
        "SELECT sha256, mime_type, orig_name, size_bytes FROM attachment WHERE id = $1",
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    stream_file(
        state.files.content_path(&row.sha256),
        &row.mime_type,
        &row.sha256,
        Some(&row.orig_name),
    )
    .await
}

#[utoipa::path(
    get,
    operation_id = "attachmentThumbnail",
    path = "/api/attachments/{id}/thumbnail",
    tag = "attachments",
    params(("id" = i64, Path, description = "Attachment id")),
    responses(
        (status = 200, description = "JPEG thumbnail"),
        (status = 404, description = "Unknown attachment or not an image")
    )
)]
pub async fn thumbnail(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Response> {
    let row = sqlx::query!(
        "SELECT sha256, mime_type, has_thumbnail FROM attachment WHERE id = $1",
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    if !row.has_thumbnail {
        // Older rows, or images whose thumbnail generation failed: try once more.
        let created = state
            .files
            .generate_thumbnail(&row.sha256, &row.mime_type)
            .await?;
        if !created {
            return Err(AppError::NotFound);
        }
        sqlx::query!(
            "UPDATE attachment SET has_thumbnail = true WHERE id = $1",
            id
        )
        .execute(&state.pool)
        .await?;
    }

    stream_file(
        state.files.thumbnail_path(&row.sha256),
        "image/jpeg",
        &row.sha256,
        None,
    )
    .await
}

/// Streams a file from disk with immutable caching (content-addressed paths never change).
async fn stream_file(
    path: std::path::PathBuf,
    mime_type: &str,
    sha256: &str,
    download_name: Option<&str>,
) -> AppResult<Response> {
    let file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tracing::error!(path = %path.display(), "attachment row without file on disk");
            return Err(AppError::NotFound);
        }
        Err(error) => return Err(error.into()),
    };

    let mut headers = HeaderMap::new();
    insert_header(&mut headers, header::CONTENT_TYPE, mime_type);
    insert_header(
        &mut headers,
        header::CACHE_CONTROL,
        "private, max-age=31536000, immutable",
    );
    insert_header(&mut headers, header::ETAG, &format!("\"{sha256}\""));
    if let Some(name) = download_name {
        let sanitized: String = name.chars().filter(|c| *c != '"' && *c != '\\').collect();
        insert_header(
            &mut headers,
            header::CONTENT_DISPOSITION,
            &format!("inline; filename=\"{sanitized}\""),
        );
    }

    Ok((
        StatusCode::OK,
        headers,
        Body::from_stream(ReaderStream::new(file)),
    )
        .into_response())
}

/// Adds a header, skipping values that cannot be encoded rather than failing the download.
fn insert_header(headers: &mut HeaderMap, name: header::HeaderName, value: &str) {
    match HeaderValue::from_str(value) {
        Ok(value) => {
            headers.insert(name, value);
        }
        Err(error) => tracing::warn!(%error, %name, "skipping unencodable header value"),
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/attachments", post(upload))
        .route("/attachments/{id}", get(download))
        .route("/attachments/{id}/thumbnail", get(thumbnail))
}

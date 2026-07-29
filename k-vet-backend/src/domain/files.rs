//! Content-addressed attachment storage.
//!
//! Uploads stream to a temporary file while being hashed, then move atomically to
//! `attachments/ab/cd/<sha256>` — identical files deduplicate for free. Thumbnails are
//! derived from the source hash and live under `attachments/thumbnails/ab/cd/<sha256>.jpg`.

use std::path::{Path, PathBuf};

use image::ImageReader;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::error::{AppError, AppResult};

/// Longest edge of a generated thumbnail, in pixels.
const THUMBNAIL_MAX_EDGE: u32 = 320;

#[derive(Debug, Clone)]
pub struct AttachmentStore {
    root: PathBuf,
}

/// Result of storing an upload.
#[derive(Debug, Clone)]
pub struct StoredFile {
    pub sha256: String,
    pub size_bytes: i64,
    /// `false` when the very same bytes were already on disk.
    pub newly_written: bool,
}

impl AttachmentStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `attachments/ab/cd/<sha256>`
    pub fn content_path(&self, sha256: &str) -> PathBuf {
        let (first, second) = shard(sha256);
        self.root.join(first).join(second).join(sha256)
    }

    /// `attachments/thumbnails/ab/cd/<sha256>.jpg`
    pub fn thumbnail_path(&self, sha256: &str) -> PathBuf {
        let (first, second) = shard(sha256);
        self.root
            .join("thumbnails")
            .join(first)
            .join(second)
            .join(format!("{sha256}.jpg"))
    }

    /// Streams `chunks` to disk while hashing, then moves the file into place.
    ///
    /// `max_bytes` guards against a runaway upload; the temporary file is removed on error.
    pub async fn store<S>(&self, mut chunks: S, max_bytes: u64) -> AppResult<StoredFile>
    where
        S: ChunkSource,
    {
        let temp_dir = self.root.join("tmp");
        tokio::fs::create_dir_all(&temp_dir).await?;
        let temp_path = temp_dir.join(format!("upload-{}", unique_suffix()));

        let mut file = tokio::fs::File::create(&temp_path).await?;
        let mut hasher = Sha256::new();
        let mut size: u64 = 0;

        loop {
            match chunks.next_chunk().await {
                Ok(Some(chunk)) => {
                    size = size.saturating_add(chunk.len() as u64);
                    if size > max_bytes {
                        drop(file);
                        let _ = tokio::fs::remove_file(&temp_path).await;
                        return Err(AppError::PayloadTooLarge);
                    }
                    hasher.update(&chunk);
                    if let Err(error) = file.write_all(&chunk).await {
                        drop(file);
                        let _ = tokio::fs::remove_file(&temp_path).await;
                        return Err(error.into());
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    drop(file);
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    return Err(error);
                }
            }
        }

        file.flush().await?;
        file.sync_all().await?;
        drop(file);

        let sha256 = hex::encode(hasher.finalize());
        let target = self.content_path(&sha256);
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let newly_written = !tokio::fs::try_exists(&target).await.unwrap_or(false);
        if newly_written {
            tokio::fs::rename(&temp_path, &target).await?;
        } else {
            // Same bytes already stored — drop the duplicate.
            let _ = tokio::fs::remove_file(&temp_path).await;
        }

        Ok(StoredFile {
            sha256,
            size_bytes: i64::try_from(size).unwrap_or(i64::MAX),
            newly_written,
        })
    }

    /// Generates a thumbnail for image attachments. Returns `false` for non-images.
    pub async fn generate_thumbnail(&self, sha256: &str, mime_type: &str) -> AppResult<bool> {
        if !mime_type.starts_with("image/") {
            return Ok(false);
        }
        let source = self.content_path(sha256);
        let target = self.thumbnail_path(sha256);
        if tokio::fs::try_exists(&target).await.unwrap_or(false) {
            return Ok(true);
        }
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Decoding is CPU-bound: keep it off the async runtime's worker threads.
        let created = tokio::task::spawn_blocking(move || write_thumbnail(&source, &target))
            .await
            .map_err(|error| AppError::internal("joining thumbnail task", error))?;
        match created {
            Ok(()) => Ok(true),
            Err(error) => {
                tracing::warn!(%error, sha256, "thumbnail generation failed");
                Ok(false)
            }
        }
    }

    /// Deletes content and thumbnail of a hash — used by the monthly orphan sweep.
    pub async fn remove(&self, sha256: &str) -> AppResult<()> {
        let _ = tokio::fs::remove_file(self.content_path(sha256)).await;
        let _ = tokio::fs::remove_file(self.thumbnail_path(sha256)).await;
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn write_thumbnail(source: &Path, target: &Path) -> Result<(), String> {
    let reader = ImageReader::open(source)
        .map_err(|error| format!("opening image: {error}"))?
        .with_guessed_format()
        .map_err(|error| format!("guessing image format: {error}"))?;
    let image = reader
        .decode()
        .map_err(|error| format!("decoding image: {error}"))?;
    image
        .thumbnail(THUMBNAIL_MAX_EDGE, THUMBNAIL_MAX_EDGE)
        .into_rgb8()
        .save_with_format(target, image::ImageFormat::Jpeg)
        .map_err(|error| format!("writing thumbnail: {error}"))
}

/// Two-level sharding so no directory grows unbounded.
fn shard(sha256: &str) -> (&str, &str) {
    let first = sha256.get(0..2).unwrap_or("00");
    let second = sha256.get(2..4).unwrap_or("00");
    (first, second)
}

/// Unique-enough temp file name without pulling in a UUID dependency.
fn unique_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("{nanos:x}-{counter:x}-{}", std::process::id())
}

/// Abstracts "the next chunk of bytes", so uploads and tests share one code path.
pub trait ChunkSource {
    fn next_chunk(&mut self) -> impl Future<Output = AppResult<Option<Vec<u8>>>> + Send;
}

/// Chunk source over an in-memory buffer (tests, generated PDFs).
pub struct BytesSource {
    data: Option<Vec<u8>>,
}

impl BytesSource {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data: Some(data) }
    }
}

impl ChunkSource for BytesSource {
    async fn next_chunk(&mut self) -> AppResult<Option<Vec<u8>>> {
        Ok(self.data.take())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_path_is_sharded_by_the_first_two_byte_pairs() {
        let store = AttachmentStore::new("/data/attachments");
        let hash = "abcdef0000000000000000000000000000000000000000000000000000000000";
        assert_eq!(
            store.content_path(hash),
            PathBuf::from(format!("/data/attachments/ab/cd/{hash}"))
        );
        assert_eq!(
            store.thumbnail_path(hash),
            PathBuf::from(format!("/data/attachments/thumbnails/ab/cd/{hash}.jpg"))
        );
    }

    #[tokio::test]
    async fn stores_hashes_and_deduplicates() {
        let root = tempfile::tempdir().expect("temp dir");
        let store = AttachmentStore::new(root.path());

        let first = store
            .store(BytesSource::new(b"hello vet".to_vec()), 1024)
            .await
            .expect("stores");
        assert_eq!(first.size_bytes, 9);
        assert!(first.newly_written);
        assert!(store.content_path(&first.sha256).exists());

        let second = store
            .store(BytesSource::new(b"hello vet".to_vec()), 1024)
            .await
            .expect("stores again");
        assert_eq!(second.sha256, first.sha256, "content addressed");
        assert!(!second.newly_written, "identical bytes deduplicate");

        // No temporary files are left behind.
        let mut entries = tokio::fs::read_dir(root.path().join("tmp"))
            .await
            .expect("tmp dir");
        assert!(entries.next_entry().await.expect("readable").is_none());
    }

    #[tokio::test]
    async fn rejects_oversized_uploads_without_leaving_files() {
        let root = tempfile::tempdir().expect("temp dir");
        let store = AttachmentStore::new(root.path());

        let error = store
            .store(BytesSource::new(vec![0u8; 32]), 16)
            .await
            .expect_err("limit is enforced");
        assert!(matches!(error, AppError::PayloadTooLarge));

        let mut entries = tokio::fs::read_dir(root.path().join("tmp"))
            .await
            .expect("tmp dir");
        assert!(entries.next_entry().await.expect("readable").is_none());
    }

    #[tokio::test]
    async fn thumbnails_are_only_generated_for_images() {
        let root = tempfile::tempdir().expect("temp dir");
        let store = AttachmentStore::new(root.path());
        let stored = store
            .store(BytesSource::new(b"%PDF-1.7".to_vec()), 1024)
            .await
            .expect("stores");

        let created = store
            .generate_thumbnail(&stored.sha256, "application/pdf")
            .await
            .expect("no error for non-images");
        assert!(!created);
    }

    #[tokio::test]
    async fn generates_a_thumbnail_for_a_png() {
        let root = tempfile::tempdir().expect("temp dir");
        let store = AttachmentStore::new(root.path());

        let mut png = Vec::new();
        let image = image::RgbImage::from_pixel(600, 400, image::Rgb([12, 80, 60]));
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encodes png");

        let stored = store
            .store(BytesSource::new(png), 1024 * 1024)
            .await
            .expect("stores");
        let created = store
            .generate_thumbnail(&stored.sha256, "image/png")
            .await
            .expect("thumbnail works");

        assert!(created);
        let thumbnail = store.thumbnail_path(&stored.sha256);
        assert!(thumbnail.exists());
        let decoded = image::open(&thumbnail).expect("thumbnail is a readable image");
        assert!(decoded.width() <= THUMBNAIL_MAX_EDGE && decoded.height() <= THUMBNAIL_MAX_EDGE);
    }
}

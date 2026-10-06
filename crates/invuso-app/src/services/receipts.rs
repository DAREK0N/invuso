//! Receipt images: archiving a new photo with its thumbnail (RCP-01..03,
//! CORE-06) and serving the files to the WebView (RCP-04, GRP-21).

use std::io::Cursor;
use std::path::{Path, PathBuf};

use dioxus::mobile::wry::http::{Response, StatusCode};
use dioxus::mobile::{AssetRequest, RequestAsyncResponder, use_asset_handler};
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageDecoder, ImageReader};
use thiserror::Error;

use crate::platform::{self, ImageKind, ImageSource, PickOutcome};
use crate::storage::{Db, ReceiptFiles, StorageError};

/// Folder of the receipt images inside the data directory.
pub(crate) const RECEIPTS_DIR: &str = "receipts";

/// URL prefix under which the WebView loads receipt files.
const URL_PREFIX: &str = "receipt-files";

/// Longest side of a thumbnail in pixels: sharp in a list row and across
/// the full-width detail card on high-density screens, yet below 100 KB.
const THUMBNAIL_SIZE: u32 = 960;

#[derive(Debug, Error)]
pub enum ReceiptError {
    #[error("{0}")]
    Platform(String),
    #[error("the image could not be stored: {0}")]
    Io(#[from] std::io::Error),
    #[error("the image could not be read: {0}")]
    Image(String),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// Lets the user take or choose a photo and archives it as a new receipt
/// (status `new`). `None` if the user backed out.
pub async fn capture_receipt(
    db: Db,
    kind: ImageKind,
) -> Result<Option<ReceiptFiles>, ReceiptError> {
    let data_dir = platform::data_dir().map_err(ReceiptError::Platform)?;
    std::fs::create_dir_all(data_dir.join(RECEIPTS_DIR))?;
    let name = uuid::Uuid::now_v7().to_string();
    let staged = data_dir.join(RECEIPTS_DIR).join(format!("{name}.part"));

    let outcome = platform::image_source()
        .pick(kind, &staged)
        .await
        .map_err(ReceiptError::Platform)?;
    if outcome == PickOutcome::Cancelled {
        // Nothing of the user's: at most an empty or partial copy.
        let _ = std::fs::remove_file(&staged);
        return Ok(None);
    }
    // Decoding a 12-megapixel photo takes a moment; keep it off the UI.
    tokio::task::spawn_blocking(move || archive(&db, &data_dir, &staged, &name))
        .await
        .map_err(|e| ReceiptError::Platform(e.to_string()))?
        .map(Some)
}

/// Whether this device can take (`Camera`) or choose (`Gallery`) photos.
pub fn supports(kind: ImageKind) -> bool {
    platform::image_source().supports(kind)
}

/// Moves the staged copy to its final name, writes the thumbnail and
/// records both. An image that cannot be decoded is archived anyway,
/// without a thumbnail.
fn archive(
    db: &Db,
    data_dir: &Path,
    staged: &Path,
    name: &str,
) -> Result<ReceiptFiles, ReceiptError> {
    let bytes = std::fs::read(staged)?;
    let extension = image::guess_format(&bytes)
        .ok()
        .and_then(|format| format.extensions_str().first().copied())
        .unwrap_or("img");
    let original = format!("{RECEIPTS_DIR}/{name}.{extension}");
    std::fs::rename(staged, data_dir.join(&original))?;

    let thumbnail_path = match thumbnail(&bytes) {
        Ok(jpeg) => {
            let path = format!("{RECEIPTS_DIR}/{name}_thumb.jpg");
            std::fs::write(data_dir.join(&path), jpeg)?;
            Some(path)
        }
        Err(_) => None,
    };
    Ok(db.create_receipt(&original, thumbnail_path.as_deref())?)
}

impl From<image::ImageError> for ReceiptError {
    fn from(error: image::ImageError) -> Self {
        Self::Image(error.to_string())
    }
}

/// JPEG preview of an image, turned upright by its EXIF orientation.
fn thumbnail(bytes: &[u8]) -> Result<Vec<u8>, image::ImageError> {
    thumbnail_of(&decode_upright(bytes)?)
}

/// Decodes an image file turned upright by its EXIF orientation, the way
/// the WebView shows it.
pub(crate) fn decode_upright(bytes: &[u8]) -> Result<DynamicImage, image::ImageError> {
    let mut decoder = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()?
        .into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(image)
}

/// JPEG preview of an upright image.
pub(crate) fn thumbnail_of(image: &DynamicImage) -> Result<Vec<u8>, image::ImageError> {
    let small = image.thumbnail(THUMBNAIL_SIZE, THUMBNAIL_SIZE).into_rgb8();
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 80).encode_image(&small)?;
    Ok(jpeg)
}

/// URL of a stored receipt file (path relative to the data directory) for
/// `img src`.
pub fn file_url(path: &str) -> String {
    let file = path
        .strip_prefix(&format!("{RECEIPTS_DIR}/"))
        .unwrap_or(path);
    format!("/{URL_PREFIX}/{file}")
}

/// Serves `/receipt-files/<file>` from the receipts folder for the rest of
/// the app's life; call once at the root.
pub fn use_receipt_files() {
    use_asset_handler(URL_PREFIX, |request: AssetRequest, responder| {
        // Originals are several megabytes; read them off the UI thread.
        std::thread::spawn(move || respond(request.uri().path(), responder));
    });
}

fn respond(url_path: &str, responder: RequestAsyncResponder) {
    let response =
        match resolve(url_path).and_then(|path| std::fs::read(&path).ok().map(|b| (path, b))) {
            Some((path, bytes)) => Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", content_type(&path))
                .body(bytes),
            None => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Vec::new()),
        };
    if let Ok(response) = response {
        responder.respond(response);
    }
}

/// The file behind a URL path; `None` for anything but a plain file name,
/// so no request can reach outside the receipts folder.
fn resolve(url_path: &str) -> Option<PathBuf> {
    let file = url_path
        .trim_start_matches('/')
        .strip_prefix(URL_PREFIX)?
        .strip_prefix('/')?;
    let plain = !file.is_empty()
        && file
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !file.starts_with('.');
    if !plain {
        return None;
    }
    Some(platform::data_dir().ok()?.join(RECEIPTS_DIR).join(file))
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use image::{ImageFormat, RgbImage};

    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb8(RgbImage::new(width, height))
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn thumbnail_is_a_small_jpeg_keeping_the_aspect_ratio() {
        let jpeg = thumbnail(&png(1200, 3000)).unwrap();
        assert_eq!(image::guess_format(&jpeg).unwrap(), ImageFormat::Jpeg);
        let small = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((small.width(), small.height()), (384, 960));
    }

    #[test]
    fn undecodable_bytes_have_no_thumbnail() {
        assert!(thumbnail(b"not an image").is_err());
    }

    #[test]
    fn archives_original_and_thumbnail() {
        let db = Db::open_in_memory().unwrap();
        let dir = std::env::temp_dir().join(format!("invuso-receipts-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(dir.join(RECEIPTS_DIR)).unwrap();
        let staged = dir.join(RECEIPTS_DIR).join("r1.part");
        std::fs::write(&staged, png(40, 20)).unwrap();

        let receipt = archive(&db, &dir, &staged, "r1").unwrap();
        assert_eq!(receipt.image_paths, ["receipts/r1.png"]);
        assert_eq!(
            receipt.thumbnail_path.as_deref(),
            Some("receipts/r1_thumb.jpg")
        );
        assert!(!staged.exists());
        assert!(dir.join("receipts/r1.png").exists());
        assert!(dir.join("receipts/r1_thumb.jpg").exists());
        assert_eq!(db.receipt(&receipt.id).unwrap(), Some(receipt));

        // An unknown format is kept as it is, without preview.
        std::fs::write(&staged, b"heic?").unwrap();
        let odd = archive(&db, &dir, &staged, "r2").unwrap();
        assert_eq!(odd.image_paths, ["receipts/r2.img"]);
        assert_eq!(odd.thumbnail_path, None);
        assert!(dir.join("receipts/r2.img").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn urls_map_to_plain_file_names_only() {
        assert_eq!(
            file_url("receipts/a_thumb.jpg"),
            "/receipt-files/a_thumb.jpg"
        );
        // Host builds have no data directory, so even valid names resolve
        // to nothing; the checks before that are what matters here.
        assert_eq!(resolve("/receipt-files/../invuso.sqlite3"), None);
        assert_eq!(resolve("/receipt-files/a/b.jpg"), None);
        assert_eq!(resolve("/receipt-files/"), None);
        assert_eq!(resolve("/other/a.jpg"), None);
        assert_eq!(content_type(Path::new("a.jpg")), "image/jpeg");
        assert_eq!(content_type(Path::new("a.img")), "application/octet-stream");
    }
}

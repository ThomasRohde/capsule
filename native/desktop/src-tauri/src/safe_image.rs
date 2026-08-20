//! Fail-closed projection of capsule-declared icons into host-owned static PNGs.

use std::{
    fmt::{self, Write as _},
    io::{BufReader, Cursor},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::{
    ColorType, DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder, Limits,
    codecs::{
        png::{PngDecoder, PngEncoder},
        webp::WebPDecoder,
    },
};
use sha2::{Digest, Sha256};
use sqlite_capsule_launch::RetainedLaunchInspection;

const MAX_COMPRESSED_BYTES: usize = 512 * 1024;
const MAX_DIMENSION: u32 = 1024;
const MAX_RGBA_BYTES: u64 = 4 * 1024 * 1024;
const MAX_DERIVATIVE_PNG_BYTES: usize = (4 * 1024 * 1024) + (64 * 1024);
const PNG_DATA_URL_PREFIX: &str = "data:image/png;base64,";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SafeImageSelection {
    ApplicationIcon,
    InstanceIcon,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SafeImageDerivative {
    data_url: String,
    width: u32,
    height: u32,
}

impl SafeImageDerivative {
    pub(crate) fn data_url(&self) -> &str {
        &self.data_url
    }

    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    pub(crate) fn height(&self) -> u32 {
        self.height
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SafeImageError {
    SnapshotUnavailable,
}

impl fmt::Display for SafeImageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the verified capsule snapshot is no longer available")
    }
}

impl std::error::Error for SafeImageError {}

#[derive(Debug)]
struct DeclaredImage {
    media_type: Option<String>,
    byte_len: Option<i64>,
    content: Option<Vec<u8>>,
    sha256: Option<String>,
    declared_width: Option<i64>,
    declared_height: Option<i64>,
}

#[derive(Clone, Copy)]
enum DeclaredMedia {
    Png,
    WebP,
}

/// Project only a selected v0.3 application or instance icon declaration from
/// the exact snapshot retained by launch inspection. Invalid, unsupported, or
/// oversized media deterministically becomes `None`; stale source identity is
/// reported separately so callers can force a fresh inspection.
pub(crate) fn project_safe_image(
    retained: &RetainedLaunchInspection,
    selection: SafeImageSelection,
) -> Result<Option<SafeImageDerivative>, SafeImageError> {
    if retained.inspection().identity.format_version != "0.3" {
        return Ok(None);
    }
    retained
        .assert_source_current()
        .map_err(|_| SafeImageError::SnapshotUnavailable)?;

    let declared = retained.project_snapshot(|connection| {
        let sql = match selection {
            SafeImageSelection::ApplicationIcon => {
                "SELECT asset.media_type, length(asset.content), \
                        CASE WHEN length(asset.content) <= ?1 THEN asset.content END, \
                        asset.sha256, NULL, NULL \
                 FROM capsule_application AS application \
                 LEFT JOIN capsule_asset AS asset \
                   ON asset.path = application.icon_asset \
                 WHERE application.id = 1"
            }
            SafeImageSelection::InstanceIcon => {
                "SELECT asset.media_type, length(asset.content), \
                        CASE WHEN length(asset.content) <= ?1 THEN asset.content END, \
                        asset.sha256, asset.width, asset.height \
                 FROM capsule_instance AS instance \
                 LEFT JOIN capsule_instance_asset AS asset \
                   ON asset.id = instance.icon_asset_id \
                 WHERE instance.id = 1"
            }
        };
        connection
            .query_row(sql, [MAX_COMPRESSED_BYTES as i64], |row| {
                Ok(DeclaredImage {
                    media_type: row.get(0)?,
                    byte_len: row.get(1)?,
                    content: row.get(2)?,
                    sha256: row.get(3)?,
                    declared_width: row.get(4)?,
                    declared_height: row.get(5)?,
                })
            })
            .ok()
    });
    let derivative = declared.and_then(process_declared_image);

    retained
        .assert_source_current()
        .map_err(|_| SafeImageError::SnapshotUnavailable)?;
    Ok(derivative)
}

fn process_declared_image(declared: DeclaredImage) -> Option<SafeImageDerivative> {
    let byte_len = usize::try_from(declared.byte_len?).ok()?;
    let content = declared.content?;
    if byte_len == 0 || byte_len > MAX_COMPRESSED_BYTES || byte_len != content.len() {
        return None;
    }

    let media = match declared.media_type?.as_str() {
        "image/png" => DeclaredMedia::Png,
        "image/webp" => DeclaredMedia::WebP,
        _ => return None,
    };
    let expected_sha256 = declared.sha256?;
    if expected_sha256.len() != 64
        || !expected_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || sha256_hex(&content) != expected_sha256
    {
        return None;
    }

    let declared_dimensions = match (declared.declared_width, declared.declared_height) {
        (None, None) => None,
        (Some(width), Some(height)) => {
            Some((u32::try_from(width).ok()?, u32::try_from(height).ok()?))
        }
        _ => return None,
    };
    decode_static_png_derivative(&content, media, declared_dimensions)
}

fn decode_static_png_derivative(
    content: &[u8],
    media: DeclaredMedia,
    declared_dimensions: Option<(u32, u32)>,
) -> Option<SafeImageDerivative> {
    let limits = decoder_limits();
    let image = match media {
        DeclaredMedia::Png => {
            if !content.starts_with(b"\x89PNG\r\n\x1a\n") {
                return None;
            }
            let decoder =
                PngDecoder::with_limits(BufReader::new(Cursor::new(content)), limits).ok()?;
            if decoder.is_apng().ok()? {
                return None;
            }
            decode_one_static_image(decoder, declared_dimensions)?
        }
        DeclaredMedia::WebP => {
            if content.len() < 12 || &content[..4] != b"RIFF" || &content[8..12] != b"WEBP" {
                return None;
            }
            let mut decoder = WebPDecoder::new(BufReader::new(Cursor::new(content))).ok()?;
            if decoder.has_animation() {
                return None;
            }
            decoder.set_limits(limits).ok()?;
            decode_one_static_image(decoder, declared_dimensions)?
        }
    };

    let width = image.width();
    let height = image.height();
    let rgba = image.into_rgba8();
    if u64::try_from(rgba.as_raw().len()).ok()? > MAX_RGBA_BYTES {
        return None;
    }

    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(rgba.as_raw(), width, height, ExtendedColorType::Rgba8)
        .ok()?;
    if png.len() > MAX_DERIVATIVE_PNG_BYTES {
        return None;
    }
    let data_url = format!("{PNG_DATA_URL_PREFIX}{}", STANDARD.encode(&png));
    let max_data_url_bytes =
        PNG_DATA_URL_PREFIX.len() + MAX_DERIVATIVE_PNG_BYTES.div_ceil(3).checked_mul(4)?;
    if data_url.len() > max_data_url_bytes {
        return None;
    }
    Some(SafeImageDerivative {
        data_url,
        width,
        height,
    })
}

fn decode_one_static_image(
    decoder: impl ImageDecoder,
    declared_dimensions: Option<(u32, u32)>,
) -> Option<DynamicImage> {
    let dimensions = decoder.dimensions();
    if dimensions.0 == 0
        || dimensions.1 == 0
        || dimensions.0 > MAX_DIMENSION
        || dimensions.1 > MAX_DIMENSION
        || declared_dimensions.is_some_and(|declared| declared != dimensions)
    {
        return None;
    }
    let rgba_bytes = u64::from(dimensions.0)
        .checked_mul(u64::from(dimensions.1))?
        .checked_mul(4)?;
    if rgba_bytes > MAX_RGBA_BYTES
        || decoder.total_bytes() > MAX_RGBA_BYTES
        || !matches!(
            decoder.color_type(),
            ColorType::L8 | ColorType::La8 | ColorType::Rgb8 | ColorType::Rgba8
        )
    {
        return None;
    }
    let image = DynamicImage::from_decoder(decoder).ok()?;
    ((image.width(), image.height()) == dimensions).then_some(image)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

fn decoder_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_RGBA_BYTES);
    limits
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicUsize, Ordering},
    };

    use image::codecs::webp::WebPEncoder;
    use rusqlite::Connection;
    use sqlite_capsule_launch::inspect_launch_retained;

    use super::*;

    static TEST_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    struct TestCapsule(PathBuf);

    impl TestCapsule {
        fn v03(name: &str) -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "sqlite-capsule-safe-image-{name}-{}-{sequence}.sqlitecapsule",
                std::process::id()
            ));
            let connection = Connection::open(&path).expect("create v0.3 icon fixture");
            connection
                .execute_batch(include_str!("../../../../format/capsule-v0.3.sql"))
                .expect("create v0.3 format");
            connection
                .execute_batch(include_str!(
                    "../../../../format/capsule-signed-app-v0.3.sql"
                ))
                .expect("create v0.3 signed-app extension");
            connection
                .execute_batch(include_str!(
                    "../../../../compatibility/signed-app-v0.3/fixture-v0.3.sql"
                ))
                .expect("seed v0.3 fixture");
            drop(connection);
            Self(path)
        }
    }

    impl Drop for TestCapsule {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn checked_v02_capsule() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join("capsules/diagram-studio.capsule.sqlite")
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let rgba = vec![0x7f; usize::try_from(u64::from(width) * u64::from(height) * 4).unwrap()];
        let mut encoded = Vec::new();
        PngEncoder::new(&mut encoded)
            .write_image(&rgba, width, height, ExtendedColorType::Rgba8)
            .expect("encode PNG test image");
        encoded
    }

    fn webp(width: u32, height: u32) -> Vec<u8> {
        let rgba = vec![0x55; usize::try_from(u64::from(width) * u64::from(height) * 4).unwrap()];
        let mut encoded = Vec::new();
        WebPEncoder::new_lossless(&mut encoded)
            .write_image(&rgba, width, height, ExtendedColorType::Rgba8)
            .expect("encode WebP test image");
        encoded
    }

    fn declared(
        media_type: &str,
        content: Vec<u8>,
        dimensions: Option<(u32, u32)>,
    ) -> DeclaredImage {
        let sha256 = sha256_hex(&content);
        DeclaredImage {
            media_type: Some(media_type.to_owned()),
            byte_len: Some(content.len() as i64),
            content: Some(content),
            sha256: Some(sha256),
            declared_width: dimensions.map(|value| i64::from(value.0)),
            declared_height: dimensions.map(|value| i64::from(value.1)),
        }
    }

    #[test]
    fn projects_application_and_instance_icons_from_the_retained_snapshot() {
        let capsule = TestCapsule::v03("snapshot-projection");
        let retained = inspect_launch_retained(&capsule.0).expect("inspect icon fixture");
        for selection in [
            SafeImageSelection::ApplicationIcon,
            SafeImageSelection::InstanceIcon,
        ] {
            let derivative = project_safe_image(&retained, selection)
                .expect("snapshot remains current")
                .expect("valid selected icon");
            assert_eq!((derivative.width(), derivative.height()), (1, 1));
            assert!(derivative.data_url().starts_with(PNG_DATA_URL_PREFIX));
            assert!(!derivative.data_url().contains("<svg"));
        }
    }

    #[test]
    fn legacy_v02_has_no_safe_image_projection() {
        let retained =
            inspect_launch_retained(&checked_v02_capsule()).expect("inspect v0.2 fixture");
        assert_eq!(
            project_safe_image(&retained, SafeImageSelection::ApplicationIcon)
                .expect("v0.2 fallback"),
            None
        );
    }

    #[test]
    fn accepts_static_png_and_webp_and_reencodes_both_as_metadata_free_png() {
        for (media_type, bytes) in [("image/png", png(2, 1)), ("image/webp", webp(2, 1))] {
            let derivative = process_declared_image(declared(media_type, bytes, Some((2, 1))))
                .expect("valid static image");
            assert_eq!((derivative.width(), derivative.height()), (2, 1));
            let png = STANDARD
                .decode(
                    derivative
                        .data_url()
                        .strip_prefix(PNG_DATA_URL_PREFIX)
                        .unwrap(),
                )
                .expect("decode derivative data URL");
            assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
            for forbidden in [b"eXIf".as_slice(), b"iCCP", b"tEXt", b"acTL"] {
                assert!(
                    !png.windows(forbidden.len())
                        .any(|window| window == forbidden)
                );
            }
        }
    }

    #[test]
    fn rejects_hash_type_magic_size_dimension_and_decode_failures() {
        let valid = png(2, 1);

        let mut bad_hash = declared("image/png", valid.clone(), Some((2, 1)));
        bad_hash.sha256 = Some("0".repeat(64));
        assert!(process_declared_image(bad_hash).is_none());
        assert!(
            process_declared_image(declared("image/svg+xml", valid.clone(), Some((2, 1))))
                .is_none()
        );
        assert!(
            process_declared_image(declared("image/webp", valid.clone(), Some((2, 1)))).is_none()
        );

        let oversized = vec![0_u8; MAX_COMPRESSED_BYTES + 1];
        assert!(process_declared_image(declared("image/png", oversized, None)).is_none());
        assert!(
            process_declared_image(declared("image/png", valid.clone(), Some((1, 1)))).is_none()
        );
        assert!(
            process_declared_image(declared(
                "image/png",
                b"\x89PNG\r\n\x1a\ntruncated".to_vec(),
                None
            ))
            .is_none()
        );

        let dimension_bomb = png(MAX_DIMENSION + 1, 1);
        assert!(dimension_bomb.len() <= MAX_COMPRESSED_BYTES);
        assert!(process_declared_image(declared("image/png", dimension_bomb, None)).is_none());
    }

    #[test]
    fn rejects_declared_animation_before_reencoding() {
        let mut animated = png(1, 1);
        let mut chunk = Vec::new();
        chunk.extend_from_slice(&8_u32.to_be_bytes());
        chunk.extend_from_slice(b"acTL");
        chunk.extend_from_slice(&1_u32.to_be_bytes());
        chunk.extend_from_slice(&0_u32.to_be_bytes());
        chunk.extend_from_slice(&crc32(&chunk[4..]).to_be_bytes());
        animated.splice(33..33, chunk);
        assert!(process_declared_image(declared("image/png", animated, Some((1, 1)))).is_none());
    }

    #[test]
    fn stale_source_requires_reinspection_instead_of_reusing_the_derivative() {
        let capsule = TestCapsule::v03("stale-source");
        let retained = inspect_launch_retained(&capsule.0).expect("inspect icon fixture");
        let connection = Connection::open(&capsule.0).expect("open external writer");
        connection
            .execute(
                "UPDATE capsule_instance SET title = title || ' changed' WHERE id = 1",
                [],
            )
            .expect("change source after inspection");
        drop(connection);
        assert_eq!(
            project_safe_image(&retained, SafeImageSelection::InstanceIcon),
            Err(SafeImageError::SnapshotUnavailable)
        );
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xffff_ffff_u32;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320_u32 & (0_u32.wrapping_sub(crc & 1)));
            }
        }
        !crc
    }
}

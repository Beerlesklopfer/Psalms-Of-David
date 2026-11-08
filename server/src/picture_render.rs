// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The pictures as the apps and the browser show them (docs/spec-pictures.md): a preview for the grid and a screen size for the slideshow,
//! always a JPEG, made by the server from the file (project owner, 2026-11-08: the server makes them, also for the old folders), kept in
//! `<data dir>/picture-cache/` and made again when the file is newer. The originals are only read, never changed.
//!
//! - JPEG, PNG, WebP, GIF, BMP, TIFF: the `image` crate, turned upright by the orientation in the EXIF data.
//! - HEIC, HEIF, AVIF (the iPhones): libheif on Linux (it turns them upright itself); elsewhere there is no rendering of them.
//! - Videos: one frame through `ffmpeg` when it is installed (the package recommends it), else nothing (the apps show a symbol).
//!
//! At most [`PARALLEL`] pictures are made at once, so a slideshow on three devices does not take the whole server.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use image::{DynamicImage, ImageDecoder, ImageReader};

/// How many pictures are made at once.
pub const PARALLEL: usize = 2;

/// What is made of a picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    /// For the grid: at most 480 px on the longer side.
    Thumb,
    /// For the slideshow: at most 2048 px on the longer side (about a tablet, a television at full HD).
    Screen,
}

impl Size {
    pub fn px(self) -> u32 {
        match self {
            Size::Thumb => 480,
            Size::Screen => 2048,
        }
    }

    fn quality(self) -> u8 {
        match self {
            Size::Thumb => 78,
            Size::Screen => 85,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Size::Thumb => "thumb",
            Size::Screen => "screen",
        }
    }
}

/// The folder of the made pictures.
pub fn cache_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("picture-cache")
}

/// Where the made picture of `key` (`b12`, `l7`) in this size is kept.
pub fn cache_path(data_dir: &Path, key: &str, size: Size) -> PathBuf {
    cache_dir(data_dir).join(format!("{key}-{}.jpg", size.name()))
}

/// Removes what was made of a picture that is gone.
pub fn forget(data_dir: &Path, key: &str) {
    for size in [Size::Thumb, Size::Screen] {
        let _ = std::fs::remove_file(cache_path(data_dir, key, size));
    }
}

fn semaphore() -> &'static tokio::sync::Semaphore {
    static S: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
    S.get_or_init(|| tokio::sync::Semaphore::new(PARALLEL))
}

/// The made picture of `source` (a file of type `mime`) in this size: from the cache when it is not older than the file, else made now.
/// `None` when the server cannot read this kind of file (a HEIC without libheif, a video without ffmpeg, a damaged file).
pub async fn rendered(data_dir: &Path, key: &str, source: &Path, mime: &str, size: Size) -> Option<PathBuf> {
    let cached = cache_path(data_dir, key, size);
    if fresh(&cached, source) {
        return Some(cached);
    }
    let _permit = semaphore().acquire().await.ok()?;
    // Another request may have made it while this one waited.
    if fresh(&cached, source) {
        return Some(cached);
    }
    let (source, mime, target) = (source.to_path_buf(), mime.to_owned(), cached.clone());
    let made = tokio::task::spawn_blocking(move || {
        let jpeg = render(&source, &mime, size)?;
        store(&target, &jpeg).ok()
    })
    .await
    .ok()
    .flatten();
    made.map(|()| cached)
}

/// The cached file exists and is not older than its source.
fn fresh(cached: &Path, source: &Path) -> bool {
    let (Ok(c), Ok(s)) = (std::fs::metadata(cached), std::fs::metadata(source)) else { return false };
    match (c.modified(), s.modified()) {
        (Ok(c), Ok(s)) => c >= s,
        _ => false,
    }
}

/// Writes through a temporary file, so a half-written picture is never served.
fn store(target: &Path, jpeg: &[u8]) -> std::io::Result<()> {
    let dir = target.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let temp = dir.join(format!(".{}.part", uuid::Uuid::new_v4().simple()));
    std::fs::write(&temp, jpeg)?;
    std::fs::rename(&temp, target).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

/// Makes the JPEG of a file in this size; `None` when it cannot be read.
pub fn render(source: &Path, mime: &str, size: Size) -> Option<Vec<u8>> {
    if mime.starts_with("video/") {
        return video_frame(source, size);
    }
    let image = decode(source, mime)?;
    Some(encode(&fit(image, size.px()), size.quality()))
}

/// The picture, upright.
pub fn decode(source: &Path, mime: &str) -> Option<DynamicImage> {
    if is_heif(mime) {
        return decode_heif(source);
    }
    let mut decoder = ImageReader::open(source).ok()?.with_guessed_format().ok()?.into_decoder().ok()?;
    let orientation = decoder.orientation().ok();
    let mut image = DynamicImage::from_decoder(decoder).ok()?;
    if let Some(o) = orientation {
        image.apply_orientation(o);
    }
    Some(image)
}

pub fn is_heif(mime: &str) -> bool {
    matches!(mime, "image/heic" | "image/heif" | "image/heic-sequence" | "image/heif-sequence" | "image/avif")
}

/// Whether this server can render HEIC (libheif is there).
pub const HEIF: bool = cfg!(target_os = "linux");

#[cfg(target_os = "linux")]
fn decode_heif(source: &Path) -> Option<DynamicImage> {
    use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};
    let lib = LibHeif::new();
    let ctx = HeifContext::read_from_file(source.to_str()?).ok()?;
    let handle = ctx.primary_image_handle().ok()?;
    // libheif turns the picture upright itself (its transformations are not ignored).
    let decoded = lib.decode(&handle, ColorSpace::Rgb(RgbChroma::Rgb), None).ok()?;
    let planes = decoded.planes();
    let plane = planes.interleaved?;
    let (w, h, stride) = (plane.width, plane.height, plane.stride);
    let row = usize::try_from(w).ok()? * 3;
    let mut rgb = Vec::with_capacity(row * usize::try_from(h).ok()?);
    for y in 0..usize::try_from(h).ok()? {
        rgb.extend_from_slice(plane.data.get(y * stride..y * stride + row)?);
    }
    image::RgbImage::from_raw(w, h, rgb).map(DynamicImage::ImageRgb8)
}

#[cfg(not(target_os = "linux"))]
fn decode_heif(_source: &Path) -> Option<DynamicImage> {
    None
}

/// Smaller so that the longer side is at most `px`; a smaller picture stays as it is.
pub fn fit(image: DynamicImage, px: u32) -> DynamicImage {
    if image.width() <= px && image.height() <= px {
        return image;
    }
    image.resize(px, px, image::imageops::FilterType::Triangle)
}

pub fn encode(image: &DynamicImage, quality: u8) -> Vec<u8> {
    let mut out = Vec::new();
    let rgb = image.to_rgb8();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    // A picture that was decoded can be encoded; an error here leaves an empty answer, which the caller treats as none.
    if enc.encode_image(&rgb).is_err() {
        out.clear();
    }
    out
}

/// Whether `ffmpeg` can be started.
pub fn has_ffmpeg() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| Command::new("ffmpeg").arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success()))
}

/// One frame of a video, one second in (or the first, for a shorter one), upright, as a JPEG.
fn video_frame(source: &Path, size: Size) -> Option<Vec<u8>> {
    if !has_ffmpeg() {
        return None;
    }
    let scale = format!("scale='if(gt(iw,ih),min({0},iw),-2)':'if(gt(iw,ih),-2,min({0},ih))'", size.px());
    for at in ["1", "0"] {
        let out = Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-ss", at, "-i"])
            .arg(source)
            .args(["-frames:v", "1", "-vf", &scale, "-f", "image2", "-c:v", "mjpeg", "-q:v", "4", "pipe:1"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if out.status.success() && out.stdout.len() > 3 {
            return Some(out.stdout);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(dir: &Path, w: u32, h: u32) -> PathBuf {
        let p = dir.join(format!("p{w}x{h}.png"));
        image::RgbImage::from_pixel(w, h, image::Rgb([200, 30, 30])).save(&p).unwrap();
        p
    }

    #[test]
    fn a_large_picture_gets_smaller_and_a_small_one_stays() {
        let dir = tempfile::tempdir().unwrap();
        let big = render(&png(dir.path(), 3000, 1000), "image/png", Size::Screen).unwrap();
        let back = image::load_from_memory(&big).unwrap();
        assert_eq!((back.width(), back.height()), (2048, 683));
        let small = render(&png(dir.path(), 300, 200), "image/png", Size::Thumb).unwrap();
        let back = image::load_from_memory(&small).unwrap();
        assert_eq!((back.width(), back.height()), (300, 200));
        assert!(render(&dir.path().join("none.png"), "image/png", Size::Thumb).is_none());
    }

    /// A HEIC as the iPhones write it, made with the encoder of libheif (x265), is read and made smaller.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_heic_is_read_through_libheif() {
        use libheif_rs::{Channel, ColorSpace, CompressionFormat, EncoderQuality, HeifContext, Image, LibHeif, RgbChroma};
        let (w, h) = (1600u32, 1200u32);
        let mut img = Image::new(w, h, ColorSpace::Rgb(RgbChroma::C444)).unwrap();
        for c in [Channel::R, Channel::G, Channel::B] {
            img.create_plane(c, w, h, 8).unwrap();
        }
        let planes = img.planes_mut();
        for p in [planes.r, planes.g, planes.b].into_iter().flatten() {
            p.data.fill(128);
        }
        let lib = LibHeif::new();
        let Ok(mut encoder) = lib.encoder_for_format(CompressionFormat::Hevc) else {
            eprintln!("no HEVC encoder for libheif here: skipped");
            return;
        };
        encoder.set_quality(EncoderQuality::Lossy(50)).unwrap();
        let mut ctx = HeifContext::new().unwrap();
        ctx.encode_image(&img, &mut encoder, None).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("IMG_0001.HEIC");
        ctx.write_to_file(path.to_str().unwrap()).unwrap();
        let jpeg = render(&path, "image/heic", Size::Screen).expect("libheif reads it");
        let back = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((back.width(), back.height()), (1600, 1200));
        let jpeg = render(&path, "image/heic", Size::Thumb).unwrap();
        assert_eq!(image::load_from_memory(&jpeg).unwrap().width(), 480);
    }

    #[test]
    fn a_video_gives_a_frame_when_ffmpeg_is_there() {
        if !has_ffmpeg() {
            eprintln!("no ffmpeg here: skipped");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.mp4");
        let ok = Command::new("ffmpeg")
            .args(["-v", "error", "-f", "lavfi", "-i", "testsrc=size=1280x720:rate=10", "-t", "3", "-y"])
            .arg(&path)
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok);
        let jpeg = render(&path, "video/mp4", Size::Thumb).expect("a frame");
        let back = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((back.width(), back.height()), (480, 270));
    }

    #[tokio::test]
    async fn the_cache_is_used_until_the_file_changes() {
        let dir = tempfile::tempdir().unwrap();
        let source = png(dir.path(), 900, 600);
        let made = rendered(dir.path(), "l1", &source, "image/png", Size::Thumb).await.unwrap();
        assert_eq!(made, cache_path(dir.path(), "l1", Size::Thumb));
        let first = std::fs::metadata(&made).unwrap().modified().unwrap();
        let again = rendered(dir.path(), "l1", &source, "image/png", Size::Thumb).await.unwrap();
        assert_eq!(std::fs::metadata(&again).unwrap().modified().unwrap(), first, "taken from the cache");
        forget(dir.path(), "l1");
        assert!(!made.exists());
    }
}

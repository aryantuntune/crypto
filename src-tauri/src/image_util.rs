use crate::error::Result;
use image::ImageReader;
use std::path::Path;

pub const MAX_LONG_EDGE: u32 = 1568;
pub const MAX_BYTES: u64 = 5 * 1024 * 1024;

pub fn needs_resize(path: &Path) -> Result<bool> {
    let meta = std::fs::metadata(path)?;
    if meta.len() > MAX_BYTES { return Ok(true); }
    let img = ImageReader::open(path)?.with_guessed_format()?.decode()
        .map_err(|e| crate::error::AppError::Internal(format!("decode: {}", e)))?;
    let (w, h) = (img.width(), img.height());
    Ok(w.max(h) > MAX_LONG_EDGE)
}

pub fn resize_in_place(path: &Path) -> Result<()> {
    let img = ImageReader::open(path)?.with_guessed_format()?.decode()
        .map_err(|e| crate::error::AppError::Internal(format!("decode: {}", e)))?;
    let (w, h) = (img.width(), img.height());
    let scale = MAX_LONG_EDGE as f32 / w.max(h) as f32;
    let nw = (w as f32 * scale) as u32;
    let nh = (h as f32 * scale) as u32;
    let resized = img.resize_exact(nw, nh, image::imageops::FilterType::Lanczos3);
    resized.save(path).map_err(|e| crate::error::AppError::Internal(format!("save: {}", e)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    #[test]
    fn small_image_no_resize_needed() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("small.png");
        let buf: ImageBuffer<Rgb<u8>, _> = ImageBuffer::from_fn(100, 100, |_, _| Rgb([0u8, 0, 0]));
        buf.save(&p).unwrap();
        assert!(!needs_resize(&p).unwrap());
    }

    #[test]
    fn big_image_gets_resized() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("big.png");
        let buf: ImageBuffer<Rgb<u8>, _> = ImageBuffer::from_fn(2400, 1200, |_, _| Rgb([255u8, 255, 255]));
        buf.save(&p).unwrap();
        assert!(needs_resize(&p).unwrap());
        resize_in_place(&p).unwrap();
        let img = ImageReader::open(&p).unwrap().decode().unwrap();
        assert!(img.width().max(img.height()) <= MAX_LONG_EDGE);
    }
}

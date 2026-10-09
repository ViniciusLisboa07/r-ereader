//! Capas: guardamos uma versão grande (painel de detalhes) e uma miniatura (grade).

use std::path::Path;

use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat};

use crate::error::{Error, Result};

pub const COVER_FILE: &str = "cover.jpg";
pub const THUMBNAIL_FILE: &str = "thumb.jpg";

const COVER_MAX_HEIGHT: u32 = 1200;
/// Cartões da grade têm ~220px de altura; o dobro cobre telas HiDPI.
const THUMBNAIL_MAX_HEIGHT: u32 = 450;

/// Decodifica a capa original e grava `cover.jpg` e `thumb.jpg` em `folder`.
/// Devolve `false` se a imagem não puder ser decodificada (ex.: capa em SVG).
pub fn write_covers(bytes: &[u8], folder: &Path) -> Result<bool> {
    let Ok(image) = image::load_from_memory(bytes) else {
        return Ok(false);
    };
    save_jpeg(&shrink(&image, COVER_MAX_HEIGHT), &folder.join(COVER_FILE))?;
    save_jpeg(
        &shrink(&image, THUMBNAIL_MAX_HEIGHT),
        &folder.join(THUMBNAIL_FILE),
    )?;
    Ok(true)
}

fn shrink(image: &DynamicImage, max_height: u32) -> DynamicImage {
    if image.height() <= max_height {
        return image.clone();
    }
    let width = (image.width() as u64 * max_height as u64 / image.height() as u64).max(1) as u32;
    image.resize_exact(width, max_height, FilterType::Lanczos3)
}

fn save_jpeg(image: &DynamicImage, path: &Path) -> Result<()> {
    // JPEG não tem canal alfa: convertemos para RGB antes de gravar.
    DynamicImage::ImageRgb8(image.to_rgb8())
        .save_with_format(path, ImageFormat::Jpeg)
        .map_err(|e| Error::Io(std::io::Error::other(e)))
}

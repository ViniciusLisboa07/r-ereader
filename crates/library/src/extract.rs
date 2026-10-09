//! Extração de metadados e capa dos arquivos na hora da importação.

use std::path::Path;

use r_ereader_epub::Epub;

use crate::error::{Error, Result};
use crate::model::{Format, SeriesRef};
use crate::text;

#[derive(Debug, Default)]
pub struct Extracted {
    pub title: String,
    pub authors: Vec<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub language: Option<String>,
    pub published: Option<String>,
    pub isbn: Option<String>,
    pub tags: Vec<String>,
    pub series: Option<SeriesRef>,
    pub identifiers: Vec<(String, String)>,
    pub cover: Option<Vec<u8>>,
}

pub fn extract(path: &Path, format: Format) -> Result<Extracted> {
    let mut extracted = match format {
        Format::Epub => from_epub(path)?,
        Format::Pdf => from_pdf(path)?,
    };
    if extracted.title.trim().is_empty() {
        extracted.title = path
            .file_stem()
            .map(|s| text::title_from_file_stem(&s.to_string_lossy()))
            .unwrap_or_else(|| "Sem título".to_owned());
    }
    Ok(extracted)
}

fn from_epub(path: &Path) -> Result<Extracted> {
    let mut book = Epub::open(path)?;
    let meta = book.metadata().clone();
    let cover = book.cover().ok().flatten().map(|(_, bytes)| bytes);

    Ok(Extracted {
        title: meta.title.clone().unwrap_or_default(),
        authors: meta.creators.iter().flat_map(|c| split_authors(c)).collect(),
        description: meta
            .description
            .as_deref()
            .map(text::html_to_text)
            .filter(|d| !d.is_empty()),
        publisher: meta.publisher.clone(),
        language: meta.language.clone(),
        published: meta.date.as_deref().map(normalize_date),
        isbn: meta.isbn(),
        tags: meta.subjects.iter().flat_map(|s| text::split_list(s)).collect(),
        series: meta.series.as_ref().map(|s| SeriesRef {
            name: s.name.clone(),
            index: s.index,
        }),
        identifiers: meta
            .identifiers
            .iter()
            .filter_map(|id| Some((id.scheme.clone()?.to_lowercase(), id.value.clone())))
            .collect(),
        cover,
    })
}

fn from_pdf(path: &Path) -> Result<Extracted> {
    let meta = lopdf::Document::load_metadata(path).map_err(|e| Error::Pdf(e.to_string()))?;
    let clean = |value: Option<String>| {
        value
            .map(|v| repair_utf8(&v).trim().to_owned())
            .filter(|v| !v.is_empty())
    };

    Ok(Extracted {
        title: clean(meta.title)
            .filter(|t| !is_junk_pdf_title(t))
            .unwrap_or_default(),
        authors: clean(meta.author).map(|a| split_authors(&a)).unwrap_or_default(),
        description: clean(meta.subject),
        published: clean(meta.creation_date).and_then(|d| pdf_date(&d)),
        tags: clean(meta.keywords)
            .map(|k| text::split_list(&k))
            .unwrap_or_default(),
        ..Extracted::default()
    })
}

/// Muitos geradores gravam UTF-8 cru no dicionário Info, que o leitor decodifica
/// como PDFDocEncoding ("oração" vira "oraÃ§Ã£o"). Se todos os caracteres cabem em
/// um byte e esses bytes formam UTF-8 válido com acentos, usamos a versão UTF-8.
fn repair_utf8(value: &str) -> String {
    if value.is_ascii() || value.chars().any(|c| c as u32 > 0xFF) {
        return value.to_owned();
    }
    let bytes: Vec<u8> = value.chars().map(|c| c as u32 as u8).collect();
    match String::from_utf8(bytes) {
        Ok(repaired) => repaired,
        Err(_) => value.to_owned(),
    }
}

/// Títulos que o programa gerador põe no PDF e não dizem nada sobre o livro.
fn is_junk_pdf_title(title: &str) -> bool {
    let lower = title.to_lowercase();
    const PREFIXES: &[&str] = &[
        "microsoft word - ",
        "microsoft powerpoint - ",
        "untitled",
        "sem título",
    ];
    const EXTENSIONS: &[&str] = &[
        ".doc", ".docx", ".odt", ".rtf", ".indd", ".pdf", ".tex", ".dvi", ".qxd",
    ];
    PREFIXES.iter().any(|p| lower.starts_with(p)) || EXTENSIONS.iter().any(|e| lower.ends_with(e))
}

/// "Fulano & Ciclano" e "Fulano; Ciclano" viram dois autores. Vírgula não separa,
/// porque "Lewis, C. S." é um nome só.
fn split_authors(raw: &str) -> Vec<String> {
    raw.split(['&', ';'])
        .map(text::normalize_author)
        .filter(|a| !a.is_empty())
        .collect()
}

/// Mantém só a data (`2019-11-13T08:00:00+00:00` → `2019-11-13`).
fn normalize_date(date: &str) -> String {
    date.split('T').next().unwrap_or(date).trim().to_owned()
}

/// `D:20190131120000Z` → `2019-01-31`.
fn pdf_date(raw: &str) -> Option<String> {
    let digits: String = raw.trim_start_matches("D:").chars().take(8).collect();
    if digits.len() < 4 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(match digits.len() {
        8 => format!("{}-{}-{}", &digits[..4], &digits[4..6], &digits[6..8]),
        6 | 7 => format!("{}-{}", &digits[..4], &digits[4..6]),
        _ => digits[..4].to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_multiple_authors() {
        assert_eq!(split_authors("Jez Humble & Gene Kim"), ["Jez Humble", "Gene Kim"]);
        assert_eq!(split_authors("Lewis, C. S."), ["Lewis, C. S."]);
        assert_eq!(split_authors("C.S. Lewis"), ["C. S. Lewis"]);
    }

    #[test]
    fn repairs_utf8_read_as_latin1() {
        assert_eq!(repair_utf8("oraÃ§Ã£o"), "oração");
        // Latin-1 legítimo (bytes que não formam UTF-8) fica como está.
        assert_eq!(repair_utf8("ação"), "ação");
        assert_eq!(repair_utf8("ascii"), "ascii");
    }

    #[test]
    fn detects_junk_pdf_titles() {
        assert!(is_junk_pdf_title("Microsoft Word - IGLH.doc"));
        assert!(is_junk_pdf_title("capitulo3.indd"));
        assert!(!is_junk_pdf_title("The garden of peace"));
    }

    #[test]
    fn normalizes_dates() {
        assert_eq!(normalize_date("2019-11-13T08:00:00+00:00"), "2019-11-13");
        assert_eq!(pdf_date("D:20190131120000Z").as_deref(), Some("2019-01-31"));
        assert_eq!(pdf_date("lixo"), None);
    }
}

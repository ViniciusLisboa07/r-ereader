use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use zip::ZipArchive;

use crate::dom::{self, Element};
use crate::error::{Error, Result};
use crate::package::{ManifestItem, Metadata, Package};
use crate::path;
use crate::toc::{self, TocEntry};

const CONTAINER_PATH: &str = "META-INF/container.xml";
const OPF_MEDIA_TYPE: &str = "application/oebps-package+xml";

/// Um documento da ordem de leitura (spine), já resolvido contra o manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct Chapter {
    pub id: String,
    pub path: String,
    pub media_type: String,
    pub linear: bool,
}

pub struct Epub<R> {
    archive: ZipArchive<R>,
    opf_path: String,
    package: Package,
    chapters: Vec<Chapter>,
    toc: Vec<TocEntry>,
}

impl Epub<BufReader<File>> {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_reader(BufReader::new(File::open(path)?))
    }
}

impl<R: Read + Seek> Epub<R> {
    pub fn from_reader(reader: R) -> Result<Self> {
        let mut archive = ZipArchive::new(reader)?;

        let container = parse_xml(&mut archive, CONTAINER_PATH)?;
        let opf_path = rootfile_path(&container)?;
        let opf_source = read_text(&mut archive, &opf_path)?;
        let package = Package::parse(&opf_source, &opf_path)?;

        let chapters = package
            .spine
            .iter()
            .filter_map(|spine_item| {
                let item = package.item(&spine_item.idref)?;
                Some(Chapter {
                    id: item.id.clone(),
                    path: item.path.clone(),
                    media_type: item.media_type.clone(),
                    linear: spine_item.linear,
                })
            })
            .collect();

        // Sumário quebrado não deve impedir a leitura do livro.
        let toc = load_toc(&mut archive, &package);

        Ok(Epub {
            archive,
            opf_path,
            package,
            chapters,
            toc,
        })
    }

    pub fn metadata(&self) -> &Metadata {
        &self.package.metadata
    }

    pub fn package(&self) -> &Package {
        &self.package
    }

    pub fn opf_path(&self) -> &str {
        &self.opf_path
    }

    /// Documentos na ordem de leitura.
    pub fn chapters(&self) -> &[Chapter] {
        &self.chapters
    }

    pub fn toc(&self) -> &[TocEntry] {
        &self.toc
    }

    /// Índice do capítulo que contém o `href` (fragmento ignorado).
    pub fn chapter_index(&self, href: &str) -> Option<usize> {
        let (target, _) = path::split_fragment(href);
        self.chapters.iter().position(|c| c.path == target)
    }

    pub fn read_bytes(&mut self, path: &str) -> Result<Vec<u8>> {
        read_bytes(&mut self.archive, path)
    }

    pub fn read_text(&mut self, path: &str) -> Result<String> {
        read_text(&mut self.archive, path)
    }

    /// Faz o parse do XHTML de um capítulo.
    pub fn chapter_document(&mut self, index: usize) -> Result<Element> {
        let chapter = self
            .chapters
            .get(index)
            .ok_or_else(|| Error::Invalid(format!("capítulo {index} não existe")))?;
        let path = chapter.path.clone();
        parse_xml(&mut self.archive, &path)
    }

    pub fn cover(&mut self) -> Result<Option<(ManifestItem, Vec<u8>)>> {
        let Some(item) = self.package.cover_item().cloned() else {
            return Ok(None);
        };
        let bytes = self.read_bytes(&item.path)?;
        Ok(Some((item, bytes)))
    }
}

fn rootfile_path(container: &Element) -> Result<String> {
    let rootfiles: Vec<&Element> = container.descendants().filter(|e| e.name == "rootfile").collect();
    rootfiles
        .iter()
        .find(|r| r.attr("media-type") == Some(OPF_MEDIA_TYPE))
        .or(rootfiles.first())
        .and_then(|r| r.attr("full-path"))
        .map(|p| path::resolve("", p))
        .ok_or_else(|| Error::Invalid("container.xml sem <rootfile full-path>".into()))
}

fn load_toc<R: Read + Seek>(archive: &mut ZipArchive<R>, package: &Package) -> Vec<TocEntry> {
    if let Some(nav) = package.nav_item()
        && let Ok(root) = parse_xml(archive, &nav.path)
    {
        let entries = toc::from_nav(&root, &nav.path);
        if !entries.is_empty() {
            return entries;
        }
    }
    if let Some(ncx) = package.ncx_item()
        && let Ok(root) = parse_xml(archive, &ncx.path)
    {
        return toc::from_ncx(&root, &ncx.path);
    }
    Vec::new()
}

fn read_bytes<R: Read + Seek>(archive: &mut ZipArchive<R>, path: &str) -> Result<Vec<u8>> {
    let (path, _) = path::split_fragment(path);
    let name = match archive.index_for_name(path) {
        Some(index) => index,
        // Alguns geradores erram a caixa dos nomes; tentamos sem diferenciar.
        None => (0..archive.len())
            .find(|&i| {
                archive
                    .name_for_index(i)
                    .is_some_and(|name| name.eq_ignore_ascii_case(path))
            })
            .ok_or_else(|| Error::MissingFile(path.to_owned()))?,
    };
    let mut file = archive.by_index(name)?;
    let mut bytes = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn read_text<R: Read + Seek>(archive: &mut ZipArchive<R>, path: &str) -> Result<String> {
    let bytes = read_bytes(archive, path)?;
    Ok(match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    })
}

fn parse_xml<R: Read + Seek>(archive: &mut ZipArchive<R>, path: &str) -> Result<Element> {
    let source = read_text(archive, path)?;
    dom::parse(&source).map_err(|source| Error::Xml {
        path: path.to_owned(),
        source,
    })
}

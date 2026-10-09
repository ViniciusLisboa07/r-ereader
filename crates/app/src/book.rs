//! Estado de um livro aberto: metadados prontos para exibição, sumário
//! achatado e o capítulo atual.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{Image, ImageFormat, SharedString};
use r_ereader_epub::{Epub, TocEntry};

use crate::chapter_text::ChapterText;
use crate::preview::{self, Block};

type Book = Epub<BufReader<File>>;

pub struct TocRow {
    pub depth: usize,
    pub title: SharedString,
    pub chapter: Option<usize>,
}

pub enum ChapterBlock {
    Text(Block),
    Image(Arc<Image>),
}

impl ChapterBlock {
    /// Texto selecionável do bloco (imagens não têm).
    pub fn text(&self) -> &str {
        match self {
            ChapterBlock::Text(block) => block.text(),
            ChapterBlock::Image(_) => "",
        }
    }
}

pub struct OpenBook {
    epub: Book,
    pub title: SharedString,
    pub authors: SharedString,
    pub version: SharedString,
    pub language: Option<SharedString>,
    pub cover: Option<Arc<Image>>,
    pub toc: Vec<TocRow>,
    /// O livro não tem sumário: `toc` lista os arquivos da spine.
    toc_is_fallback: bool,
    pub current: usize,
    pub blocks: Vec<ChapterBlock>,
    /// Texto corrido do capítulo atual, base das posições de destaques.
    pub text: ChapterText,
    pub chapter_error: Option<SharedString>,
    /// Arquivo do livro (o índice de busca o reabre em segundo plano).
    pub path: PathBuf,
}

impl OpenBook {
    pub fn open(path: &Path) -> r_ereader_epub::Result<Self> {
        let mut epub = Epub::open(path)?;
        let metadata = epub.metadata().clone();

        let title = metadata.title.unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        let cover = epub
            .cover()
            .ok()
            .flatten()
            .and_then(|(item, bytes)| image(&item.media_type, bytes));
        let toc_is_fallback = epub.toc().is_empty();
        let toc = toc_rows(&epub);
        let start = epub.chapters().iter().position(|c| c.linear).unwrap_or(0);

        let mut book = OpenBook {
            title: title.into(),
            authors: metadata.creators.join(", ").into(),
            version: format!("EPUB {}", epub.package().version).into(),
            language: metadata.language.map(Into::into),
            cover,
            toc,
            toc_is_fallback,
            current: start,
            blocks: Vec::new(),
            text: ChapterText::default(),
            chapter_error: None,
            path: path.to_owned(),
            epub,
        };
        book.go_to(start);
        Ok(book)
    }

    pub fn chapter_count(&self) -> usize {
        self.epub.chapters().len()
    }

    pub fn chapter_path(&self) -> &str {
        self.epub
            .chapters()
            .get(self.current)
            .map_or("", |c| c.path.as_str())
    }

    pub fn go_to(&mut self, index: usize) -> bool {
        if index >= self.chapter_count() {
            return false;
        }
        self.current = index;
        self.chapter_error = None;
        self.blocks = match self.epub.chapter_document(index) {
            Ok(document) => {
                let chapter_path = self.chapter_path().to_owned();
                preview::extract(&document, &chapter_path)
                    .into_iter()
                    .map(|block| self.load_block(block))
                    .collect()
            }
            Err(e) => {
                self.chapter_error = Some(e.to_string().into());
                Vec::new()
            }
        };
        self.text = ChapterText::new(self.blocks.iter().map(ChapterBlock::text));
        true
    }

    /// Linha do sumário que corresponde à posição atual: a última entrada
    /// cujo capítulo não passa do atual.
    pub fn active_toc_row(&self) -> Option<usize> {
        self.toc
            .iter()
            .enumerate()
            .filter(|(_, row)| row.chapter.is_some_and(|c| c <= self.current))
            .max_by_key(|(i, row)| (row.chapter, std::cmp::Reverse(*i)))
            .map(|(i, _)| i)
    }

    /// Título do sumário para um capítulo (a última entrada que começa nele ou antes).
    /// Título do capítulo atual para anotações: a entrada do sumário que o contém;
    /// sem sumário de verdade, o primeiro título (h1–h3) do próprio capítulo.
    pub fn chapter_title(&self) -> Option<SharedString> {
        self.toc_title(self.current).or_else(|| {
            self.blocks.iter().find_map(|block| match block {
                ChapterBlock::Text(Block::Heading { level, text }) if *level <= 3 => {
                    Some(text.clone().into())
                }
                _ => None,
            })
        })
    }

    pub fn toc_title(&self, chapter: usize) -> Option<SharedString> {
        if self.toc_is_fallback {
            return None;
        }
        self.toc
            .iter()
            .filter(|row| row.chapter.is_some_and(|c| c <= chapter))
            .max_by_key(|row| row.chapter)
            .map(|row| row.title.clone())
    }

    fn load_block(&mut self, block: Block) -> ChapterBlock {
        match block {
            Block::Image(path) => {
                let media_type = self
                    .epub
                    .package()
                    .item_by_path(&path)
                    .map(|item| item.media_type.clone())
                    .unwrap_or_default();
                // Uma imagem ilegível continua ocupando seu bloco (vazio), para que as
                // posições no texto batam com as do índice de busca.
                let loaded = self
                    .epub
                    .read_bytes(&path)
                    .ok()
                    .and_then(|bytes| image(&media_type, bytes));
                loaded.map_or(ChapterBlock::Text(Block::Image(path)), ChapterBlock::Image)
            }
            text => ChapterBlock::Text(text),
        }
    }
}

fn image(media_type: &str, bytes: Vec<u8>) -> Option<Arc<Image>> {
    let format = ImageFormat::from_mime_type(media_type)?;
    Some(Arc::new(Image::from_bytes(format, bytes)))
}

fn toc_rows(epub: &Book) -> Vec<TocRow> {
    let rows: Vec<TocRow> = TocEntry::flatten(epub.toc())
        .into_iter()
        .map(|(depth, entry)| TocRow {
            depth,
            title: entry.title.clone().into(),
            chapter: entry.href.as_deref().and_then(|href| epub.chapter_index(href)),
        })
        .collect();
    if !rows.is_empty() {
        return rows;
    }

    // Sem sumário: listamos a spine pelo nome dos arquivos.
    epub.chapters()
        .iter()
        .enumerate()
        .filter(|(_, chapter)| chapter.linear)
        .map(|(index, chapter)| TocRow {
            depth: 0,
            title: chapter
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&chapter.path)
                .to_owned()
                .into(),
            chapter: Some(index),
        })
        .collect()
}

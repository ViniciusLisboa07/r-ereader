use std::path::{Path, PathBuf};

pub type BookId = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Format {
    Epub,
    Pdf,
}

impl Format {
    pub const ALL: [Format; 2] = [Format::Epub, Format::Pdf];

    pub fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "epub" => Some(Format::Epub),
            "pdf" => Some(Format::Pdf),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Format::Epub => "EPUB",
            Format::Pdf => "PDF",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Epub => "epub",
            Format::Pdf => "pdf",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        Format::ALL.into_iter().find(|f| f.as_str() == value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeriesRef {
    pub name: String,
    pub index: Option<f64>,
}

/// O que a grade/lista da biblioteca precisa de cada livro.
#[derive(Debug, Clone, PartialEq)]
pub struct BookSummary {
    pub id: BookId,
    pub title: String,
    pub authors: Vec<String>,
    pub series: Option<SeriesRef>,
    pub formats: Vec<Format>,
    /// Miniatura da capa, se o livro tiver uma.
    pub thumbnail: Option<PathBuf>,
    /// Fração lida (0.0–1.0), se o livro já foi aberto.
    pub progress: Option<f32>,
    pub added_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BookFile {
    pub format: Format,
    pub path: PathBuf,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CollectionRef {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BookDetails {
    pub summary: BookSummary,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub language: Option<String>,
    pub published: Option<String>,
    pub isbn: Option<String>,
    pub tags: Vec<String>,
    pub collections: Vec<CollectionRef>,
    pub cover: Option<PathBuf>,
    pub files: Vec<BookFile>,
    pub folder: PathBuf,
}

/// Item do navegador lateral (autor, série, tag, coleção) com a contagem de livros.
#[derive(Debug, Clone, PartialEq)]
pub struct Facet {
    pub id: i64,
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Filter {
    #[default]
    All,
    /// Livros com progresso salvo e ainda não terminados.
    Reading,
    Author(i64),
    Series(i64),
    Tag(i64),
    Collection(i64),
    Format(Format),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    RecentlyAdded,
    Title,
    Author,
    RecentlyRead,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct BookQuery {
    pub text: String,
    pub filter: Filter,
    pub sort: SortOrder,
}

/// Metadados editáveis. Campos `None`/vazios apagam o valor.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MetadataEdit {
    pub title: String,
    pub authors: Vec<String>,
    pub series: Option<String>,
    pub series_index: Option<f64>,
    pub tags: Vec<String>,
    pub publisher: Option<String>,
    pub language: Option<String>,
    pub published: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HighlightColor {
    Yellow,
    Green,
    Blue,
    Pink,
}

impl HighlightColor {
    pub const ALL: [HighlightColor; 4] = [
        HighlightColor::Yellow,
        HighlightColor::Green,
        HighlightColor::Blue,
        HighlightColor::Pink,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            HighlightColor::Yellow => "yellow",
            HighlightColor::Green => "green",
            HighlightColor::Blue => "blue",
            HighlightColor::Pink => "pink",
        }
    }

    pub(crate) fn parse(value: &str) -> Self {
        HighlightColor::ALL
            .into_iter()
            .find(|c| c.as_str() == value)
            .unwrap_or(HighlightColor::Yellow)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Highlight {
    pub id: i64,
    pub book_id: BookId,
    /// Índice do capítulo na spine.
    pub chapter: usize,
    pub chapter_title: Option<String>,
    /// Intervalo de bytes no texto do capítulo.
    pub start: usize,
    pub end: usize,
    pub quote: String,
    pub color: HighlightColor,
    pub note: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewHighlight {
    pub chapter: usize,
    pub chapter_title: Option<String>,
    pub start: usize,
    pub end: usize,
    pub quote: String,
    pub color: HighlightColor,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    pub chapter: usize,
    pub fraction: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportOutcome {
    Added(BookId),
    /// O mesmo arquivo (mesmo hash) já está na biblioteca.
    Duplicate(BookId),
}

impl ImportOutcome {
    pub fn id(self) -> BookId {
        match self {
            ImportOutcome::Added(id) | ImportOutcome::Duplicate(id) => id,
        }
    }
}

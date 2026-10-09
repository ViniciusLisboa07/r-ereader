//! Leitura de arquivos EPUB 2 e 3: container, package (OPF), spine e sumário.

mod book;
pub mod dom;
mod error;
mod package;
pub mod path;
mod toc;

pub use book::{Chapter, Epub};
pub use error::{Error, Result};
pub use package::{Identifier, ManifestItem, Metadata, Package, Series, SpineItem};
pub use toc::TocEntry;

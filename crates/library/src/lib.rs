//! Biblioteca gerenciada no estilo Calibre: banco SQLite com metadados, busca
//! textual, coleções e progresso de leitura, mais uma pasta com os arquivos
//! organizados por autor e título.

pub mod anchor;
mod cover;
mod error;
mod extract;
mod library;
mod model;
pub mod obsidian;
mod schema;
pub mod text;

#[cfg(feature = "test-support")]
pub mod testing;

pub use error::{Error, Result};
pub use library::Library;
pub use model::*;

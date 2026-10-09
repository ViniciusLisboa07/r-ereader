use std::fmt;
use std::path::PathBuf;

use crate::model::BookId;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Database(rusqlite::Error),
    Epub(r_ereader_epub::Error),
    Pdf(String),
    /// Extensão que a biblioteca não sabe catalogar.
    Unsupported(PathBuf),
    NotFound(BookId),
    /// Nome inválido ou já em uso (coleções, por exemplo).
    InvalidName(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "erro de I/O: {e}"),
            Error::Database(e) => write!(f, "erro no banco da biblioteca: {e}"),
            Error::Epub(e) => write!(f, "{e}"),
            Error::Pdf(e) => write!(f, "PDF inválido: {e}"),
            Error::Unsupported(path) => write!(f, "formato não suportado: {}", path.display()),
            Error::NotFound(id) => write!(f, "livro {id} não existe na biblioteca"),
            Error::InvalidName(name) => write!(f, "nome inválido: {name}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Database(e) => Some(e),
            Error::Epub(e) => Some(e),
            Error::Pdf(_) | Error::Unsupported(_) | Error::NotFound(_) | Error::InvalidName(_) => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Database(e)
    }
}

impl From<r_ereader_epub::Error> for Error {
    fn from(e: r_ereader_epub::Error) -> Self {
        Error::Epub(e)
    }
}

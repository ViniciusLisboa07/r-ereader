use std::fmt;

use crate::dom::XmlError;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Zip(zip::result::ZipError),
    /// Arquivo referenciado não existe no container.
    MissingFile(String),
    Xml {
        path: String,
        source: XmlError,
    },
    /// Estrutura do EPUB fora da especificação.
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "erro de I/O: {e}"),
            Error::Zip(e) => write!(f, "arquivo ZIP inválido: {e}"),
            Error::MissingFile(path) => write!(f, "arquivo ausente no EPUB: {path}"),
            Error::Xml { path, source } => write!(f, "XML inválido em {path}: {source}"),
            Error::Invalid(message) => write!(f, "EPUB inválido: {message}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Zip(e) => Some(e),
            Error::Xml { source, .. } => Some(source),
            Error::MissingFile(_) | Error::Invalid(_) => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<zip::result::ZipError> for Error {
    fn from(e: zip::result::ZipError) -> Self {
        Error::Zip(e)
    }
}

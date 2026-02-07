use std::io;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("I/O error")]
    Io,
    #[error("Preprocessing error")]
    Preprocessing,
    #[error("Conditional error")]
    Conditional,
    #[error("Regex error")]
    Regex,
    #[error("Variable error")]
    Variable,
    #[error("Reference error")]
    Reference,
    #[error("Include error")]
    Include,
    #[error("Stream error")]
    Stream,
    #[error("Catalog error")]
    Catalog,
    #[error("Parsing error")]
    Parsing,
}

impl From<io::Error> for crate::Error {
    fn from(_: io::Error) -> Self {
        Error::Io
    }
}

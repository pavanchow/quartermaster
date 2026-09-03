//! One error type for the whole crate. Resolution *failure* is not an error
//! here: an unsolvable dependency graph returns a structured explanation, not
//! an `Err`. `Err` is reserved for malformed input and IO.
use std::fmt;

#[derive(Debug)]
pub enum Error {
    Version(String),
    Range(String),
    Manifest(String),
    Lock(String),
    Io(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Version(m) => write!(f, "version error: {m}"),
            Error::Range(m) => write!(f, "constraint error: {m}"),
            Error::Manifest(m) => write!(f, "manifest error: {m}"),
            Error::Lock(m) => write!(f, "lockfile error: {m}"),
            Error::Io(m) => write!(f, "io error: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

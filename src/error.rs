use std::{convert::Infallible, error::Error as StdError};

use tokio::task::JoinError;

pub type Result<T = ()> = core::result::Result<T, Error>;

pub struct MagickError(pub String);

impl StdError for MagickError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        None
    }
}

impl std::fmt::Debug for MagickError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Display for MagickError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub enum Error {
    Reqwest(reqwest::Error),
    Libsql(libsql::Error),
    De(serde::de::value::Error),
    Io(std::io::Error),
    Magick(MagickError),
    Threading(tokio::task::JoinError),
    Infallible,
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Reqwest(err) => err.source(),
            Self::Libsql(err) => err.source(),
            Self::De(err) => err.source(),
            Self::Io(err) => err.source(),
            Self::Magick(err) => err.source(),
            Self::Threading(err) => err.source(),
            Self::Infallible => None,
        }
    }
}

impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Reqwest(err) => write!(f, "{err}"),
            Self::Libsql(err) => write!(f, "{err}"),
            Self::De(err) => write!(f, "{err}"),
            Self::Io(err) => write!(f, "{err}"),
            Self::Magick(err) => write!(f, "{err}"),
            Self::Threading(err) => write!(f, "{err}"),
            Self::Infallible => Ok(()),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Reqwest(err) => write!(f, "{err}"),
            Self::Libsql(err) => write!(f, "{err}"),
            Self::De(err) => write!(f, "{err}"),
            Self::Io(err) => write!(f, "{err}"),
            Self::Magick(err) => write!(f, "{err}"),
            Self::Threading(err) => write!(f, "{err}"),
            Self::Infallible => Ok(()),
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self {
        Self::Reqwest(value)
    }
}

impl From<libsql::Error> for Error {
    fn from(value: libsql::Error) -> Self {
        Self::Libsql(value)
    }
}

impl From<serde::de::value::Error> for Error {
    fn from(value: serde::de::value::Error) -> Self {
        Self::De(value)
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<MagickError> for Error {
    fn from(value: MagickError) -> Self {
        Self::Magick(value)
    }
}

impl From<JoinError> for Error {
    fn from(value: JoinError) -> Self {
        Self::Threading(value)
    }
}

impl From<Infallible> for Error {
    fn from(_value: Infallible) -> Self {
        Self::Infallible
    }
}

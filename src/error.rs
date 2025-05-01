// src/error.rs
use std::fmt;
use std::io;
use rusqlite;
use std::error::Error;

/// Custom error type for aptsync operations.
#[derive(Debug)]
pub enum AptSyncError {
    Io(io::Error),
    Sqlite(rusqlite::Error),
    Custom(String),
}

impl fmt::Display for AptSyncError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AptSyncError::Io(err) => write!(f, "IO error: {}", err),
            AptSyncError::Sqlite(err) => write!(f, "Database error: {}", err),
            AptSyncError::Custom(err) => write!(f, "{}", err),
        }
    }
}

// Implement the standard Error trait
impl Error for AptSyncError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            AptSyncError::Io(ref err) => Some(err),
            AptSyncError::Sqlite(ref err) => Some(err),
            AptSyncError::Custom(_) => None,
        }
    }
}

// Allow converting standard errors into our custom error type
impl From<io::Error> for AptSyncError {
    fn from(err: io::Error) -> AptSyncError {
        AptSyncError::Io(err)
    }
}

impl From<rusqlite::Error> for AptSyncError {
    fn from(err: rusqlite::Error) -> AptSyncError {
        AptSyncError::Sqlite(err)
    }
}

/// A convenient type alias for Results using our custom error type.
pub type Result<T> = std::result::Result<T, AptSyncError>;

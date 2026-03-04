pub mod cli;
pub mod config;
pub mod daemon;
pub mod db;
pub mod github;
pub mod query;
pub mod sync;

use thiserror::Error;
use tokio::sync::AcquireError;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("Configuration error: {0}")]
    Config(String),
    
    #[error("GitHub API error: {0}")]
    GithubApi(String),
    
    #[error("Not found: {0}")]
    NotFound(String),
    
    #[error("Daemon error: {0}")]
    Daemon(String),
    
    #[error("Sync error: {0}")]
    Sync(String),
}

impl From<AcquireError> for Error {
    fn from(e: AcquireError) -> Self {
        Error::Sync(e.to_string())
    }
}

impl serde::Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
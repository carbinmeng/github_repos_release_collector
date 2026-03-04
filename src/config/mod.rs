use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Concurrent sync count
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
    
    /// Incremental sync interval (seconds)
    #[serde(default = "default_incremental_interval")]
    pub incremental_interval: u64,
    
    /// Full sync interval (seconds)
    #[serde(default = "default_full_interval")]
    pub full_interval: u64,
    
    /// GitHub Token (recommend using environment variable GITHUB_TOKEN)
    pub github_token: Option<String>,
    
    /// Request timeout (seconds)
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    
    /// Requests per page
    #[serde(default = "default_per_page")]
    pub per_page: usize,
    
    /// Maximum retry count
    #[serde(default = "default_max_retries")]
    pub max_retries: usize,
    
    /// PID file path
    #[serde(skip)]
    pub pid_path: Option<PathBuf>,
}

fn default_concurrency() -> usize { 5 }
fn default_incremental_interval() -> u64 { 3600 } // 1 hour
fn default_full_interval() -> u64 { 86400 } // 24 hours
fn default_timeout() -> u64 { 30 }
fn default_per_page() -> usize { 100 }
fn default_max_retries() -> usize { 3 }

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path()?;
        
        if config_path.exists() {
            let content = fs::read_to_string(&config_path)?;
            let mut config: Config = toml::from_str(&content).map_err(|e| {
                Error::Config(format!("Failed to parse config: {}", e))
            })?;
            config.pid_path = Some(Self::pid_path()?);
            
            // Load token from environment if not in config
            if config.github_token.is_none() {
                config.github_token = std::env::var("GITHUB_TOKEN").ok();
            }
            
            Ok(config)
        } else {
            let config = Config {
                concurrency: default_concurrency(),
                incremental_interval: default_incremental_interval(),
                full_interval: default_full_interval(),
                github_token: std::env::var("GITHUB_TOKEN").ok(),
                timeout: default_timeout(),
                per_page: default_per_page(),
                max_retries: default_max_retries(),
                pid_path: Some(Self::pid_path()?),
            };
            
            // Create config directory if not exists
            if let Some(parent) = config_path.parent() {
                fs::create_dir_all(parent)?;
            }
            
            // Save default config
            let content = toml::to_string_pretty(&config).map_err(|e| {
                Error::Config(format!("Failed to serialize config: {}", e))
            })?;
            fs::write(&config_path, content)?;
            
            Ok(config)
        }
    }

    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        let mut config = self.clone();
        
        match key {
            "concurrency" => config.concurrency = value.parse().map_err(|_| Error::Config("Invalid concurrency value".to_string()))?,
            "incremental_interval" => config.incremental_interval = value.parse().map_err(|_| Error::Config("Invalid incremental_interval value".to_string()))?,
            "full_interval" => config.full_interval = value.parse().map_err(|_| Error::Config("Invalid full_interval value".to_string()))?,
            "github_token" => config.github_token = Some(value.to_string()),
            "timeout" => config.timeout = value.parse().map_err(|_| Error::Config("Invalid timeout value".to_string()))?,
            "per_page" => config.per_page = value.parse().map_err(|_| Error::Config("Invalid per_page value".to_string()))?,
            "max_retries" => config.max_retries = value.parse().map_err(|_| Error::Config("Invalid max_retries value".to_string()))?,
            _ => return Err(Error::Config(format!("Unknown config key: {}", key))),
        }
        
        config.save()
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path()?;
        let content = toml::to_string_pretty(self).map_err(|e| {
            Error::Config(format!("Failed to serialize config: {}", e))
        })?;
        fs::write(config_path, content)?;
        Ok(())
    }

    pub fn database_path(&self) -> PathBuf {
        Self::data_dir().join("releases.db")
    }

    fn config_path() -> Result<PathBuf> {
        Ok(Self::config_dir().join("config.toml"))
    }

    pub fn pid_path() -> Result<PathBuf> {
        Ok(Self::data_dir().join("daemon.pid"))
    }

    fn config_dir() -> PathBuf {
        if let Some(proj_dirs) = ProjectDirs::from("com", "grc", "github-release-collector") {
            proj_dirs.config_dir().to_path_buf()
        } else {
            PathBuf::from("~/.config/grc")
        }
    }

    pub fn data_dir() -> PathBuf {
        if let Some(proj_dirs) = ProjectDirs::from("com", "grc", "github-release-collector") {
            proj_dirs.data_dir().to_path_buf()
        } else {
            PathBuf::from("~/.local/share/grc")
        }
    }

    pub fn github_token(&self) -> Option<&str> {
        self.github_token.as_deref()
    }
}

impl std::fmt::Display for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, r#"Configuration:
  Concurrency: {}
  Incremental Interval: {} seconds
  Full Interval: {} seconds
  Timeout: {} seconds
  Per Page: {}
  Max Retries: {}
  Database: {}
  Config File: {}
  PID File: {}
  GitHub Token: {}
"#,
            self.concurrency,
            self.incremental_interval,
            self.full_interval,
            self.timeout,
            self.per_page,
            self.max_retries,
            self.database_path().display(),
            Self::config_path().map(|p| p.display().to_string()).unwrap_or_default(),
            Self::pid_path().map(|p| p.display().to_string()).unwrap_or_default(),
            if self.github_token.is_some() { "configured (from env/config)" } else { "NOT SET" }
        )
    }
}

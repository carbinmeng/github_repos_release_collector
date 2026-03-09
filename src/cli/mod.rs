use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "grc")]
#[command(version = "1.0.0")]
#[command(about = "GitHub Release text mirror system", long_about = None)]
pub struct Cli {
    #[arg(long, hide = true, global = true)]
    pub daemon_mode: bool,

    #[command(subcommand)]
    pub command: CliCommand,
}

#[derive(Subcommand, Debug)]
pub enum CliCommand {
    /// Initialize database
    Init,

    /// Add repository
    RepoAdd {
        /// Repository URL (owner/repo or https://github.com/owner/repo)
        repo: String,
    },

    /// Remove repository
    RepoRemove {
        /// Repository URL
        repo: String,
        
        /// Keep synchronized data
        #[arg(long, default_value = "true")]
        keep_data: bool,
    },

    /// Enable repository
    RepoEnable {
        /// Repository URL
        repo: String,
    },

    /// Disable repository
    RepoDisable {
        /// Repository URL
        repo: String,
    },

    /// List all repositories
    RepoList,

    /// Run synchronization
    SyncRun {
        /// Perform full synchronization
        #[arg(long, short)]
        full: bool,
    },

    /// Start daemon
    DaemonStart,

    /// Stop daemon
    DaemonStop,

    /// Show daemon status
    DaemonStatus,

    /// Reload configuration
    DaemonReload,

    /// Show configuration
    ConfigShow,

    /// Set configuration
    ConfigSet {
        /// Configuration key
        key: String,
        
        /// Configuration value
        value: String,
    },

    /// List releases
    QueryList {
        /// Repository name (optional)
        #[arg(long)]
        repo: Option<String>,
        
        /// Limit count
        #[arg(long, short, default_value = "50")]
        limit: usize,
        
        /// Show releases from last N days
        #[arg(long)]
        days: Option<u32>,
        
        /// Include deleted releases
        #[arg(long)]
        include_deleted: bool,
    },

    /// Show single release
    QueryShow {
        /// Query by release ID
        #[arg(long)]
        id: Option<i64>,
        
        /// Query by tag
        #[arg(long)]
        tag: Option<String>,
        
        /// Show full content
        #[arg(long, short)]
        full: bool,
    },

    /// Keyword search
    QuerySearch {
        /// Search keyword
        keyword: String,
        
        /// Limit count
        #[arg(long, short, default_value = "50")]
        limit: usize,
    },

    /// Show status
    Status,
}

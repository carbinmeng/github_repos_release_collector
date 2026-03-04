use clap::Parser;
use github_release_collector::cli::{Cli, CliCommand};
use github_release_collector::config::Config;
use github_release_collector::db::Database;
use github_release_collector::Error;
use github_release_collector::daemon;
use std::process;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

fn main() {
    // Initialize logging
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env().add_directive("github_release_collector=info".parse().unwrap()))
        .init();

    // Parse CLI arguments
    let cli = Cli::parse();

    // Run the application
    if let Err(e) = run(cli) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), Error> {
    // Load configuration
    let config = Config::load()?;

    // Initialize database
    let db = Database::new(&config)?;

    if cli.daemon_mode {
        return daemon::daemon_loop(&config, &db);
    }

    // Execute command
    match cli.command {
        CliCommand::Init => {
            println!("Database initialized at: {}", config.database_path().display());
            Ok(())
        }
        CliCommand::RepoAdd { repo } => {
            db.add_repo(&repo)?;
            println!("Added repository: {}", repo);
            Ok(())
        }
        CliCommand::RepoRemove { repo, keep_data } => {
            db.remove_repo(&repo, keep_data)?;
            println!("Removed repository: {}", repo);
            Ok(())
        }
        CliCommand::RepoEnable { repo } => {
            db.enable_repo(&repo, true)?;
            println!("Enabled repository: {}", repo);
            Ok(())
        }
        CliCommand::RepoDisable { repo } => {
            db.enable_repo(&repo, false)?;
            println!("Disabled repository: {}", repo);
            Ok(())
        }
        CliCommand::RepoList => {
            let repos = db.list_repos()?;
            for r in repos {
                println!("{}", r);
            }
            Ok(())
        }
        CliCommand::SyncRun { full } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async {
                github_release_collector::sync::run_sync(&config, &db, full).await
            })?;
            Ok(())
        }
        CliCommand::DaemonStart => {
            daemon::start_daemon(&config)?;
            Ok(())
        }
        CliCommand::DaemonStop => {
            daemon::stop_daemon(&config)?;
            Ok(())
        }
        CliCommand::DaemonStatus => {
            daemon::show_status(&config)?;
            Ok(())
        }
        CliCommand::DaemonReload => {
            daemon::reload_daemon(&config)?;
            Ok(())
        }
        CliCommand::ConfigShow => {
            println!("{}", config);
            Ok(())
        }
        CliCommand::ConfigSet { key, value } => {
            config.set(&key, &value)?;
            println!("Updated configuration: {} = {}", key, value);
            Ok(())
        }
        CliCommand::QueryList { repo, limit, include_deleted } => {
            let releases = db.query_releases(repo.as_deref(), Some(limit), include_deleted)?;
            for r in releases {
                println!("{}", r);
            }
            Ok(())
        }
        CliCommand::QueryShow { id, tag, full } => {
            let release = if let Some(release_id) = id {
                db.get_release_by_id(release_id)?
            } else if let Some(ref t) = tag {
                db.get_release_by_tag(t)?
            } else {
                return Err(Error::NotFound("Either --id or --tag required".to_string()));
            };
            
            if let Some(r) = release {
                if full {
                    println!("{}", r.full());
                } else {
                    println!("{}", r);
                }
                Ok(())
            } else {
                Err(Error::NotFound("Release not found".to_string()))
            }
        }
        CliCommand::QuerySearch { keyword, limit } => {
            let results = db.search_releases(&keyword, Some(limit))?;
            for r in results {
                println!("{}", r);
            }
            Ok(())
        }
        CliCommand::Status => {
            let repos = db.list_repos()?;
            let total_releases = db.get_total_releases()?;
            println!("Total repositories: {}", repos.len());
            println!("Total releases: {}", total_releases);
            for r in repos {
                println!("  {}", r);
            }
            Ok(())
        }
    }
}

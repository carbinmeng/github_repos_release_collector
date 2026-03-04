use std::fs;
use std::path::PathBuf;
use std::process;
use std::time::Duration;

use crate::config::Config;
use crate::{Error, Result};

pub fn start_daemon(config: &Config) -> Result<()> {
    let pid_path = config.pid_path.clone().unwrap_or_else(|| Config::data_dir().join("daemon.pid"));
    
    // Check if daemon is already running
    if pid_path.exists() {
        if let Ok(pid) = fs::read_to_string(&pid_path) {
            let pid: u32 = pid.trim().parse().unwrap_or(0);
            if pid > 0 {
                // Check if process is running
                #[cfg(unix)]
                {
                    if process::Command::new("kill")
                        .arg("-0")
                        .arg(pid.to_string())
                        .output()
                        .map(|o| o.status.success())
                        .unwrap_or(false)
                    {
                        return Err(Error::Daemon(format!("Daemon is already running with PID {}", pid)));
                    }
                }
            }
        }
        // Remove stale PID file
        fs::remove_file(&pid_path).ok();
    }

    // Create daemon process
    #[cfg(unix)]
    {
        let child = process::Command::new(std::env::current_exe()?)
            .args(std::env::args().skip(1))
            .arg("--daemon-mode")
            .spawn();
        
        match child {
            Ok(child) => {
                // Write PID file
                fs::create_dir_all(pid_path.parent().unwrap_or(&PathBuf::from(".")))?;
                fs::write(&pid_path, child.id().to_string())?;
                println!("Daemon started with PID: {}", child.id());
            }
            Err(e) => {
                return Err(Error::Daemon(format!("Failed to start daemon: {}", e)));
            }
        }
    }

    #[cfg(not(unix))]
    {
        return Err(Error::Daemon("Daemon mode is only supported on Unix-like systems".to_string()));
    }

    Ok(())
}

pub fn stop_daemon(config: &Config) -> Result<()> {
    let pid_path = config.pid_path.clone().unwrap_or_else(|| Config::data_dir().join("daemon.pid"));
    
    if !pid_path.exists() {
        return Err(Error::Daemon("PID file not found. Is daemon running?".to_string()));
    }

    let pid: u32 = fs::read_to_string(&pid_path)?
        .trim()
        .parse()
        .map_err(|_| Error::Daemon("Invalid PID file".to_string()))?;

    // Send SIGTERM
    #[cfg(unix)]
    {
        let result = process::Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .output();

        match result {
            Ok(output) if output.status.success() => {
                // Wait for process to stop
                let mut attempts = 0;
                while attempts < 10 {
                    if process::Command::new("kill")
                        .arg("-0")
                        .arg(pid.to_string())
                        .output()
                        .map(|o| !o.status.success())
                        .unwrap_or(true)
                    {
                        break;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                    attempts += 1;
                }

                fs::remove_file(&pid_path).ok();
                println!("Daemon stopped");
                Ok(())
            }
            Ok(output) => {
                Err(Error::Daemon(format!("Failed to stop daemon: {}", String::from_utf8_lossy(&output.stderr))))
            }
            Err(e) => {
                Err(Error::Daemon(format!("Failed to execute kill: {}", e)))
            }
        }
    }

    #[cfg(not(unix))]
    {
        Err(Error::Daemon("Daemon mode is only supported on Unix-like systems".to_string()))
    }
}

pub fn show_status(config: &Config) -> Result<()> {
    let pid_path = config.pid_path.clone().unwrap_or_else(|| Config::data_dir().join("daemon.pid"));
    
    if !pid_path.exists() {
        println!("Daemon is not running");
        return Ok(());
    }

    let pid: u32 = fs::read_to_string(&pid_path)?
        .trim()
        .parse()
        .map_err(|_| Error::Daemon("Invalid PID file".to_string()))?;

    // Check if process is running
    #[cfg(unix)]
    {
        let running = process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if running {
            println!("Daemon is running (PID: {})", pid);
        } else {
            println!("Daemon is not running (stale PID file)");
            fs::remove_file(&pid_path).ok();
        }
    }

    #[cfg(not(unix))]
    {
        println!("Daemon is running (PID: {})", pid);
    }

    Ok(())
}

pub fn reload_daemon(config: &Config) -> Result<()> {
    let pid_path = config.pid_path.clone().unwrap_or_else(|| Config::data_dir().join("daemon.pid"));
    
    if !pid_path.exists() {
        return Err(Error::Daemon("Daemon is not running".to_string()));
    }

    let pid: u32 = fs::read_to_string(&pid_path)?
        .trim()
        .parse()
        .map_err(|_| Error::Daemon("Invalid PID file".to_string()))?;

    // Send SIGHUP
    #[cfg(unix)]
    {
        let result = process::Command::new("kill")
            .arg("-HUP")
            .arg(pid.to_string())
            .output();

        match result {
            Ok(output) if output.status.success() => {
                println!("Daemon reload signal sent");
                Ok(())
            }
            Ok(output) => {
                Err(Error::Daemon(format!("Failed to reload daemon: {}", String::from_utf8_lossy(&output.stderr))))
            }
            Err(e) => {
                Err(Error::Daemon(format!("Failed to execute kill: {}", e)))
            }
        }
    }

    #[cfg(not(unix))]
    {
        Err(Error::Daemon("Reload is only supported on Unix-like systems".to_string()))
    }
}

pub fn daemon_loop(config: &Config, db: &crate::db::Database) -> Result<()> {
    use crate::sync;
    
    let mut last_incremental = std::time::Instant::now();
    let mut last_full = std::time::Instant::now();

    println!("Daemon mode started");
    println!("Incremental interval: {}s", config.incremental_interval);
    println!("Full interval: {}s", config.full_interval);

    loop {
        let now = std::time::Instant::now();
        
        // Check if it's time for incremental sync
        if now.duration_since(last_incremental).as_secs() >= config.incremental_interval {
            println!("Running incremental sync...");
            let rt = tokio::runtime::Runtime::new().unwrap();
            match rt.block_on(sync::run_sync(config, db, false)) {
                Ok(_) => {
                    println!("Incremental sync completed");
                }
                Err(e) => {
                    eprintln!("Incremental sync failed: {}", e);
                }
            }
            last_incremental = now;
        }

        // Check if it's time for full sync
        if now.duration_since(last_full).as_secs() >= config.full_interval {
            println!("Running full sync...");
            let rt = tokio::runtime::Runtime::new().unwrap();
            match rt.block_on(sync::run_sync(config, db, true)) {
                Ok(_) => {
                    println!("Full sync completed");
                }
                Err(e) => {
                    eprintln!("Full sync failed: {}", e);
                }
            }
            last_full = now;
        }

        // Sleep for a bit before checking again
        std::thread::sleep(Duration::from_secs(60));
    }
}

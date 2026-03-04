use std::fs;
use std::sync::Mutex;

use chrono::Utc;
use rusqlite::{params, Connection, Result as SqliteResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::Result;

pub struct Database {
    pub conn: Mutex<Connection>,
}

impl Database {
    pub fn new(config: &Config) -> Result<Self> {
        let db_path = config.database_path();
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        
        let db = Self { conn: Mutex::new(conn) };
        db.init_tables()?;
        Ok(db)
    }

    fn init_tables(&self) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS repos (
                repo TEXT PRIMARY KEY,
                enabled INTEGER NOT NULL DEFAULT 1,
                last_polled_at TEXT,
                last_full_sync_at TEXT,
                last_success_at TEXT,
                last_error TEXT,
                fail_count INTEGER NOT NULL DEFAULT 0,
                etag_page1 TEXT
            );

            CREATE TABLE IF NOT EXISTS releases (
                release_id INTEGER PRIMARY KEY,
                repo TEXT NOT NULL,
                tag_name TEXT,
                name TEXT,
                draft INTEGER NOT NULL DEFAULT 0,
                prerelease INTEGER NOT NULL DEFAULT 0,
                created_at TEXT,
                published_at TEXT,
                updated_at TEXT,
                html_url TEXT,
                body TEXT,
                body_hash TEXT,
                is_deleted INTEGER NOT NULL DEFAULT 0,
                deleted_at TEXT,
                UNIQUE(repo, release_id)
            );

            CREATE INDEX IF NOT EXISTS idx_releases_repo_updated ON releases(repo, updated_at);
            CREATE INDEX IF NOT EXISTS idx_releases_repo ON releases(repo);
            "#,
        )?;
        Ok(())
    }

    // ============ Repo Operations ============

    pub fn add_repo(&self, repo: &str) -> Result<()> {
        let normalized = normalize_repo(repo);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO repos (repo, enabled) VALUES (?1, 1)",
            params![normalized],
        )?;
        Ok(())
    }

    pub fn remove_repo(&self, repo: &str, keep_data: bool) -> Result<()> {
        let normalized = normalize_repo(repo);
        let conn = self.conn.lock().unwrap();
        if !keep_data {
            conn.execute("DELETE FROM releases WHERE repo = ?1", params![normalized])?;
        }
        conn.execute("DELETE FROM repos WHERE repo = ?1", params![normalized])?;
        Ok(())
    }

    pub fn enable_repo(&self, repo: &str, enabled: bool) -> Result<()> {
        let normalized = normalize_repo(repo);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE repos SET enabled = ?1 WHERE repo = ?2",
            params![enabled as i32, normalized],
        )?;
        Ok(())
    }

    pub fn get_repo(&self, repo: &str) -> Result<Option<Repo>> {
        let normalized = normalize_repo(repo);
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT repo, enabled, last_polled_at, last_full_sync_at, last_success_at, last_error, fail_count, etag_page1 FROM repos WHERE repo = ?1"
        )?;
        
        let result = stmt.query_row(params![normalized], |row| {
            Ok(Repo {
                repo: row.get(0)?,
                enabled: row.get::<_, i32>(1)? != 0,
                last_polled_at: row.get::<_, Option<String>>(2)?,
                last_full_sync_at: row.get::<_, Option<String>>(3)?,
                last_success_at: row.get::<_, Option<String>>(4)?,
                last_error: row.get::<_, Option<String>>(5)?,
                fail_count: row.get(6)?,
                etag_page1: row.get::<_, Option<String>>(7)?,
            })
        });

        match result {
            Ok(repo) => Ok(Some(repo)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn list_repos(&self) -> Result<Vec<Repo>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT r.repo, r.enabled, r.last_polled_at, r.last_full_sync_at, r.last_success_at, r.last_error, r.fail_count, r.etag_page1
             FROM repos r
             ORDER BY r.repo"
        )?;
        
        let repos = stmt.query_map([], |row| {
            Ok(RepoWithCount {
                repo: row.get(0)?,
                enabled: row.get::<_, i32>(1)? != 0,
                last_polled_at: row.get::<_, Option<String>>(2)?,
                last_full_sync_at: row.get::<_, Option<String>>(3)?,
                last_success_at: row.get::<_, Option<String>>(4)?,
                last_error: row.get::<_, Option<String>>(5)?,
                fail_count: row.get(6)?,
                etag_page1: row.get::<_, Option<String>>(7)?,
            })
        })?.collect::<SqliteResult<Vec<_>>>()?;

        Ok(repos.into_iter().map(|r| Repo::from(r)).collect())
    }

    pub fn get_enabled_repos(&self) -> Result<Vec<Repo>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT repo, enabled, last_polled_at, last_full_sync_at, last_success_at, last_error, fail_count, etag_page1 FROM repos WHERE enabled = 1"
        )?;
        
        let repos = stmt.query_map([], |row| {
            Ok(Repo {
                repo: row.get(0)?,
                enabled: row.get::<_, i32>(1)? != 0,
                last_polled_at: row.get::<_, Option<String>>(2)?,
                last_full_sync_at: row.get::<_, Option<String>>(3)?,
                last_success_at: row.get::<_, Option<String>>(4)?,
                last_error: row.get::<_, Option<String>>(5)?,
                fail_count: row.get(6)?,
                etag_page1: row.get::<_, Option<String>>(7)?,
            })
        })?.collect::<SqliteResult<Vec<_>>>()?;

        Ok(repos)
    }

    pub fn update_repo_sync_status(&self, repo: &str, status: &SyncStatus) -> Result<()> {
        let normalized = normalize_repo(repo);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"UPDATE repos SET 
                last_polled_at = ?1,
                last_success_at = ?2,
                last_error = ?3,
                fail_count = ?4,
                etag_page1 = ?5,
                last_full_sync_at = CASE WHEN ?6 = 1 THEN ?1 ELSE last_full_sync_at END
               WHERE repo = ?7"#,
            params![
                status.started_at,
                status.success_at.as_deref(),
                status.error.as_deref(),
                status.fail_count,
                status.etag.as_deref(),
                status.is_full_sync,
                normalized
            ],
        )?;
        Ok(())
    }

    // ============ Release Operations ============

    pub fn upsert_release(&self, release: &Release) -> Result<UpsertResult> {
        // Calculate body hash
        let body_hash = calculate_hash(release.body.as_deref().unwrap_or(""));
        
        let conn = self.conn.lock().unwrap();
        
        // Check if exists
        let existing: Option<(String, i32)> = conn.query_row(
            "SELECT body_hash, is_deleted FROM releases WHERE release_id = ?1 AND repo = ?2",
            params![release.release_id, release.repo],
            |row| Ok((row.get(0)?, row.get(1)?))
        ).ok();

        let mut inserted = 0;
        let mut updated = 0;
        let mut skipped = 0;

        if let Some((existing_hash, is_deleted)) = existing {
            if is_deleted == 1 {
                // Restore deleted release
                conn.execute(
                    r#"UPDATE releases SET 
                        tag_name = ?1, name = ?2, draft = ?3, prerelease = ?4,
                        created_at = ?5, published_at = ?6, updated_at = ?7,
                        html_url = ?8, body = ?9, body_hash = ?10, is_deleted = 0, deleted_at = NULL
                       WHERE release_id = ?11 AND repo = ?12"#,
                    params![
                        release.tag_name, release.name, release.draft as i32, release.prerelease as i32,
                        release.created_at, release.published_at, release.updated_at,
                        release.html_url, release.body, body_hash,
                        release.release_id, release.repo
                    ],
                )?;
                updated += 1;
            } else if existing_hash != body_hash {
                // Update if body changed
                conn.execute(
                    r#"UPDATE releases SET 
                        tag_name = ?1, name = ?2, draft = ?3, prerelease = ?4,
                        created_at = ?5, published_at = ?6, updated_at = ?7,
                        html_url = ?8, body = ?9, body_hash = ?10
                       WHERE release_id = ?11 AND repo = ?12"#,
                    params![
                        release.tag_name, release.name, release.draft as i32, release.prerelease as i32,
                        release.created_at, release.published_at, release.updated_at,
                        release.html_url, release.body, body_hash,
                        release.release_id, release.repo
                    ],
                )?;
                updated += 1;
            } else {
                skipped += 1;
            }
        } else {
            // Insert new
            conn.execute(
                r#"INSERT INTO releases 
                   (release_id, repo, tag_name, name, draft, prerelease, created_at, published_at, updated_at, html_url, body, body_hash, is_deleted)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0)"#,
                params![
                    release.release_id, release.repo, release.tag_name, release.name, 
                    release.draft as i32, release.prerelease as i32,
                    release.created_at, release.published_at, release.updated_at,
                    release.html_url, release.body, body_hash
                ],
            )?;
            inserted += 1;
        }

        Ok(UpsertResult { inserted, updated, skipped })
    }

    pub fn mark_releases_deleted(&self, repo: &str, release_ids: &[i64]) -> Result<()> {
        let normalized = normalize_repo(repo);
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        for id in release_ids {
            tx.execute(
                "UPDATE releases SET is_deleted = 1, deleted_at = ?1 WHERE release_id = ?2 AND repo = ?3 AND is_deleted = 0",
                params![Utc::now().to_rfc3339(), id, normalized],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn query_releases(&self, repo: Option<&str>, limit: Option<usize>, include_deleted: bool) -> Result<Vec<Release>> {
        let limit = limit.unwrap_or(50);
        let repo_filter = repo.map(|r| normalize_repo(r));
        let conn = self.conn.lock().unwrap();

        let base_sql =
            "SELECT release_id, repo, tag_name, name, draft, prerelease, created_at, published_at, updated_at, html_url, body, body_hash, is_deleted, deleted_at FROM releases";

        let map_row = |row: &rusqlite::Row<'_>| {
            Ok(Release {
                release_id: row.get(0)?,
                repo: row.get(1)?,
                tag_name: row.get(2)?,
                name: row.get(3)?,
                draft: row.get::<_, i32>(4)? != 0,
                prerelease: row.get::<_, i32>(5)? != 0,
                created_at: row.get(6)?,
                published_at: row.get(7)?,
                updated_at: row.get(8)?,
                html_url: row.get(9)?,
                body: row.get(10)?,
                body_hash: row.get(11)?,
                is_deleted: row.get::<_, i32>(12)? != 0,
                deleted_at: row.get(13)?,
            })
        };

        let releases = match (repo_filter.as_deref(), include_deleted) {
            (Some(repo), true) => {
                let sql = format!("{base_sql} WHERE repo = ?1 ORDER BY published_at DESC LIMIT ?2");
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(params![repo, limit], map_row)?;
                rows.collect::<SqliteResult<Vec<_>>>()?
            }
            (Some(repo), false) => {
                let sql = format!(
                    "{base_sql} WHERE repo = ?1 AND is_deleted = 0 ORDER BY published_at DESC LIMIT ?2"
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(params![repo, limit], map_row)?;
                rows.collect::<SqliteResult<Vec<_>>>()?
            }
            (None, true) => {
                let sql = format!("{base_sql} ORDER BY published_at DESC LIMIT ?1");
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(params![limit], map_row)?;
                rows.collect::<SqliteResult<Vec<_>>>()?
            }
            (None, false) => {
                let sql = format!("{base_sql} WHERE is_deleted = 0 ORDER BY published_at DESC LIMIT ?1");
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(params![limit], map_row)?;
                rows.collect::<SqliteResult<Vec<_>>>()?
            }
        };

        Ok(releases)
    }

    pub fn get_release_by_id(&self, release_id: i64) -> Result<Option<Release>> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT release_id, repo, tag_name, name, draft, prerelease, created_at, published_at, updated_at, html_url, body, body_hash, is_deleted, deleted_at FROM releases WHERE release_id = ?1",
            params![release_id],
            |row| {
                Ok(Release {
                    release_id: row.get(0)?,
                    repo: row.get(1)?,
                    tag_name: row.get(2)?,
                    name: row.get(3)?,
                    draft: row.get::<_, i32>(4)? != 0,
                    prerelease: row.get::<_, i32>(5)? != 0,
                    created_at: row.get(6)?,
                    published_at: row.get(7)?,
                    updated_at: row.get(8)?,
                    html_url: row.get(9)?,
                    body: row.get(10)?,
                    body_hash: row.get(11)?,
                    is_deleted: row.get::<_, i32>(12)? != 0,
                    deleted_at: row.get(13)?,
                })
            }
        );

        match result {
            Ok(r) => Ok(Some(r)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_release_by_tag(&self, tag: &str) -> Result<Option<Release>> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT release_id, repo, tag_name, name, draft, prerelease, created_at, published_at, updated_at, html_url, body, body_hash, is_deleted, deleted_at FROM releases WHERE tag_name = ?1 AND is_deleted = 0 ORDER BY published_at DESC LIMIT 1",
            params![tag],
            |row| {
                Ok(Release {
                    release_id: row.get(0)?,
                    repo: row.get(1)?,
                    tag_name: row.get(2)?,
                    name: row.get(3)?,
                    draft: row.get::<_, i32>(4)? != 0,
                    prerelease: row.get::<_, i32>(5)? != 0,
                    created_at: row.get(6)?,
                    published_at: row.get(7)?,
                    updated_at: row.get(8)?,
                    html_url: row.get(9)?,
                    body: row.get(10)?,
                    body_hash: row.get(11)?,
                    is_deleted: row.get::<_, i32>(12)? != 0,
                    deleted_at: row.get(13)?,
                })
            }
        );

        match result {
            Ok(r) => Ok(Some(r)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn search_releases(&self, keyword: &str, limit: Option<usize>) -> Result<Vec<Release>> {
        let limit = limit.unwrap_or(50);
        let pattern = format!("%{}%", keyword);
        
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT release_id, repo, tag_name, name, draft, prerelease, created_at, published_at, updated_at, html_url, body, body_hash, is_deleted, deleted_at 
             FROM releases 
             WHERE is_deleted = 0 AND (name LIKE ?1 OR body LIKE ?1) 
             ORDER BY published_at DESC LIMIT ?2"
        )?;
        
        let releases = stmt.query_map(params![pattern, limit], |row| {
            Ok(Release {
                release_id: row.get(0)?,
                repo: row.get(1)?,
                tag_name: row.get(2)?,
                name: row.get(3)?,
                draft: row.get::<_, i32>(4)? != 0,
                prerelease: row.get::<_, i32>(5)? != 0,
                created_at: row.get(6)?,
                published_at: row.get(7)?,
                updated_at: row.get(8)?,
                html_url: row.get(9)?,
                body: row.get(10)?,
                body_hash: row.get(11)?,
                is_deleted: row.get::<_, i32>(12)? != 0,
                deleted_at: row.get(13)?,
            })
        })?.collect::<SqliteResult<Vec<_>>>()?;

        Ok(releases)
    }

    pub fn get_total_releases(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM releases WHERE is_deleted = 0",
            [],
            |row| row.get(0)
        )?;
        Ok(count)
    }

    pub fn get_release_ids(&self, repo: &str) -> Result<Vec<i64>> {
        let normalized = normalize_repo(repo);
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT release_id FROM releases WHERE repo = ?1"
        )?;
        let ids = stmt.query_map(params![normalized], |row| row.get(0))?
            .collect::<SqliteResult<Vec<_>>>()?;
        Ok(ids)
    }

    // ============ Methods needed by sync module ============

    /// Get all releases for a repo (including body_hash for comparison)
    pub fn get_releases_by_repo(&self, repo: &str) -> Result<Vec<Release>> {
        let normalized = normalize_repo(repo);
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT release_id, repo, tag_name, name, draft, prerelease, created_at, published_at, updated_at, html_url, body, body_hash, is_deleted, deleted_at 
             FROM releases WHERE repo = ?1 AND is_deleted = 0 ORDER BY published_at DESC"
        )?;
        
        let releases = stmt.query_map(params![normalized], |row| {
            Ok(Release {
                release_id: row.get(0)?,
                repo: row.get(1)?,
                tag_name: row.get(2)?,
                name: row.get(3)?,
                draft: row.get::<_, i32>(4)? != 0,
                prerelease: row.get::<_, i32>(5)? != 0,
                created_at: row.get(6)?,
                published_at: row.get(7)?,
                updated_at: row.get(8)?,
                html_url: row.get(9)?,
                body: row.get(10)?,
                body_hash: row.get(11)?,
                is_deleted: row.get::<_, i32>(12)? != 0,
                deleted_at: row.get(13)?,
            })
        })?.collect::<SqliteResult<Vec<_>>>()?;

        Ok(releases)
    }

    /// Get the ETag for a repo (for conditional GitHub API requests)
    pub fn get_repo_etag(&self, repo: &str) -> Result<Option<String>> {
        let normalized = normalize_repo(repo);
        let conn = self.conn.lock().unwrap();
        let etag: Option<String> = conn.query_row(
            "SELECT etag_page1 FROM repos WHERE repo = ?1",
            params![normalized],
            |row| row.get(0)
        ).ok();
        Ok(etag)
    }

    /// Get all repos (for syncing all repos)
    pub fn get_all_repos(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT repo FROM repos WHERE enabled = 1")?;
        let repos = stmt.query_map([], |row| row.get(0))?
            .collect::<SqliteResult<Vec<_>>>()?;
        Ok(repos)
    }

    /// Insert a single release
    pub fn insert_release(&self, release: &Release) -> Result<()> {
        let body_hash = calculate_hash(release.body.as_deref().unwrap_or(""));
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"INSERT OR REPLACE INTO releases 
               (release_id, repo, tag_name, name, draft, prerelease, created_at, published_at, updated_at, html_url, body, body_hash, is_deleted)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0)"#,
            params![
                release.release_id, release.repo, release.tag_name, release.name, 
                release.draft as i32, release.prerelease as i32,
                release.created_at, release.published_at, release.updated_at,
                release.html_url, release.body, body_hash
            ],
        )?;
        Ok(())
    }

    /// Update an existing release
    pub fn update_release(&self, release: &Release) -> Result<()> {
        let body_hash = calculate_hash(release.body.as_deref().unwrap_or(""));
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"UPDATE releases SET 
                tag_name = ?1, name = ?2, draft = ?3, prerelease = ?4,
                created_at = ?5, published_at = ?6, updated_at = ?7,
                html_url = ?8, body = ?9, body_hash = ?10
               WHERE release_id = ?11 AND repo = ?12"#,
            params![
                release.tag_name, release.name, release.draft as i32, release.prerelease as i32,
                release.created_at, release.published_at, release.updated_at,
                release.html_url, release.body, body_hash,
                release.release_id, release.repo
            ],
        )?;
        Ok(())
    }
}

fn normalize_repo(repo: &str) -> String {
    // Handle https://github.com/owner/repo format
    if repo.contains("github.com") {
        if let Some(path) = repo.strip_prefix("https://github.com/") {
            return path.trim_end_matches('/').to_string();
        } else if let Some(path) = repo.strip_prefix("http://github.com/") {
            return path.trim_end_matches('/').to_string();
        }
    }
    repo.trim_end_matches('/').to_string()
}

fn calculate_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

// ============ Data Models ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    pub repo: String,
    pub enabled: bool,
    pub last_polled_at: Option<String>,
    pub last_full_sync_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_error: Option<String>,
    pub fail_count: i32,
    pub etag_page1: Option<String>,
}

#[derive(Debug, Clone)]
struct RepoWithCount {
    pub repo: String,
    pub enabled: bool,
    pub last_polled_at: Option<String>,
    pub last_full_sync_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_error: Option<String>,
    pub fail_count: i32,
    pub etag_page1: Option<String>,
}

impl From<RepoWithCount> for Repo {
    fn from(r: RepoWithCount) -> Self {
        Self {
            repo: r.repo,
            enabled: r.enabled,
            last_polled_at: r.last_polled_at,
            last_full_sync_at: r.last_full_sync_at,
            last_success_at: r.last_success_at,
            last_error: r.last_error,
            fail_count: r.fail_count,
            etag_page1: r.etag_page1,
        }
    }
}

impl std::fmt::Display for Repo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status = if self.enabled { "enabled" } else { "disabled" };
        let last_sync = self.last_success_at.as_deref().unwrap_or("never");
        let error = self.last_error.as_deref().map(|e| format!(" | Error: {}", e)).unwrap_or_default();
        
        write!(f, "{} [{}] | Last sync: {} | Failures: {}{}", 
            self.repo, status, last_sync, self.fail_count, error)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    pub release_id: i64,
    pub repo: String,
    pub tag_name: Option<String>,
    pub name: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
    pub created_at: Option<String>,
    pub published_at: Option<String>,
    pub updated_at: Option<String>,
    pub html_url: Option<String>,
    pub body: Option<String>,
    pub body_hash: Option<String>,
    pub is_deleted: bool,
    pub deleted_at: Option<String>,
}

impl std::fmt::Display for Release {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = self.name.as_deref().unwrap_or(self.tag_name.as_deref().unwrap_or("Unnamed"));
        let published = self.published_at.as_deref().unwrap_or("unknown");
        
        write!(f, "[{}] {} - {} ({})", 
            self.release_id, self.repo, name, published)
    }
}

impl Release {
    pub fn full(&self) -> String {
        let name = self.name.as_deref().unwrap_or(self.tag_name.as_deref().unwrap_or("Unnamed"));
        let tag = self.tag_name.as_deref().unwrap_or("no tag");
        let body = self.body.as_deref().unwrap_or("");
        
        format!(
            "Release: {} ({})\nTag: {}\nRepo: {}\nDraft: {} | Pre-release: {}\nCreated: {}\nPublished: {}\nUpdated: {}\nURL: {}\n\n{}",
            name,
            self.release_id,
            tag,
            self.repo,
            self.draft,
            self.prerelease,
            self.created_at.as_deref().unwrap_or("N/A"),
            self.published_at.as_deref().unwrap_or("N/A"),
            self.updated_at.as_deref().unwrap_or("N/A"),
            self.html_url.as_deref().unwrap_or("N/A"),
            body
        )
    }
}

#[derive(Debug)]
pub struct SyncStatus {
    pub started_at: String,
    pub success_at: Option<String>,
    pub error: Option<String>,
    pub fail_count: i32,
    pub etag: Option<String>,
    pub is_full_sync: bool,
}

impl SyncStatus {
    pub fn new(is_full_sync: bool) -> Self {
        Self {
            started_at: Utc::now().to_rfc3339(),
            success_at: None,
            error: None,
            fail_count: 0,
            etag: None,
            is_full_sync,
        }
    }
}

#[derive(Debug)]
pub struct UpsertResult {
    pub inserted: i32,
    pub updated: i32,
    pub skipped: i32,
}

impl UpsertResult {
    pub fn total(&self) -> i32 {
        self.inserted + self.updated + self.skipped
    }
}

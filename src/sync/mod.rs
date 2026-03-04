use std::sync::Arc;
use std::collections::{HashMap, HashSet};
use chrono::Utc;
use tokio::sync::Semaphore;

use crate::config::Config;
use crate::db::{Database, Release, SyncStatus};
use crate::github::{GithubClient, GithubRelease};
use crate::{Error, Result};

pub struct SyncEngine {
    config: Arc<Config>,
    db: Arc<Database>,
    concurrency_limit: usize,
}

impl SyncEngine {
    pub fn new(config: Arc<Config>, db: Arc<Database>) -> Result<Self> {
        let concurrency_limit = config.concurrency;
        
        Ok(Self {
            config,
            db,
            concurrency_limit,
        })
    }

    pub async fn sync_repo(&self, repo: &str, incremental: bool) -> Result<(usize, usize)> {
        Self::sync_repo_once(self.config.as_ref(), self.db.as_ref(), repo, incremental).await
    }

    pub async fn sync_all(&self, incremental: bool) -> Result<Vec<(String, usize, usize)>> {
        let repos = self.db.get_all_repos()?;
        
        // Use semaphore to limit concurrent syncs
        let semaphore = Arc::new(Semaphore::new(self.concurrency_limit));
        let mut handles = vec![];

        for repo in repos {
            let repo_name = repo.clone();
            let config = Arc::clone(&self.config);
            let db = Arc::clone(&self.db);
            let sem = Arc::clone(&semaphore);

            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await.map_err(|e| Error::Sync(e.to_string()))?;
                let (new_count, updated_count) =
                    Self::sync_repo_once(config.as_ref(), db.as_ref(), &repo_name, incremental).await?;

                Ok::<_, Error>((repo_name, new_count, updated_count))
            });

            handles.push(handle);
        }

        let mut results = vec![];
        
        for handle in handles {
            match handle.await {
                Ok(Ok(result)) => results.push(result),
                Ok(Err(e)) => return Err(e),
                Err(e) => return Err(Error::Sync(format!("Task join error: {}", e))),
            }
        }

        Ok(results)
    }

    async fn sync_repo_once(
        config: &Config,
        db: &Database,
        repo: &str,
        incremental: bool,
    ) -> Result<(usize, usize)> {
        let repo_state = db.get_repo(repo)?;
        let previous_fail_count = repo_state.as_ref().map_or(0, |state| state.fail_count);
        let previous_etag = repo_state.and_then(|state| state.etag_page1);
        let mut status = SyncStatus::new(!incremental);
        status.etag = previous_etag.clone();

        let result: Result<(usize, usize)> = async {
            let client = GithubClient::new(config)?;

            let github_releases = if incremental {
                let page = client.fetch_releases(repo, previous_etag.as_deref()).await?;
                status.etag = page.etag.or_else(|| previous_etag.clone());

                if page.not_modified {
                    status.success_at = Some(Utc::now().to_rfc3339());
                    return Ok((0, 0));
                }

                page.releases
            } else {
                client.fetch_all_releases(repo).await?
            };

            let existing_releases = db.get_releases_by_repo(repo)?;
            let existing_by_id: HashMap<_, _> = existing_releases
                .iter()
                .map(|r| (r.release_id, r.body_hash.clone()))
                .collect();

            let mut new_count = 0;
            let mut updated_count = 0;
            let mut fetched_ids = HashSet::with_capacity(github_releases.len());

            for gr in github_releases {
                fetched_ids.insert(gr.id);
                let body_hash = Self::compute_body_hash(gr.body.as_deref().unwrap_or(""));

                if let Some(existing_hash) = existing_by_id.get(&gr.id) {
                    if existing_hash.as_ref() != Some(&body_hash) {
                        let release = Self::convert_release(repo, &gr, &body_hash);
                        db.update_release(&release)?;
                        updated_count += 1;
                    }
                } else {
                    let release = Self::convert_release(repo, &gr, &body_hash);
                    db.insert_release(&release)?;
                    new_count += 1;
                }
            }

            if !incremental {
                let deleted_ids: Vec<_> = db
                    .get_release_ids(repo)?
                    .into_iter()
                    .filter(|id| !fetched_ids.contains(id))
                    .collect();

                if !deleted_ids.is_empty() {
                    db.mark_releases_deleted(repo, &deleted_ids)?;
                }
            }

            status.success_at = Some(Utc::now().to_rfc3339());
            Ok((new_count, updated_count))
        }
        .await;

        match result {
            Ok(counts) => {
                db.update_repo_sync_status(repo, &status)?;
                Ok(counts)
            }
            Err(err) => {
                status.error = Some(err.to_string());
                status.fail_count = previous_fail_count + 1;
                db.update_repo_sync_status(repo, &status)?;
                Err(err)
            }
        }
    }

    fn compute_body_hash(body: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        body.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    fn convert_release(repo: &str, gr: &GithubRelease, body_hash: &str) -> Release {
        Release {
            release_id: gr.id, // Use GitHub's release ID as release_id
            repo: repo.to_string(),
            tag_name: gr.tag_name.clone(),
            name: gr.name.clone(),
            draft: gr.draft,
            prerelease: gr.prerelease,
            created_at: gr.created_at.clone(),
            published_at: gr.published_at.clone(),
            updated_at: gr.updated_at.clone(),
            html_url: gr.html_url.clone(),
            body: gr.body.clone(),
            body_hash: Some(body_hash.to_string()),
            is_deleted: false,
            deleted_at: None,
        }
    }

    pub fn set_concurrency_limit(&mut self, limit: usize) {
        self.concurrency_limit = limit;
    }
}

/// Run sync for all repositories
pub async fn run_sync(config: &Config, _db: &Database, full: bool) -> Result<()> {
    use std::sync::Arc;
    
    let config = Arc::new(config.clone());
    // Create a new Database instance with same connection path
    let db = Arc::new(Database::new(config.as_ref())?);
    
    let engine = SyncEngine::new(config, db)?;
    let results = engine.sync_all(!full).await?;
    
    for (repo, new_count, updated_count) in results {
        println!("{}: {} new, {} updated", repo, new_count, updated_count);
    }
    
    Ok(())
}

use reqwest::Client;
use serde::Deserialize;

use crate::config::Config;
use crate::{Error, Result};

pub struct GithubClient {
    client: Client,
    token: Option<String>,
    per_page: usize,
}

#[derive(Debug)]
pub struct ReleasePage {
    pub releases: Vec<GithubRelease>,
    pub etag: Option<String>,
    pub not_modified: bool,
}

#[derive(Debug, Deserialize)]
pub struct GithubRelease {
    pub id: i64,
    #[serde(rename = "tag_name")]
    pub tag_name: Option<String>,
    pub name: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
    #[serde(rename = "created_at")]
    pub created_at: Option<String>,
    #[serde(rename = "published_at")]
    pub published_at: Option<String>,
    #[serde(rename = "updated_at")]
    pub updated_at: Option<String>,
    #[serde(rename = "html_url")]
    pub html_url: Option<String>,
    pub body: Option<String>,
}

impl GithubClient {
    pub fn new(config: &Config) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout))
            .user_agent("github-release-collector/1.0")
            .build()
            .map_err(|e| Error::Config(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            client,
            token: config.github_token().map(String::from),
            per_page: config.per_page,
        })
    }

    pub async fn fetch_releases(&self, repo: &str, etag: Option<&str>) -> Result<ReleasePage> {
        let url = format!(
            "https://api.github.com/repos/{}/releases?per_page={}&page=1",
            repo, self.per_page
        );

        let mut request = self.client.get(&url);

        // Add authentication header if token is available
        if let Some(token) = &self.token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        // Add ETag for conditional request
        if let Some(etag) = etag {
            request = request.header("If-None-Match", etag);
        }

        let response = request.send().await?;

        // Handle 304 Not Modified
        if response.status() == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(ReleasePage {
                releases: Vec::new(),
                etag: etag.map(String::from),
                not_modified: true,
            });
        }

        // Check rate limiting
        if response.status() == reqwest::StatusCode::FORBIDDEN {
            let remaining = response.headers()
                .get("X-RateLimit-Remaining")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("unknown");
            return Err(Error::GithubApi(format!("Rate limited. Remaining: {}", remaining)));
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::GithubApi(format!("HTTP {}: {}", status, body)));
        }

        // Get ETag from response
        let etag = Self::extract_etag(&response);

        let releases: Vec<GithubRelease> = response.json().await?;

        Ok(ReleasePage {
            releases,
            etag,
            not_modified: false,
        })
    }

    pub async fn fetch_all_releases(&self, repo: &str) -> Result<Vec<GithubRelease>> {
        let mut all_releases = Vec::new();
        let mut page = 1;
        let mut has_next = true;

        while has_next {
            let url = format!(
                "https://api.github.com/repos/{}/releases?per_page={}&page={}",
                repo, self.per_page, page
            );

            let mut request = self.client.get(&url);

            if let Some(token) = &self.token {
                request = request.header("Authorization", format!("Bearer {}", token));
            }

            let response = request.send().await?;

            if response.status() == reqwest::StatusCode::FORBIDDEN {
                let remaining = response.headers()
                    .get("X-RateLimit-Remaining")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("unknown");
                return Err(Error::GithubApi(format!("Rate limited. Remaining: {}", remaining)));
            }

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(Error::GithubApi(format!("HTTP {}: {}", status, body)));
            }

            let releases: Vec<GithubRelease> = response.json().await?;
            
            if releases.is_empty() {
                has_next = false;
            } else {
                all_releases.extend(releases);
                page += 1;
                
                // Safety limit to prevent infinite loops
                if page > 100 {
                    break;
                }
            }
        }

        Ok(all_releases)
    }

    pub fn extract_etag(response: &reqwest::Response) -> Option<String> {
        response.headers()
            .get("ETag")
            .and_then(|v| v.to_str().ok())
            .map(String::from)
    }
}

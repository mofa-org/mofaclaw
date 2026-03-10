//! Skills Hub client for MofaClaw
//! Provides integration with the OpenClaw Skills Hub for discovering and installing skills

use anyhow::{Context, Result, bail};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_SKILLS_HUB_CATALOG_URL: &str = "https://clawhub.run/api/skills/catalog";

const CATALOG_CACHE_FILE: &str = "catalog.json";
const REGISTRY_DIR: &str = "registry";

/// Hub skill version information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HubSkillVersion {
    pub version: String,
    #[serde(default, alias = "downloadUrl")]
    pub download_url: Option<String>,
    #[serde(default)]
    pub rating: Option<f32>,
}

/// A skill entry in the hub catalog
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HubSkillCatalogEntry {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub compatibility: Vec<String>,
    #[serde(default, alias = "latestVersion")]
    pub latest_version: Option<String>,
    #[serde(default, alias = "downloadUrl")]
    pub download_url: Option<String>,
    #[serde(default)]
    pub versions: Vec<HubSkillVersion>,
    #[serde(default)]
    pub rating: Option<f32>,
    #[serde(default)]
    pub downloads: Option<u64>,
}

impl HubSkillCatalogEntry {
    fn matches(&self, query: &str) -> bool {
        self.name.to_lowercase().contains(query)
            || self.description.to_lowercase().contains(query)
            || self
                .tags
                .iter()
                .any(|tag| tag.to_lowercase().contains(query))
    }

    fn download_url_for(&self, version: Option<&str>) -> Option<String> {
        if let Some(v) = version {
            self.versions
                .iter()
                .find(|vinfo| vinfo.version == v)
                .and_then(|vinfo| vinfo.download_url.clone())
        } else {
            self.download_url.clone()
        }
    }

    fn categories(&self) -> Vec<String> {
        if !self.categories.is_empty() {
            self.categories.clone()
        } else if let Some(cat) = &self.category {
            vec![cat.clone()]
        } else {
            Vec::new()
        }
    }
}

/// A file within a skill bundle
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HubSkillBundleFile {
    pub path: String,
    pub content: String,
}

/// A skill bundle downloaded from the hub
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HubSkillBundle {
    #[serde(default)]
    pub version: Option<String>,
    pub files: Vec<HubSkillBundleFile>,
}

/// An installed skill from the hub
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstalledHubSkill {
    pub name: String,
    pub version: Option<String>,
    pub install_dir: PathBuf,
    pub source_url: Option<String>,
}

/// Registry record for a hub-installed skill
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManagedHubSkillRecord {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub source: String,
    pub source_url: Option<String>,
    pub installed_path: PathBuf,
    pub catalog_url: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub compatibility: Vec<String>,
    #[serde(default)]
    pub rating: Option<f32>,
    pub installed_at_epoch_secs: u64,
    #[serde(default)]
    pub updated_at_epoch_secs: Option<u64>,
    #[serde(default)]
    pub last_used_at_epoch_secs: Option<u64>,
}

/// Cached hub catalog
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HubCatalogCache {
    pub catalog_url: String,
    pub fetched_at_epoch_secs: u64,
    pub entries: Vec<HubSkillCatalogEntry>,
}

/// Status of the hub catalog cache
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubCacheStatus {
    pub path: PathBuf,
    pub exists: bool,
    pub entry_count: usize,
    pub fetched_at_epoch_secs: Option<u64>,
}

/// Configuration for the Skills Hub client
#[derive(Debug, Clone)]
pub struct SkillHubClientConfig {
    pub catalog_url: String,
    pub auth_token: Option<String>,
    pub managed_root: PathBuf,
    pub cache_root: PathBuf,
}

impl SkillHubClientConfig {
    /// Create a new config with MofaClaw-specific defaults
    pub fn for_mofaclaw() -> Self {
        let managed_root = dirs::home_dir()
            .map(|h| h.join(".mofaclaw").join("skills").join("hub"))
            .unwrap_or_else(|| PathBuf::from(".mofaclaw/skills/hub"));
        let cache_root = dirs::home_dir()
            .map(|h| h.join(".mofaclaw").join("cache").join("hub"))
            .unwrap_or_else(|| PathBuf::from(".mofaclaw/cache/hub"));

        Self {
            catalog_url: DEFAULT_SKILLS_HUB_CATALOG_URL.to_string(),
            auth_token: Self::env_auth_token(),
            managed_root,
            cache_root,
        }
    }

    /// Create a config from the root MofaClaw configuration.
    pub fn from_config(config: &crate::Config) -> Self {
        Self::from_skills_config(&config.skills)
    }

    /// Create a config from the MofaClaw skills configuration.
    pub fn from_skills_config(skills: &crate::config::SkillsConfig) -> Self {
        Self::for_mofaclaw().with_catalog_url(skills.hub_url.clone())
    }

    /// Create a new config with a custom catalog URL
    pub fn with_catalog_url(mut self, catalog_url: impl Into<String>) -> Self {
        self.catalog_url = catalog_url.into();
        self
    }

    /// Set the auth token
    pub fn with_auth_token(mut self, auth_token: impl Into<String>) -> Self {
        self.auth_token = Some(auth_token.into());
        self
    }

    /// Set the managed root directory
    pub fn with_managed_root(mut self, managed_root: impl Into<PathBuf>) -> Self {
        self.managed_root = managed_root.into();
        self
    }

    /// Set the cache root directory
    pub fn with_cache_root(mut self, cache_root: impl Into<PathBuf>) -> Self {
        self.cache_root = cache_root.into();
        self
    }

    fn env_auth_token() -> Option<String> {
        std::env::var("MOFACLAW_HUB_TOKEN")
            .ok()
            .or_else(|| std::env::var("CLAWHUB_AUTH_TOKEN").ok())
            .or_else(|| std::env::var("CLAWHUB_API_KEY").ok())
    }
}

/// Client for the OpenClaw Skills Hub
#[derive(Debug, Clone)]
pub struct SkillHubClient {
    client: reqwest::Client,
    config: SkillHubClientConfig,
}

impl SkillHubClient {
    /// Create a new hub client
    pub fn new(config: SkillHubClientConfig) -> Result<Self> {
        fs::create_dir_all(&config.managed_root).with_context(|| {
            format!(
                "failed to create managed root {}",
                config.managed_root.display()
            )
        })?;
        fs::create_dir_all(&config.cache_root).with_context(|| {
            format!(
                "failed to create cache root {}",
                config.cache_root.display()
            )
        })?;
        fs::create_dir_all(registry_dir(&config)).with_context(|| {
            format!(
                "failed to create registry dir {}",
                registry_dir(&config).display()
            )
        })?;

        let mut headers = HeaderMap::new();
        if let Some(token) = &config.auth_token {
            let value = HeaderValue::from_str(&format!("Bearer {}", token))
                .context("invalid hub auth token")?;
            headers.insert(AUTHORIZATION, value);
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self { client, config })
    }

    /// Get the client configuration
    pub fn config(&self) -> &SkillHubClientConfig {
        &self.config
    }

    /// Search the hub catalog
    pub async fn search(&self, query: &str) -> Result<Vec<HubSkillCatalogEntry>> {
        let query = query.trim().to_lowercase();
        let entries = self
            .sync_catalog()
            .await?
            .into_iter()
            .filter(|entry| query.is_empty() || entry.matches(&query))
            .collect();
        Ok(entries)
    }

    /// Get available categories from the catalog
    pub async fn categories(&self) -> Result<Vec<String>> {
        let mut categories = BTreeSet::new();
        for entry in self.sync_catalog().await? {
            for category in entry.categories() {
                categories.insert(category);
            }
        }
        Ok(categories.into_iter().collect())
    }

    /// Get details for a specific skill
    pub async fn skill_details(&self, name: &str) -> Result<Option<HubSkillCatalogEntry>> {
        Ok(self
            .sync_catalog()
            .await?
            .into_iter()
            .find(|entry| entry.name == name))
    }

    /// Install a skill from the hub
    pub async fn install(
        &self,
        name: &str,
        version: Option<&str>,
    ) -> Result<ManagedHubSkillRecord> {
        validate_skill_name(name)?;
        let entry = self
            .skill_details(name)
            .await?
            .with_context(|| format!("skill '{}' not found in hub", name))?;

        let download_url = entry
            .download_url_for(version)
            .with_context(|| format!("no download URL for skill {}", name))?;

        // Download bundle
        let response = self
            .client
            .get(&download_url)
            .send()
            .await
            .with_context(|| format!("failed to download skill from {}", download_url))?;
        let response = response
            .error_for_status()
            .with_context(|| format!("hub download failed: {}", download_url))?;
        let bundle: HubSkillBundle = response.json().await.context("failed to decode bundle")?;

        // Install files
        let installed = install_skill_bundle(
            &self.config.managed_root,
            &entry.name,
            &bundle,
            Some(&download_url),
        )?;

        let now = now_epoch_secs();
        let record = ManagedHubSkillRecord {
            name: entry.name.clone(),
            description: entry.description.clone(),
            version: installed.version.or_else(|| entry.latest_version.clone()),
            source: "hub".to_string(),
            source_url: installed.source_url.clone(),
            installed_path: installed.install_dir,
            catalog_url: self.config.catalog_url.clone(),
            categories: entry.categories(),
            compatibility: entry.compatibility.clone(),
            rating: entry.rating,
            installed_at_epoch_secs: now,
            updated_at_epoch_secs: None,
            last_used_at_epoch_secs: Some(now),
        };

        self.save_record(&record)?;
        Ok(record)
    }

    /// List all installed hub skills
    pub fn list_installed(&self) -> Result<Vec<ManagedHubSkillRecord>> {
        let mut records = Vec::new();
        let dir = registry_dir(&self.config);
        if !dir.exists() {
            return Ok(records);
        }

        for entry in fs::read_dir(&dir)
            .with_context(|| format!("failed to read registry: {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }

            let bytes =
                fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
            let record: ManagedHubSkillRecord =
                serde_json::from_slice(&bytes).context("failed to decode record")?;
            records.push(record);
        }

        records.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(records)
    }

    /// Get a specific installed skill
    pub fn get_installed(&self, name: &str) -> Result<Option<ManagedHubSkillRecord>> {
        validate_skill_name(name)?;
        let path = record_path(&self.config, name);
        if !path.exists() {
            return Ok(None);
        }

        let bytes =
            fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
        let record: ManagedHubSkillRecord =
            serde_json::from_slice(&bytes).context("failed to decode")?;
        Ok(Some(record))
    }

    /// Remove an installed skill
    pub fn remove(&self, name: &str) -> Result<bool> {
        validate_skill_name(name)?;
        let mut removed = false;
        let skill_dir = self.config.managed_root.join(name);
        if skill_dir.exists() {
            fs::remove_dir_all(&skill_dir)
                .with_context(|| format!("failed to remove {}", skill_dir.display()))?;
            removed = true;
        }

        let record_path = record_path(&self.config, name);
        if record_path.exists() {
            fs::remove_file(&record_path)?;
            removed = true;
        }

        Ok(removed)
    }

    fn catalog_cache_path(&self) -> PathBuf {
        self.config.cache_root.join(CATALOG_CACHE_FILE)
    }

    async fn fetch_catalog_remote(&self) -> Result<Vec<HubSkillCatalogEntry>> {
        let response = self
            .client
            .get(&self.config.catalog_url)
            .send()
            .await
            .with_context(|| format!("failed to fetch catalog from {}", self.config.catalog_url))?;
        let response = response.error_for_status()?;
        let entries: Vec<HubSkillCatalogEntry> = response.json().await?;
        Ok(entries)
    }

    async fn sync_catalog(&self) -> Result<Vec<HubSkillCatalogEntry>> {
        match self.fetch_catalog_remote().await {
            Ok(entries) => {
                self.write_catalog_cache(&entries)?;
                Ok(entries)
            }
            Err(_) => self.load_cached_catalog(),
        }
    }

    fn load_cached_catalog(&self) -> Result<Vec<HubSkillCatalogEntry>> {
        let cache_path = self.catalog_cache_path();
        let bytes = fs::read(&cache_path)
            .with_context(|| format!("failed to read cache {}", cache_path.display()))?;
        let cache: HubCatalogCache =
            serde_json::from_slice(&bytes).context("failed to decode cache")?;
        Ok(cache.entries)
    }

    fn write_catalog_cache(&self, entries: &[HubSkillCatalogEntry]) -> Result<()> {
        let cache = HubCatalogCache {
            catalog_url: self.config.catalog_url.clone(),
            fetched_at_epoch_secs: now_epoch_secs(),
            entries: entries.to_vec(),
        };
        let bytes = serde_json::to_vec(&cache)?;
        let cache_path = self.catalog_cache_path();
        fs::write(&cache_path, bytes)
            .with_context(|| format!("failed to write cache {}", cache_path.display()))?;
        Ok(())
    }

    fn save_record(&self, record: &ManagedHubSkillRecord) -> Result<()> {
        let path = record_path(&self.config, &record.name);
        let bytes = serde_json::to_vec(record)?;
        fs::write(&path, bytes)
            .with_context(|| format!("failed to write record {}", path.display()))?;
        Ok(())
    }
}

/// Install a skill bundle to the filesystem
fn install_skill_bundle(
    root: &Path,
    name: &str,
    bundle: &HubSkillBundle,
    source_url: Option<&str>,
) -> Result<InstalledHubSkill> {
    validate_skill_name(name)?;
    fs::create_dir_all(root).with_context(|| format!("failed to create dir {}", root.display()))?;

    let install_dir = root.join(name);
    let staging_dir = unique_install_state_dir(root, name, "tmp");
    fs::create_dir_all(&staging_dir)
        .with_context(|| format!("failed to create dir {}", staging_dir.display()))?;

    let write_result: Result<()> = (|| {
        for file in &bundle.files {
            let file_path = validate_bundle_file_path(&staging_dir, &file.path)?;
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create dir {}", parent.display()))?;
            }
            fs::write(&file_path, &file.content)
                .with_context(|| format!("failed to write {}", file_path.display()))?;
        }
        Ok(())
    })();

    if let Err(err) = write_result {
        cleanup_install_state(&staging_dir);
        return Err(err);
    }

    let mut backup_dir = None;
    if install_dir.exists() {
        let candidate = unique_install_state_dir(root, name, "bak");
        fs::rename(&install_dir, &candidate).with_context(|| {
            format!(
                "failed to move existing install {} to {}",
                install_dir.display(),
                candidate.display()
            )
        })?;
        backup_dir = Some(candidate);
    }

    if let Err(err) = fs::rename(&staging_dir, &install_dir)
        .with_context(|| format!("failed to activate install {}", install_dir.display()))
    {
        cleanup_install_state(&staging_dir);
        if let Some(backup_dir) = &backup_dir {
            let _ = fs::rename(backup_dir, &install_dir);
        }
        return Err(err);
    }

    if let Some(backup_dir) = backup_dir {
        cleanup_install_state(&backup_dir);
    }

    Ok(InstalledHubSkill {
        name: name.to_string(),
        version: bundle.version.clone(),
        install_dir,
        source_url: source_url.map(String::from),
    })
}

fn validate_skill_name(name: &str) -> Result<()> {
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        bail!("invalid skill name: {}", name);
    }
    Ok(())
}

fn validate_bundle_file_path(install_dir: &Path, relative_path: &str) -> Result<PathBuf> {
    let path = Path::new(relative_path);
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("invalid bundle path: {}", relative_path);
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        bail!("invalid bundle path: {}", relative_path);
    }

    Ok(install_dir.join(normalized))
}

fn unique_install_state_dir(root: &Path, name: &str, suffix: &str) -> PathBuf {
    root.join(format!(".{}.{}.{}", name, suffix, now_epoch_nanos()))
}

fn cleanup_install_state(path: &Path) {
    if path.exists() {
        let _ = fs::remove_dir_all(path);
    }
}

fn registry_dir(config: &SkillHubClientConfig) -> PathBuf {
    config.cache_root.join(REGISTRY_DIR)
}

fn record_path(config: &SkillHubClientConfig, name: &str) -> PathBuf {
    registry_dir(config).join(format!("{}.json", name))
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn now_epoch_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn install_skill_bundle_rejects_parent_traversal_paths() {
        let root = TempDir::new().unwrap();
        let bundle = HubSkillBundle {
            version: Some("1.0.0".to_string()),
            files: vec![HubSkillBundleFile {
                path: "../escape.txt".to_string(),
                content: "escape".to_string(),
            }],
        };

        let err = install_skill_bundle(root.path(), "demo-skill", &bundle, None).unwrap_err();

        assert!(
            err.to_string().contains("invalid bundle path"),
            "unexpected error: {err}"
        );
        assert!(!root.path().join("escape.txt").exists());
    }

    #[test]
    fn install_skill_bundle_cleans_up_partial_install_on_failure() {
        let root = TempDir::new().unwrap();
        let bundle = HubSkillBundle {
            version: Some("1.0.0".to_string()),
            files: vec![
                HubSkillBundleFile {
                    path: "SKILL.md".to_string(),
                    content: "---\nname: demo-skill\ndescription: partial install\n---\n".to_string(),
                },
                HubSkillBundleFile {
                    path: "../escape.txt".to_string(),
                    content: "escape".to_string(),
                },
            ],
        };

        let err = install_skill_bundle(root.path(), "demo-skill", &bundle, None).unwrap_err();

        assert!(
            err.to_string().contains("invalid bundle path"),
            "unexpected error: {err}"
        );
        assert!(
            !root.path().join("demo-skill").exists(),
            "partial install directory should be removed on failure"
        );
    }

    #[test]
    fn remove_rejects_path_traversal_name() {
        let cache_root = TempDir::new().unwrap();
        let managed_root = TempDir::new().unwrap();
        let config = SkillHubClientConfig::for_mofaclaw()
            .with_managed_root(managed_root.path())
            .with_cache_root(cache_root.path());
        let client = SkillHubClient::new(config).unwrap();

        let err = client.remove("../outside").unwrap_err();
        assert!(
            err.to_string().contains("invalid skill name"),
            "unexpected error: {err}"
        );
        assert!(
            !managed_root.path().join("outside").exists(),
            "path traversal should not remove files outside managed root"
        );
    }

    #[test]
    fn remove_returns_false_for_nonexistent_skill() {
        let cache_root = TempDir::new().unwrap();
        let managed_root = TempDir::new().unwrap();
        let config = SkillHubClientConfig::for_mofaclaw()
            .with_managed_root(managed_root.path())
            .with_cache_root(cache_root.path());
        let client = SkillHubClient::new(config).unwrap();

        let removed = client.remove("no-such-skill").unwrap();
        assert!(!removed, "expected false when skill does not exist");
    }
}

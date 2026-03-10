//! Integration tests for the skills hub module

#[cfg(test)]
mod skills_hub_tests {
    use mofaclaw_core::skills::{HubSkillCatalogEntry, SkillHubClient, SkillHubClientConfig};
    use mofaclaw_core::{load_config, Config, ContextBuilder};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::Path;
    use std::sync::{Mutex, OnceLock};
    use tempfile::TempDir;

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    struct HomeEnvGuard {
        previous: Option<std::ffi::OsString>,
    }

    impl HomeEnvGuard {
        fn set(path: &Path) -> Self {
            let previous = std::env::var_os("HOME");
            unsafe {
                std::env::set_var("HOME", path);
            }
            Self { previous }
        }
    }

    impl Drop for HomeEnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(previous) => unsafe {
                    std::env::set_var("HOME", previous);
                },
                None => unsafe {
                    std::env::remove_var("HOME");
                },
            }
        }
    }

    fn env_lock() -> &'static Mutex<()> {
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    fn spawn_test_hub_server(skill_name: &str, description: &str) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}", addr);
        let catalog_url = format!("{}/catalog", base_url);
        let bundle_url = format!("{}/bundle", base_url);

        let catalog_body = serde_json::to_string(&vec![HubSkillCatalogEntry {
            name: skill_name.to_string(),
            description: description.to_string(),
            author: Some("hub-test".to_string()),
            tags: vec!["test".to_string()],
            category: Some("utility".to_string()),
            categories: vec!["utility".to_string()],
            compatibility: vec!["mofaclaw".to_string()],
            latest_version: Some("1.0.0".to_string()),
            download_url: Some(bundle_url.clone()),
            versions: vec![],
            rating: Some(5.0),
            downloads: Some(1),
        }])
        .unwrap();

        let bundle_body = serde_json::json!({
            "version": "1.0.0",
            "files": [
                {
                    "path": "SKILL.md",
                    "content": format!(
                        "---\nname: {skill_name}\ndescription: {description}\n---\n\n# {skill_name}\n\nUse this installed skill.\n"
                    )
                }
            ]
        })
        .to_string();

        let handle = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buffer = [0_u8; 4096];
                let bytes_read = stream.read(&mut buffer).unwrap();
                let request = String::from_utf8_lossy(&buffer[..bytes_read]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/");

                let body = match path {
                    "/catalog" => &catalog_body,
                    "/bundle" => &bundle_body,
                    _ => "{\"error\":\"not found\"}",
                };
                let status = if matches!(path, "/catalog" | "/bundle") {
                    "200 OK"
                } else {
                    "404 Not Found"
                };

                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).unwrap();
                stream.flush().unwrap();
            }
        });

        (catalog_url, handle)
    }

    #[test]
    fn test_default_mofaclaw_paths() {
        let config = SkillHubClientConfig::for_mofaclaw();

        // Check that managed root uses mofaclaw paths
        let managed_path = config.managed_root.to_string_lossy();
        assert!(
            managed_path.contains(".mofaclaw"),
            "managed_root should contain .mofaclaw: {}",
            managed_path
        );
        assert!(
            managed_path.contains("skills"),
            "managed_root should contain 'skills'"
        );
        assert!(
            managed_path.contains("hub"),
            "managed_root should contain 'hub'"
        );

        // Check that cache root uses mofaclaw paths
        let cache_path = config.cache_root.to_string_lossy();
        assert!(
            cache_path.contains(".mofaclaw"),
            "cache_root should contain .mofaclaw: {}",
            cache_path
        );
        assert!(
            cache_path.contains("cache"),
            "cache_root should contain 'cache'"
        );
        assert!(
            cache_path.contains("hub"),
            "cache_root should contain 'hub'"
        );
    }

    #[test]
    fn test_default_hub_url() {
        let config = SkillHubClientConfig::for_mofaclaw();
        assert_eq!(config.catalog_url, "https://clawhub.run/api/skills/catalog");
    }

    #[test]
    fn test_config_builder() {
        let config =
            SkillHubClientConfig::for_mofaclaw().with_catalog_url("https://custom-hub.example.com/api/catalog");

        assert_eq!(
            config.catalog_url,
            "https://custom-hub.example.com/api/catalog"
        );
    }

    #[test]
    fn test_parse_skill_version() {
        // Test with version
        let input = "my-skill@1.0.0";
        if let Some(at_index) = input.find('@') {
            let name = &input[..at_index];
            let version = &input[at_index + 1..];
            assert_eq!(name, "my-skill");
            assert_eq!(version, "1.0.0");
        } else {
            panic!("Should find @ in version string");
        }

        // Test without version
        let input = "my-skill";
        assert!(input.find('@').is_none());
    }

    #[test]
    fn test_config_builder_reads_root_skills_settings() {
        let mut root = Config::default();
        root.skills.hub_url = "https://example.com/api/catalog".to_string();

        let config = SkillHubClientConfig::from_config(&root);

        assert_eq!(config.catalog_url, "https://example.com/api/catalog");
    }

    #[test]
    fn test_skill_catalog_entry_matching() {
        let entry = HubSkillCatalogEntry {
            name: "weather-skill".to_string(),
            description: "Get weather information for any location".to_string(),
            author: Some("weatherbot".to_string()),
            tags: vec!["weather".to_string(), "info".to_string()],
            category: Some("data".to_string()),
            categories: vec!["data".to_string(), "external-api".to_string()],
            compatibility: vec!["universal".to_string()],
            latest_version: Some("1.0.0".to_string()),
            download_url: Some("https://hub.example.com/weather-skill/latest.json".to_string()),
            versions: vec![],
            rating: Some(4.5),
            downloads: Some(1000),
        };

        // Test name matching
        assert!(entry.name.to_lowercase().contains("weather"));

        // Test description matching
        assert!(entry.description.to_lowercase().contains("weather"));

        // Test tag matching
        assert!(
            entry
                .tags
                .iter()
                .any(|t| t.to_lowercase().contains("weather"))
        );
    }

    #[test]
    fn test_skill_config_with_custom_managed_root() {
        let temp_dir = TempDir::new().unwrap();
        let custom_path = temp_dir.path().join("custom-hub");
        let config = SkillHubClientConfig::for_mofaclaw().with_managed_root(&custom_path);

        assert_eq!(config.managed_root, custom_path);
    }

    #[test]
    fn test_skill_config_with_custom_cache_root() {
        let temp_dir = TempDir::new().unwrap();
        let custom_path = temp_dir.path().join("custom-cache");
        let config = SkillHubClientConfig::for_mofaclaw().with_cache_root(&custom_path);

        assert_eq!(config.cache_root, custom_path);
    }

    #[tokio::test]
    async fn test_context_builder_discovers_skill_installed_via_hub_client_in_runtime_prompt() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let home_dir = TempDir::new().unwrap();
        let workspace_dir = TempDir::new().unwrap();
        let _home_guard = HomeEnvGuard::set(home_dir.path());

        std::fs::create_dir_all(workspace_dir.path().join("skills")).unwrap();

        let mut config = Config::default();
        config.agents.defaults.workspace = workspace_dir.path().display().to_string();

        let builder = ContextBuilder::new(&config);
        let (catalog_url, server_handle) =
            spawn_test_hub_server("hub-demo", "Hub-installed skill for runtime discovery");
        let hub_client = SkillHubClient::new(
            SkillHubClientConfig::for_mofaclaw().with_catalog_url(catalog_url),
        )
        .unwrap();

        let record = hub_client.install("hub-demo", None).await.unwrap();
        assert!(record.installed_path.join("SKILL.md").exists());

        let prompt = builder.build_system_prompt(None).await.unwrap();

        server_handle.join().unwrap();

        assert!(
            prompt.contains("hub-demo"),
            "expected prompt to mention the newly installed hub skill, got: {prompt}"
        );
        assert!(
            prompt.contains("Hub-installed skill for runtime discovery"),
            "expected prompt to advertise the installed hub skill in the runtime prompt, got: {prompt}"
        );
    }

    #[tokio::test]
    async fn test_load_config_rejects_removed_auto_install_key() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let home_dir = TempDir::new().unwrap();
        let _home_guard = HomeEnvGuard::set(home_dir.path());

        let config_dir = home_dir.path().join(".mofaclaw");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.json"),
            r#"{
  "skills": {
    "hub_url": "https://example.com/api/catalog",
    "auto_install": true
  }
}"#,
        )
        .unwrap();

        let err = load_config().await.unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("unknown field") && message.contains("auto_install"),
            "expected removed auto_install field to be rejected, got: {message}"
        );
    }

    #[tokio::test]
    async fn test_hub_client_remove_uninstalls_skill() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let home_dir = TempDir::new().unwrap();
        let _home_guard = HomeEnvGuard::set(home_dir.path());

        let (catalog_url, server_handle) =
            spawn_test_hub_server("removable-skill", "A skill to be removed");
        let hub_client = SkillHubClient::new(
            SkillHubClientConfig::for_mofaclaw().with_catalog_url(catalog_url),
        )
        .unwrap();

        // Install then remove
        let record = hub_client
            .install("removable-skill", None)
            .await
            .unwrap();
        assert!(record.installed_path.exists(), "skill should be installed");

        let removed = hub_client.remove("removable-skill").unwrap();
        assert!(removed, "remove should return true for an installed skill");
        assert!(
            !record.installed_path.exists(),
            "skill directory should be deleted after remove"
        );
        assert!(
            hub_client.get_installed("removable-skill").unwrap().is_none(),
            "registry record should be deleted after remove"
        );

        server_handle.join().unwrap();
    }

}

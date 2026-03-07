//! Integration tests for the skills hub module

#[cfg(test)]
mod skills_hub_tests {
    use mofaclaw_core::skills::{SkillHubClientConfig, HubSkillCatalogEntry};
    use std::path::PathBuf;

    #[test]
    fn test_default_mofaclaw_paths() {
        let config = SkillHubClientConfig::for_mofaclaw();

        // Check that managed root uses mofaclaw paths
        let managed_path = config.managed_root.to_string_lossy();
        assert!(managed_path.contains(".mofaclaw"), "managed_root should contain .mofaclaw: {}", managed_path);
        assert!(managed_path.contains("skills"), "managed_root should contain 'skills'");
        assert!(managed_path.contains("hub"), "managed_root should contain 'hub'");

        // Check that cache root uses mofaclaw paths
        let cache_path = config.cache_root.to_string_lossy();
        assert!(cache_path.contains(".mofaclaw"), "cache_root should contain .mofaclaw: {}", cache_path);
        assert!(cache_path.contains("cache"), "cache_root should contain 'cache'");
        assert!(cache_path.contains("hub"), "cache_root should contain 'hub'");
    }

    #[test]
    fn test_default_hub_url() {
        let config = SkillHubClientConfig::for_mofaclaw();
        assert_eq!(config.catalog_url, "https://clawhub.run/api/skills/catalog");
    }

    #[test]
    fn test_config_builder() {
        let config = SkillHubClientConfig::for_mofaclaw()
            .with_catalog_url("https://custom-hub.example.com/api/catalog")
            .with_auto_install(true);

        assert_eq!(config.catalog_url, "https://custom-hub.example.com/api/catalog");
        assert!(config.auto_install_on_miss);
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
        assert!(entry.tags.iter().any(|t| t.to_lowercase().contains("weather")));
    }

    #[test]
    fn test_skill_config_with_custom_managed_root() {
        let custom_path = PathBuf::from("/tmp/custom-hub");
        let config = SkillHubClientConfig::for_mofaclaw()
            .with_managed_root(&custom_path);

        assert_eq!(config.managed_root, custom_path);
    }

    #[test]
    fn test_skill_config_with_custom_cache_root() {
        let custom_path = PathBuf::from("/tmp/custom-cache");
        let config = SkillHubClientConfig::for_mofaclaw()
            .with_cache_root(&custom_path);

        assert_eq!(config.cache_root, custom_path);
    }
}

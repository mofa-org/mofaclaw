//! Path matching for filesystem access control

use glob::Pattern;
use std::path::{Path, PathBuf};
use tracing::warn;

/// Matcher for path whitelist/blacklist patterns
pub struct PathMatcher {
    workspace: PathBuf,
    home: PathBuf,
}

impl PathMatcher {
    /// Create a new path matcher
    pub fn new(workspace: PathBuf, home: PathBuf) -> Self {
        Self { workspace, home }
    }

    /// Expand variables in pattern (${workspace}, ${home})
    pub fn expand_variables(&self, pattern: &str) -> String {
        pattern
            .replace("${workspace}", self.workspace.to_string_lossy().as_ref())
            .replace("${home}", self.home.to_string_lossy().as_ref())
    }

    /// Check if a path matches any of the given glob patterns
    pub fn matches(&self, path: &Path, patterns: &[String]) -> bool {
        let path_str = path.to_string_lossy();
        let normalized_path = self.normalize_path(path);

        for pattern in patterns {
            let expanded = self.expand_variables(pattern);
            match Pattern::new(&expanded) {
                Ok(pat) => {
                    if pat.matches(&normalized_path) || pat.matches(path_str.as_ref()) {
                        return true;
                    }
                }
                Err(e) => {
                    warn!("Invalid glob pattern '{}': {}", pattern, e);
                }
            }
        }
        false
    }

    /// Normalize path for matching (resolve symlinks, canonicalize)
    fn normalize_path(&self, path: &Path) -> String {
        // Try to canonicalize, but fall back to original if it fails
        path.canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .to_string()
    }

    /// Check if path is in whitelist
    pub fn is_whitelisted(&self, path: &Path, whitelist: &[String]) -> bool {
        if whitelist.is_empty() {
            return false;
        }
        self.matches(path, whitelist)
    }

    /// Check if path is in blacklist
    pub fn is_blacklisted(&self, path: &Path, blacklist: &[String]) -> bool {
        if blacklist.is_empty() {
            return false;
        }
        self.matches(path, blacklist)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_test_matcher() -> PathMatcher {
        PathMatcher::new(
            PathBuf::from("/workspace"),
            PathBuf::from("/home/user"),
        )
    }

    #[test]
    fn test_expand_variables() {
        let matcher = create_test_matcher();
        assert_eq!(
            matcher.expand_variables("${workspace}/test"),
            "/workspace/test"
        );
        assert_eq!(
            matcher.expand_variables("${home}/projects"),
            "/home/user/projects"
        );
    }

    #[test]
    fn test_matches_glob_pattern() {
        let matcher = create_test_matcher();
        let path = PathBuf::from("/workspace/test.txt");

        assert!(matcher.matches(&path, &["${workspace}/**".to_string()]));
        assert!(matcher.matches(&path, &["**/*.txt".to_string()]));
        assert!(!matcher.matches(&path, &["${workspace}/src/**".to_string()]));
    }

    #[test]
    fn test_whitelist_blacklist() {
        let matcher = create_test_matcher();
        let path = PathBuf::from("/workspace/test.txt");

        assert!(matcher.is_whitelisted(&path, &["${workspace}/**".to_string()]));
        assert!(!matcher.is_whitelisted(&path, &["/other/**".to_string()]));

        // If a path matches a blacklist pattern, it should be blacklisted
        assert!(matcher.is_blacklisted(&path, &["${workspace}/**".to_string()]));
        assert!(matcher.is_blacklisted(&path, &["**/*.txt".to_string()]));
        // Path that doesn't match blacklist should not be blacklisted
        assert!(!matcher.is_blacklisted(&path, &["/other/**".to_string()]));
    }
}

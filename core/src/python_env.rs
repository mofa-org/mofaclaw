//! Python environment checker and dependency installer
//!
//! This module provides functionality to:
//! - Check if Python is installed
//! - Verify Python version
//! - Check if required packages are installed
//! - Automatically install missing packages

use crate::error::{ChannelError, Result};
use std::process::Stdio;
use tokio::process::Command;
use tracing::{info, warn};

/// Python environment checker and dependency installer
pub struct PythonEnv {
    /// The Python command to use (python3 or python)
    python_cmd: String,
    /// The major version
    major: u8,
    /// The minor version
    minor: u8,
}

impl PythonEnv {
    /// Minimum required Python version
    const MIN_MAJOR: u8 = 3;
    const MIN_MINOR: u8 = 11;

    /// Find available Python command and check version
    pub async fn find() -> Result<Self> {
        // Try python3 first, then python
        for cmd in &["python3", "python"] {
            match Self::check_version(cmd).await {
                Ok((major, minor)) => {
                    info!("Found Python: {} (version {}.{}), command: {}", cmd, major, minor, cmd);
                    return Ok(Self {
                        python_cmd: cmd.to_string(),
                        major,
                        minor,
                    });
                }
                Err(e) => {
                    warn!("Failed to use '{}': {}", cmd, e);
                }
            }
        }

        // Neither python3 nor python worked
        Err(ChannelError::ConnectionFailed(
            "Python not found. Please install Python 3.11 or higher.\n\n \
             Download from: https://www.python.org/downloads/\n\n \
             After installation, verify with: python3 --version"
                .to_string(),
        )
        .into())
    }

    /// Get the Python command
    pub fn command(&self) -> &str {
        &self.python_cmd
    }

    /// Get version as string
    pub fn version_string(&self) -> String {
        format!("{}.{}", self.major, self.minor)
    }

    /// Check if the Python version is sufficient
    pub fn is_version_sufficient(&self) -> bool {
        self.major > Self::MIN_MAJOR
            || (self.major == Self::MIN_MAJOR && self.minor >= Self::MIN_MINOR)
    }

    /// Check Python version by running the command
    async fn check_version(cmd: &str) -> Result<(u8, u8)> {
        let output = Command::new(cmd)
            .arg("--version")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        match output {
            Ok(output) => {
                // Python outputs version to stderr for some reason
                let version_output = if output.stdout.is_empty() {
                    String::from_utf8_lossy(&output.stderr).to_string()
                } else {
                    String::from_utf8_lossy(&output.stdout).to_string()
                };

                // Parse "Python 3.11.0" or similar
                if let Some(rest) = version_output.strip_prefix("Python ") {
                    let version: String = rest
                        .chars()
                        .take_while(|c| c.is_ascii_digit() || *c == '.')
                        .collect();
                    let parts: Vec<&str> = version.split('.').collect();
                    if parts.len() >= 2 {
                        let major = parts[0].parse::<u8>().unwrap_or(0);
                        let minor = parts[1].parse::<u8>().unwrap_or(0);
                        return Ok((major, minor));
                    }
                }

                Err(ChannelError::ConnectionFailed(format!(
                    "Could not parse Python version output: {}",
                    version_output.trim()
                ))
                .into())
            }
            Err(e) => Err(ChannelError::ConnectionFailed(format!(
                "Failed to run Python command '{}': {}",
                cmd, e
            ))
            .into()),
        }
    }

    /// Verify Python version and return error if too old
    pub fn verify_version(&self) -> Result<()> {
        if !self.is_version_sufficient() {
            return Err(ChannelError::ConnectionFailed(format!(
                "Python version {} is too old. mofaclaw requires Python {}.{} or higher.\n\n \
                 Please upgrade Python: https://www.python.org/downloads/",
                self.version_string(),
                Self::MIN_MAJOR,
                Self::MIN_MINOR
            ))
            .into());
        }
        Ok(())
    }

    /// Check if a package is installed
    pub async fn is_package_installed(&self, package: &str) -> Result<bool> {
        let output = Command::new(&self.python_cmd)
            .args(["-c", &format!("import {}", package)])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        match output {
            Ok(output) => Ok(output.status.success()),
            Err(_) => Ok(false),
        }
    }

    /// Install a package using pip
    pub async fn install_package(&self, package: &str) -> Result<()> {
        info!("Installing Python package: {}...", package);

        // Use --user flag to avoid permission issues
        let args = ["-m", "pip", "install", "--user", "--upgrade", package];

        let output = Command::new(&self.python_cmd)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        match output {
            Ok(output) => {
                if output.status.success() {
                    info!("Successfully installed: {}", package);
                    Ok(())
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    Err(ChannelError::ConnectionFailed(format!(
                        "Failed to install '{}':\nstdout: {}\nstderr: {}",
                        package, stdout, stderr
                    ))
                    .into())
                }
            }
            Err(e) => Err(ChannelError::ConnectionFailed(format!(
                "Failed to run pip install for '{}': {}",
                package, e
            ))
            .into()),
        }
    }

    /// Check and install a package if missing
    pub async fn ensure_package(&self, package: &str) -> Result<()> {
        if !self.is_package_installed(package).await? {
            info!("Package '{}' is not installed. Installing now...", package);
            self.install_package(package).await?;
        } else {
            info!("Package '{}' is already installed.", package);
        }
        Ok(())
    }

    /// Check and install multiple packages
    pub async fn ensure_packages(&self, packages: &[&str]) -> Result<()> {
        info!("Checking Python dependencies...");
        for package in packages {
            self.ensure_package(package).await?;
        }
        info!("All Python dependencies are satisfied.");
        Ok(())
    }

    /// Get a friendly help message for Python installation
    pub fn installation_help() -> &'static str {
        r#"
Python Installation Help
========================

To install Python on your system:

1. Download Python from: https://www.python.org/downloads/
2. Install Python 3.11 or higher
3. Verify installation: python3 --version

If Python is already installed but not found:
- Make sure Python is in your PATH
- On some systems, use 'python' instead of 'python3'
- On Windows, use the Python Launcher: py

For Docker/containers:
- apt-get update && apt-get install -y python3 python3-pip
- apk add python3 py3-pip  (Alpine)
"#
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // This test requires Python to be installed
    async fn test_find_python() {
        match PythonEnv::find().await {
            Ok(env) => {
                println!("Found Python: {}", env.version_string());
                assert!(env.is_version_sufficient());
            }
            Err(e) => {
                println!("Python not found (expected in CI): {}", e);
            }
        }
    }
}

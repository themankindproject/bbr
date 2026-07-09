//! Stacked PRs configuration `.bbr/stack.toml`.

use crate::error::{BitbucketError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StackConfig {
    #[serde(default)]
    pub stacks: Vec<StackDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackDef {
    pub name: String,
    pub base_branch: String,
    #[serde(default)]
    pub prs: Vec<StackPr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackPr {
    pub branch: String,
    pub pr_id: Option<u64>,
    pub parent_branch: String,
}

impl StackConfig {
    pub fn config_path() -> PathBuf {
        // Use a short timeout for git rev-parse (read-only, fast)
        use std::process::Command;

        let result = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output();

        if let Ok(output) = result {
            if output.status.success() {
                let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !root.is_empty() {
                    return PathBuf::from(root).join(".bbr").join("stack.toml");
                }
            }
        }
        PathBuf::from(".bbr").join("stack.toml")
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(StackConfig::default());
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|e| BitbucketError::Other(format!("failed to read stack config: {}", e)))?;
        let config: StackConfig = toml::from_str(&content)
            .map_err(|e| BitbucketError::Other(format!("failed to parse stack config: {}", e)))?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let content = toml::to_string_pretty(self).map_err(|e| {
            BitbucketError::Other(format!("failed to serialize stack config: {}", e))
        })?;
        std::fs::write(&path, content)
            .map_err(|e| BitbucketError::Other(format!("failed to write stack config: {}", e)))?;
        Ok(())
    }

    pub fn find_stack(&self, name: &str) -> Option<&StackDef> {
        self.stacks.iter().find(|s| s.name == name)
    }

    pub fn find_stack_mut(&mut self, name: &str) -> Option<&mut StackDef> {
        self.stacks.iter_mut().find(|s| s.name == name)
    }

    pub fn active_stack(&self) -> Result<&StackDef> {
        if self.stacks.is_empty() {
            return Err(BitbucketError::Other(
                "No stacks initialized. Run `bbr pr stack init <name>` first.".into(),
            ));
        }
        // For simplicity, treat the first stack as active, or search if we want to store active stack
        Ok(&self.stacks[0])
    }

    pub fn active_stack_mut(&mut self) -> Result<&mut StackDef> {
        if self.stacks.is_empty() {
            return Err(BitbucketError::Other(
                "No stacks initialized. Run `bbr pr stack init <name>` first.".into(),
            ));
        }
        Ok(&mut self.stacks[0])
    }
}

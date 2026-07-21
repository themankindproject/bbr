//! Stacked PRs configuration `.bbr/stack.toml`.

use crate::error::{BitbucketError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StackConfig {
    /// Name of the active stack (used by add/list/rebase/land/abort).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
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

    fn active_index(&self) -> Result<usize> {
        if self.stacks.is_empty() {
            return Err(BitbucketError::Other(
                "No stacks initialized. Run `bbr pr stack init <name>` first.".into(),
            ));
        }
        if let Some(name) = self.active.as_deref() {
            if let Some(i) = self.stacks.iter().position(|s| s.name == name) {
                return Ok(i);
            }
        }
        // Missing/stale `active` → first stack (legacy configs).
        Ok(0)
    }

    /// Select which stack subsequent commands operate on.
    pub fn set_active(&mut self, name: &str) -> Result<()> {
        if self.find_stack(name).is_none() {
            return Err(BitbucketError::Other(format!(
                "Stack '{name}' not found. Run `bbr pr stack list` to see available stacks."
            )));
        }
        self.active = Some(name.to_string());
        Ok(())
    }

    pub fn active_stack(&self) -> Result<&StackDef> {
        let i = self.active_index()?;
        Ok(&self.stacks[i])
    }

    pub fn active_stack_mut(&mut self) -> Result<&mut StackDef> {
        let i = self.active_index()?;
        Ok(&mut self.stacks[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(stacks: &[&str], active: Option<&str>) -> StackConfig {
        StackConfig {
            active: active.map(str::to_string),
            stacks: stacks
                .iter()
                .map(|n| StackDef {
                    name: (*n).to_string(),
                    base_branch: "main".into(),
                    prs: vec![],
                })
                .collect(),
        }
    }

    #[test]
    fn active_stack_uses_named_selection() {
        let c = cfg(&["a", "b"], Some("b"));
        assert_eq!(c.active_stack().unwrap().name, "b");
    }

    #[test]
    fn active_stack_falls_back_to_first() {
        let c = cfg(&["a", "b"], None);
        assert_eq!(c.active_stack().unwrap().name, "a");
    }

    #[test]
    fn active_stack_falls_back_when_name_stale() {
        let c = cfg(&["a", "b"], Some("gone"));
        assert_eq!(c.active_stack().unwrap().name, "a");
    }

    #[test]
    fn set_active_rejects_unknown() {
        let mut c = cfg(&["a"], None);
        assert!(c.set_active("missing").is_err());
    }

    #[test]
    fn set_active_updates_field() {
        let mut c = cfg(&["a", "b"], Some("a"));
        c.set_active("b").unwrap();
        assert_eq!(c.active.as_deref(), Some("b"));
        assert_eq!(c.active_stack().unwrap().name, "b");
    }
}

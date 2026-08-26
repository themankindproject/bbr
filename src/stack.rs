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
        // The repo root doesn't move during a process — cache the resolved
        // path so repeated load()/save() calls don't re-shell out to git.
        static CACHED_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        if let Some(p) = CACHED_PATH.get() {
            return p.clone();
        }

        // Route through the shared git runner: it enforces the read timeout
        // and drains pipes, so a wedged git can't hang stack commands.
        let path = match crate::git::repo_toplevel() {
            Some(root) => PathBuf::from(root).join(".bbr").join("stack.toml"),
            None => PathBuf::from(".bbr").join("stack.toml"),
        };
        let _ = CACHED_PATH.set(path.clone());
        path
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

/// Apply the outcome of a `stack land` run to the config (pure, testable).
///
/// On full success only the landed stack is removed — other stacks defined
/// in the same file must survive. On partial failure the stack is kept with
/// its remaining unmerged PRs so the user can resume.
pub fn apply_land_result(
    config: &mut StackConfig,
    stack_name: &str,
    merged: &[u64],
    had_failure: bool,
) {
    if !had_failure {
        config.stacks.retain(|s| s.name != stack_name);
        if config.active.as_deref() == Some(stack_name) {
            config.active = None;
        }
    } else if let Some(s) = config.find_stack_mut(stack_name) {
        s.prs.retain(|p| !merged.contains(&p.pr_id.unwrap_or(0)));
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

    #[test]
    fn active_roundtrips_in_toml() {
        let c = cfg(&["a", "b"], Some("b"));
        let toml = toml::to_string_pretty(&c).unwrap();
        assert!(toml.contains("active = \"b\""));
        let parsed: StackConfig = toml::from_str(&toml).unwrap();
        assert_eq!(parsed.active.as_deref(), Some("b"));
        assert_eq!(parsed.active_stack().unwrap().name, "b");
    }

    #[test]
    fn missing_active_deserializes_as_none() {
        let parsed: StackConfig = toml::from_str(
            r#"
[[stacks]]
name = "a"
base_branch = "main"
"#,
        )
        .unwrap();
        assert!(parsed.active.is_none());
        assert_eq!(parsed.active_stack().unwrap().name, "a");
    }

    fn pr(branch: &str, id: Option<u64>, parent: &str) -> StackPr {
        StackPr {
            branch: branch.to_string(),
            pr_id: id,
            parent_branch: parent.to_string(),
        }
    }

    fn cfg_with_prs() -> StackConfig {
        StackConfig {
            active: Some("s1".into()),
            stacks: vec![
                StackDef {
                    name: "s1".into(),
                    base_branch: "main".into(),
                    prs: vec![pr("b1", Some(101), "main"), pr("b2", Some(102), "b1")],
                },
                StackDef {
                    name: "s2".into(),
                    base_branch: "main".into(),
                    prs: vec![pr("c1", Some(201), "main")],
                },
            ],
        }
    }

    #[test]
    fn land_success_removes_only_landed_stack() {
        // Regression: landing one stack must not wipe sibling stacks.
        let mut c = cfg_with_prs();
        apply_land_result(&mut c, "s1", &[101, 102], false);
        assert_eq!(c.stacks.len(), 1);
        assert_eq!(c.stacks[0].name, "s2");
        assert_eq!(c.stacks[0].prs.len(), 1);
        assert_eq!(c.active, None);
    }

    #[test]
    fn land_success_keeps_active_when_other_stack_active() {
        let mut c = cfg_with_prs();
        c.active = Some("s2".into());
        apply_land_result(&mut c, "s1", &[101, 102], false);
        assert_eq!(c.active.as_deref(), Some("s2"));
        assert_eq!(c.stacks.len(), 1);
    }

    #[test]
    fn land_partial_failure_retains_unmerged_prs() {
        let mut c = cfg_with_prs();
        apply_land_result(&mut c, "s1", &[101], true);
        assert_eq!(c.stacks.len(), 2);
        let s1 = c.find_stack("s1").unwrap();
        assert_eq!(s1.prs.len(), 1);
        assert_eq!(s1.prs[0].pr_id, Some(102));
        assert_eq!(c.active.as_deref(), Some("s1"));
    }
}

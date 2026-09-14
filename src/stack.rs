//! Stacked PRs configuration `.bbr/stack.toml`.

use crate::error::{BitbucketError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StackConfig {
    /// Name of the active stack (used by add/list/rebase/land/abort).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(default)]
    pub stacks: Vec<StackDef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackDef {
    pub name: String,
    pub base_branch: String,
    #[serde(default)]
    pub prs: Vec<StackPr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(StackConfig::default()),
            Err(e) => {
                return Err(BitbucketError::Other(format!(
                    "failed to read stack config {}: {e}",
                    path.display()
                )))
            }
        };
        let config: StackConfig = toml::from_str(&content)
            .map_err(|e| BitbucketError::Other(format!("failed to parse stack config: {}", e)))?;
        Ok(config)
    }

    /// Replace the complete file atomically; never truncate live stack state.
    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::config_path())
    }

    fn save_to(&self, path: &std::path::Path) -> Result<()> {
        let content = toml::to_string_pretty(self).map_err(|e| {
            BitbucketError::Other(format!("failed to serialize stack config: {}", e))
        })?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                BitbucketError::Other(format!(
                    "failed to create stack config directory {}: {e}",
                    parent.display()
                ))
            })?;
        }
        crate::config::write_private(path, &content).map_err(|e| {
            BitbucketError::Other(format!(
                "failed to write stack config {}: {e}",
                path.display()
            ))
        })?;
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
            return self.stacks.iter().position(|s| s.name == name).ok_or_else(|| {
                BitbucketError::Other(format!(
                    "Active stack {name:?} does not exist. Run `bbr pr stack use <name>` to select an existing stack, or inspect .bbr/stack.toml."
                ))
            });
        }
        // Missing `active` preserves compatibility with legacy stack files.
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
    fn failed_save_preserves_destination_and_cleans_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stack.toml");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("keep"), "unchanged").unwrap();
        let error = cfg(&["a"], Some("a")).save_to(&path).unwrap_err();
        assert!(error.to_string().contains("write stack config"));
        assert_eq!(
            std::fs::read_to_string(path.join("keep")).unwrap(),
            "unchanged"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn failed_parent_creation_is_reported_directly() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("not-a-directory");
        std::fs::write(&parent, "unchanged").unwrap();
        let error = cfg(&["a"], Some("a"))
            .save_to(&parent.join("stack.toml"))
            .unwrap_err();
        assert!(error.to_string().contains("create stack config directory"));
        assert_eq!(std::fs::read_to_string(parent).unwrap(), "unchanged");
    }

    #[test]
    fn private_stack_save_roundtrips_and_replaces_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/stack.toml");
        cfg(&["a"], Some("a")).save_to(&path).unwrap();
        cfg(&["a", "b"], Some("b")).save_to(&path).unwrap();
        let saved: StackConfig = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(saved.stacks.len(), 2);
        assert_eq!(saved.active.as_deref(), Some("b"));
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
    fn active_stack_rejects_stale_selection() {
        let c = cfg(&["a", "b"], Some("gone"));
        assert!(c
            .active_stack()
            .unwrap_err()
            .to_string()
            .contains("bbr pr stack use"));
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

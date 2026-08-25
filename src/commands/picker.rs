//! Interactive disambiguation when a branch has multiple open PRs.
//!
//! When stdin and stdout are both TTYs, a numbered list is shown on stderr
//! and the user picks one. In any non-interactive context (pipes, scripts,
//! coding agents) selection silently falls back to the first PR — preserving
//! the machine-readable contract. `BBR_NO_INTERACTIVE=1` forces the
//! non-interactive path even on a TTY.

use std::io::Write;

use crate::api::pr::PullRequest;
use crate::error::{BitbucketError, Result};

/// True when an interactive choice may be attempted.
pub fn can_interact(stdin_is_tty: bool, stdout_is_tty: bool) -> bool {
    stdin_is_tty && stdout_is_tty && std::env::var_os("BBR_NO_INTERACTIVE").is_none()
}

/// One-line summary of a PR for the choice list.
fn describe(pr: &PullRequest) -> String {
    let title = crate::commands::truncate(&pr.title, 60);
    let dst = pr.destination_branch();
    let dst = if dst.is_empty() { "?" } else { dst };
    format!("#{} {} (→ {})", pr.id, title, dst)
}

/// Render the numbered choices block (stderr).
fn render_choices(prs: &[PullRequest]) -> String {
    let mut s = String::from("Multiple open PRs for this branch:\n");
    for (i, pr) in prs.iter().enumerate() {
        s.push_str(&format!("  [{}] {}\n", i + 1, describe(pr)));
    }
    s.push_str(&format!("Choose 1-{} [1]: ", prs.len()));
    s
}

/// Parse a user selection. Empty input defaults to 1.
fn parse_choice(input: &str, max: usize) -> Result<usize> {
    let trimmed = input.trim();
    let n: usize = if trimmed.is_empty() {
        1
    } else {
        trimmed
            .parse()
            .map_err(|_| BitbucketError::Other(format!("invalid choice: {trimmed}")))?
    };
    if n == 0 || n > max {
        return Err(BitbucketError::Other(format!(
            "choice out of range: {n} (valid: 1-{max})"
        )));
    }
    Ok(n - 1)
}

/// Pick a PR from candidates.
///
/// `input` abstracts the reading source so tests can drive it without a TTY:
/// pass `None` for production (reads stdin via the blocking reader).
pub fn pick(
    prs: &[PullRequest],
    interactive: bool,
    mut input: impl FnMut() -> std::io::Result<String>,
) -> Result<PullRequest> {
    let first = prs
        .first()
        .cloned()
        .ok_or_else(|| BitbucketError::NotFound("no open pull requests to choose from".into()))?;
    if !interactive || prs.len() == 1 {
        return Ok(first);
    }

    // Show choices on stderr so stdout stays clean for piped consumers.
    eprint!("{}", render_choices(prs));
    std::io::stderr().lock().flush().ok();

    let line = input().map_err(BitbucketError::Io)?;
    let idx = parse_choice(&line, prs.len())?;
    Ok(prs[idx].clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::pr::{BranchRef, Named};

    fn pr(id: u64, title: &str, dst: &str) -> PullRequest {
        PullRequest {
            id,
            title: title.into(),
            destination: BranchRef {
                branch: Some(Named { name: dst.into() }),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn can_interact_requires_both_ttys() {
        assert!(!can_interact(false, true));
        assert!(!can_interact(true, false));
        assert!(can_interact(true, true));
    }

    #[test]
    fn render_choices_numbers_each_pr() {
        let prs = vec![pr(42, "Fix bug", "main"), pr(43, "Add API", "main")];
        let rendered = render_choices(&prs);
        assert!(rendered.contains("[1] #42 Fix bug (→ main)"));
        assert!(rendered.contains("[2] #43 Add API (→ main)"));
        assert!(rendered.contains("Choose 1-2 [1]"));
    }

    #[test]
    fn parse_choice_defaults_and_validates() {
        assert_eq!(parse_choice("", 3).unwrap(), 0);
        assert_eq!(parse_choice("2", 3).unwrap(), 1);
        assert_eq!(parse_choice(" 3 ", 3).unwrap(), 2);
        assert!(parse_choice("4", 3).is_err());
        assert!(parse_choice("0", 3).is_err());
        assert!(parse_choice("abc", 3).is_err());
    }

    #[test]
    fn pick_non_interactive_returns_first() {
        let prs = vec![pr(42, "Fix bug", "main"), pr(43, "Add API", "main")];
        let picked = pick(&prs, false, || Ok(String::new())).unwrap();
        assert_eq!(picked.id, 42);
    }

    #[test]
    fn pick_interactive_uses_selection() {
        let prs = vec![pr(42, "Fix bug", "main"), pr(43, "Add API", "main")];
        let picked = pick(&prs, true, || Ok("2".into())).unwrap();
        assert_eq!(picked.id, 43);
    }

    #[test]
    fn pick_single_pr_skips_prompt_even_interactive() {
        let prs = vec![pr(42, "Fix bug", "main")];
        let picked = pick(&prs, true, || panic!("should not prompt")).unwrap();
        assert_eq!(picked.id, 42);
    }

    #[test]
    fn pick_empty_candidates_errors() {
        assert!(pick(&[], false, || panic!("no input expected")).is_err());
    }
}

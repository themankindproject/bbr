//! `bbr completion install` — auto-wire shell completions.

use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::PathBuf;

use clap::CommandFactory;
use clap_complete::{generate, Shell};

use crate::cli::Cli;
use crate::error::{BitbucketError, Result};

pub fn install(shell: Option<Shell>) -> Result<()> {
    let shell = shell.unwrap_or_else(detect_shell);

    let (dir, filename, rc_line) = shell_paths(&shell)?;

    fs::create_dir_all(&dir)
        .map_err(|e| BitbucketError::Other(format!("creating {dir:?}: {e}")))?;

    let comp_path = dir.join(&filename);
    let mut file = fs::File::create(&comp_path)
        .map_err(|e| BitbucketError::Other(format!("creating {comp_path:?}: {e}")))?;

    generate(shell, &mut Cli::command(), "bbr", &mut file);

    println!("Wrote completion script to: {}", comp_path.display());

    if let Some(line) = rc_line {
        let rc_path = dirs::home_dir()
            .ok_or_else(|| BitbucketError::Other("cannot determine $HOME".into()))?
            .join(line.0);
        let mut rc = fs::read_to_string(&rc_path).unwrap_or_default();
        if !rc.contains(&line.1) {
            writeln!(&mut rc, "\n{}", line.1)
                .map_err(|e| BitbucketError::Other(format!("writing {rc_path:?}: {e}")))?;
            fs::write(&rc_path, &rc)
                .map_err(|e| BitbucketError::Other(format!("writing {rc_path:?}: {e}")))?;
            println!("Added source line to: {}", rc_path.display());
        }
        println!("\nRestart your shell or run: source {}", rc_path.display());
    }

    Ok(())
}

fn detect_shell() -> Shell {
    let shell = std::env::var("SHELL").unwrap_or_default();
    if shell.ends_with("zsh") {
        Shell::Zsh
    } else if shell.ends_with("fish") {
        Shell::Fish
    } else if shell.ends_with("powershell") || shell.ends_with("pwsh") {
        Shell::PowerShell
    } else {
        Shell::Bash
    }
}

type RcLine = Option<(&'static str, String)>;

fn shell_paths(shell: &Shell) -> Result<(PathBuf, String, RcLine)> {
    let home =
        dirs::home_dir().ok_or_else(|| BitbucketError::Other("cannot determine $HOME".into()))?;

    match shell {
        Shell::Bash => {
            let dir = home.join(".local/share/bash-completion/completions");
            let filename = "bbr".to_string();
            let line = (
                ".bashrc",
                format!(r#"source "{}/{}""#, dir.display(), filename),
            );
            Ok((dir, filename, Some(line)))
        }
        Shell::Zsh => {
            let dir = home.join(".zsh/completions");
            let filename = "_bbr".to_string();
            let line = (".zshrc", format!(r#"fpath=({} $fpath)"#, dir.display()));
            Ok((dir, filename, Some(line)))
        }
        Shell::Fish => {
            let dir = home.join(".config/fish/completions");
            let filename = "bbr.fish".to_string();
            let line = (
                ".config/fish/config.fish",
                format!("source {}/bbr.fish", dir.display()),
            );
            Ok((dir, filename, Some(line)))
        }
        Shell::PowerShell => {
            let (dir, rc_path) = if cfg!(windows) {
                (
                    home.join("Documents").join("PowerShell"),
                    "Documents/PowerShell/Microsoft.PowerShell_profile.ps1",
                )
            } else {
                (
                    home.join(".config/powershell"),
                    ".config/powershell/Microsoft.PowerShell_profile.ps1",
                )
            };
            let filename = "bbr.ps1".to_string();
            let line = Some((rc_path, format!(". \"{}/bbr.ps1\"", dir.display())));
            Ok((dir, filename, line))
        }
        _ => Err(BitbucketError::Other(format!("unsupported shell: {shell}"))),
    }
}

//! The OS terminal window a session pops out into: `PDO_TERMINAL`, else the
//! terminal the daemon runs in, else the first one installed.

use anyhow::{Context, Result};

use crate::application::ports::TerminalWindow;

/// Opens a detached terminal program running the attach command.
#[derive(Debug, Clone, Copy, Default)]
pub struct OsTerminalWindow;

impl TerminalWindow for OsTerminalWindow {
    fn open(&self, session: &str, attach: &str) -> Result<String> {
        let terminal = detect_terminal();
        spawn_terminal_attach(&terminal, session, attach)?;
        Ok(terminal)
    }
}

fn detect_terminal() -> String {
    if let Ok(t) = std::env::var("PDO_TERMINAL") {
        if !t.is_empty() {
            return t;
        }
    }

    if let Ok(tp) = std::env::var("TERM_PROGRAM") {
        let tp_lower = tp.to_lowercase();
        if tp_lower.contains("kitty") {
            return "kitty".into();
        }
        if tp_lower.contains("alacritty") {
            return "alacritty".into();
        }
        if tp_lower.contains("iterm") {
            return "open -a iTerm".into();
        }
    }

    if cfg!(target_os = "macos") {
        return "open -a Terminal".into();
    }

    for candidate in &["kitty", "alacritty", "gnome-terminal", "konsole", "xterm"] {
        if which_exists(candidate) {
            return (*candidate).into();
        }
    }

    "xterm".into()
}

fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .is_ok_and(|o| o.status.success())
}

fn spawn_terminal_attach(terminal: &str, session_name: &str, tmux_cmd: &str) -> Result<()> {
    let parts: Vec<&str> = terminal.split_whitespace().collect();
    let (cmd, prefix_args) = parts
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("empty terminal command"))?;

    let mut command = std::process::Command::new(cmd);
    command.args(prefix_args);

    match *cmd {
        "gnome-terminal" => {
            command.args(["--", "bash", "-c", tmux_cmd]);
        }
        "konsole" => {
            command.args(["-e", "bash", "-c", tmux_cmd]);
        }
        "kitty" => {
            command.args(["bash", "-c", tmux_cmd]);
        }
        "alacritty" => {
            command.args(["-e", "bash", "-c", tmux_cmd]);
        }
        "xterm" => {
            command.args(["-e", tmux_cmd]);
        }
        "open" => {
            let script = format!("#!/bin/bash\n{tmux_cmd}\n");
            let script_path = std::env::temp_dir().join(format!("pdo-attach-{session_name}.sh"));
            std::fs::write(&script_path, &script)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))?;
            }
            command.arg(script_path);
        }
        _ => {
            command.args(["-e", "bash", "-c", tmux_cmd]);
        }
    }

    let child = command.spawn().context("failed to spawn terminal")?;
    std::mem::forget(child);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_terminal_respects_env_override() {
        // Save and set PDO_TERMINAL
        let prev = std::env::var("PDO_TERMINAL").ok();
        std::env::set_var("PDO_TERMINAL", "my-custom-terminal");
        let result = detect_terminal();
        // Restore
        match prev {
            Some(v) => std::env::set_var("PDO_TERMINAL", v),
            None => std::env::remove_var("PDO_TERMINAL"),
        }
        assert_eq!(result, "my-custom-terminal");
    }
}

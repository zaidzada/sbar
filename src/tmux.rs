use std::path::Path;
use std::process::Command;

const EDITORS: [&str; 3] = ["vim", "nvim", "vi"];

fn tmux(args: &[&str]) -> Option<String> {
    let out = Command::new("tmux").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Quote for a POSIX shell.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Escape for a vim `:e` argument.
fn vim_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if " \\|\"%#<>[]{}*?$'`&;()".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Open `path` in the pane to the right of ours. If that pane is already
/// running vim, use `:e`; if it's a shell, type the command; with no pane to
/// the right, open a new window named after the file.
pub fn open(path: &Path) {
    let p = path.to_string_lossy();
    let right = tmux(&["display", "-p", "-t", "{right-of}", "#{pane_id} #{pane_current_command}"])
        .filter(|s| !s.is_empty());
    match right.as_deref().and_then(|s| s.split_once(' ')) {
        Some((id, cmd)) if EDITORS.contains(&cmd) => {
            tmux(&["send-keys", "-t", id, "Escape"]);
            tmux(&["send-keys", "-t", id, "-l", &format!(":e {}", vim_escape(&p))]);
            tmux(&["send-keys", "-t", id, "Enter"]);
        }
        Some((id, _)) => {
            tmux(&["send-keys", "-t", id, "-l", &format!("vim {}", sh_quote(&p))]);
            tmux(&["send-keys", "-t", id, "Enter"]);
        }
        None => open_in_window(path),
    }
}

/// Open `path` in vim in a new tmux window named after the file.
pub fn open_in_window(path: &Path) {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    tmux(&["new-window", "-n", &name, &format!("vim {}", sh_quote(&path.to_string_lossy()))]);
}

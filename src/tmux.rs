use std::path::Path;
use std::process::Command;

use ratatui::style::Color;

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

/// The id and current command of the pane to the right of ours.
fn right_pane() -> Option<(String, String)> {
    let out = tmux(&["display", "-p", "-t", "{right-of}", "#{pane_id} #{pane_current_command}"])?;
    let (id, cmd) = out.split_once(' ')?;
    Some((id.to_string(), cmd.to_string()))
}

/// Type `cmd` into the shell in pane `id`, run it and focus the pane.
fn run_in(id: &str, cmd: &str) {
    tmux(&["send-keys", "-t", id, "-l", cmd]);
    tmux(&["send-keys", "-t", id, "Enter"]);
    tmux(&["select-pane", "-t", id]);
}

/// Run `prog` on `path` in a new tmux window named after the file.
fn new_window(prog: &str, path: &Path) {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    tmux(&["new-window", "-n", &name, &format!("{prog} {}", sh_quote(&path.to_string_lossy()))]);
}

/// Open `path` in the pane to the right of ours. If that pane is already
/// running vim, use `:e`; if it's a shell, type the command; with no pane to
/// the right, open a new window named after the file. Focus moves to wherever
/// the file was opened.
pub fn open(path: &Path) {
    let p = path.to_string_lossy();
    match right_pane() {
        Some((id, cmd)) if EDITORS.contains(&cmd.as_str()) => {
            tmux(&["send-keys", "-t", &id, "Escape"]);
            tmux(&["send-keys", "-t", &id, "-l", &format!(":e {}", vim_escape(&p))]);
            tmux(&["send-keys", "-t", &id, "Enter"]);
            tmux(&["select-pane", "-t", &id]);
        }
        Some((id, _)) => run_in(&id, &format!("vim {}", sh_quote(&p))),
        None => open_in_window(path),
    }
}

/// Open `path` in vim in a new tmux window named after the file.
pub fn open_in_window(path: &Path) {
    new_window("vim", path);
}

/// The pager command: `$PAGER`, or `less -R` when it's unset or empty.
fn pager() -> String {
    std::env::var("PAGER").ok().filter(|p| !p.trim().is_empty()).unwrap_or_else(|| "less -R".into())
}

/// The program name a pane running `cmd` reports: `/usr/bin/less -R` is `less`.
fn program(cmd: &str) -> &str {
    let first = cmd.split_whitespace().next().unwrap_or_default();
    first.rsplit('/').next().unwrap_or(first)
}

/// View `path` in the pager in the pane to the right of ours. If that pane is
/// already running the pager, quit it first; if it's a shell, type the
/// command. A pane busy in vim is left alone, and with no pane to the right
/// the file opens in a new window. Focus moves to wherever the file was opened.
pub fn view(path: &Path) {
    let pager = pager();
    match right_pane() {
        Some((id, cmd)) if !EDITORS.contains(&cmd.as_str()) => {
            if cmd == program(&pager) {
                tmux(&["send-keys", "-t", &id, "q"]);
            }
            run_in(&id, &format!("{pager} {}", sh_quote(&path.to_string_lossy())));
        }
        _ => new_window(&pager, path),
    }
}

/// The background of tmux's `mode-style` (its copy-mode selection), with
/// theme colours like `themedarkgrey` resolved for the current light/dark
/// theme. `None` outside tmux or when the style sets no usable background.
pub fn selection_bg() -> Option<Color> {
    let out = tmux(&["display", "-p", "#{theme}\t#{E:mode-style}"])?;
    let (theme, style) = out.split_once('\t')?;
    let bg = style_bg(style)?;
    let value = match theme_option(bg, theme) {
        Some(opt) => tmux(&["display", "-p", &format!("#{{{opt}}}")])?,
        None => bg.to_string(),
    };
    parse_colour(&value)
}

/// The `bg=` value of a tmux style such as `bg=red,bold`.
fn style_bg(style: &str) -> Option<&str> {
    style.split([',', ' ']).find_map(|s| s.strip_prefix("bg="))
}

/// The option holding a theme colour's value: `themedarkgrey` in a light theme
/// is `light-theme-dark-grey`. Like tmux, treat any theme but light as dark.
fn theme_option(colour: &str, theme: &str) -> Option<String> {
    let name = match colour.strip_prefix("theme")? {
        "darkgrey" | "darkgray" => "dark-grey",
        "lightgrey" | "lightgray" => "light-grey",
        n => n,
    };
    let set = if theme == "light" { "light" } else { "dark" };
    Some(format!("{set}-theme-{name}"))
}

/// A tmux colour: `#rrggbb`, `colourN` or a name like `red`.
fn parse_colour(s: &str) -> Option<Color> {
    if let Some(n) = s.strip_prefix("colour").or_else(|| s.strip_prefix("color")) {
        return n.parse().ok().map(Color::Indexed);
    }
    if s == "default" {
        return None;
    }
    s.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_colour() {
        assert_eq!(style_bg("bg=themedarkgrey"), Some("themedarkgrey"));
        assert_eq!(style_bg("fg=black,bg=yellow bold"), Some("yellow"));
        assert_eq!(style_bg("underscore"), None);
        assert_eq!(theme_option("themedarkgrey", "light").as_deref(), Some("light-theme-dark-grey"));
        assert_eq!(theme_option("themered", "detect").as_deref(), Some("dark-theme-red"));
        assert_eq!(theme_option("red", "dark"), None);
        assert_eq!(parse_colour("#313640"), Some(Color::Rgb(0x31, 0x36, 0x40)));
        assert_eq!(parse_colour("colour240"), Some(Color::Indexed(240)));
        assert_eq!(parse_colour("yellow"), Some(Color::Yellow));
        assert_eq!(parse_colour("default"), None);
    }

    #[test]
    fn pager_program() {
        assert_eq!(program("less -R"), "less");
        assert_eq!(program("/usr/bin/less"), "less");
        assert_eq!(program("bat --paging=always"), "bat");
    }
}

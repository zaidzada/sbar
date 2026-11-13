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

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Run the shell command `cmd` in a new tmux window called `name`.
fn window(name: &str, cmd: &str) {
    tmux(&["new-window", "-n", name, cmd]);
}

/// Run `prog` on `path` in a new tmux window named after the file.
fn new_window(prog: &str, path: &Path) {
    window(&file_name(path), &format!("{prog} {}", sh_quote(&path.to_string_lossy())));
}

/// Run the shell command `cmd` in an 80% tmux popup over the current pane,
/// titled `title`. The popup closes when the command exits.
fn popup(title: &str, cmd: &str) {
    tmux(&["display-popup", "-E", "-w", "80%", "-h", "80%", "-T", &format!(" {title} "), cmd]);
}

/// The editor command: `$EDITOR`, or `vim` when it's unset or empty.
fn editor() -> String {
    std::env::var("EDITOR").ok().filter(|e| !e.trim().is_empty()).unwrap_or_else(|| "vim".into())
}

/// Whether a pane running `cmd` is busy in vim or in `$EDITOR`.
fn editing(cmd: &str) -> bool {
    EDITORS.contains(&cmd) || cmd == program(&editor())
}

/// Open `path` in the pane to the right of ours. If that pane is already
/// running vim, use `:e`; if it's a shell, type the `$EDITOR` command; if it's
/// busy in some other editor, or there's no pane to the right, open a new
/// window named after the file. Focus moves to wherever the file was opened.
pub fn open(path: &Path) {
    let p = path.to_string_lossy();
    match right_pane() {
        Some((id, cmd)) if EDITORS.contains(&cmd.as_str()) => {
            tmux(&["send-keys", "-t", &id, "Escape"]);
            tmux(&["send-keys", "-t", &id, "-l", &format!(":e {}", vim_escape(&p))]);
            tmux(&["send-keys", "-t", &id, "Enter"]);
            tmux(&["select-pane", "-t", &id]);
        }
        Some((id, cmd)) if !editing(&cmd) => run_in(&id, &format!("{} {}", editor(), sh_quote(&p))),
        _ => open_in_window(path),
    }
}

/// Open `path` in `$EDITOR` in a new tmux window named after the file.
pub fn open_in_window(path: &Path) {
    new_window(&editor(), path);
}

/// Edit `path` in `$EDITOR` in a popup over the current pane, titled with the
/// file name. The popup closes when the editor exits.
pub fn edit(path: &Path) {
    popup(&file_name(path), &format!("{} {}", editor(), sh_quote(&path.to_string_lossy())));
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
/// command. A pane busy in an editor is left alone, and with no pane to the right
/// the file opens in a new window. Focus moves to wherever the file was opened.
pub fn view(path: &Path) {
    let pager = pager();
    match right_pane() {
        Some((id, cmd)) if !editing(&cmd) => {
            if cmd == program(&pager) {
                tmux(&["send-keys", "-t", &id, "q"]);
            }
            run_in(&id, &format!("{pager} {}", sh_quote(&path.to_string_lossy())));
        }
        _ => new_window(&pager, path),
    }
}

/// The shell command for previewing `path`: every word quoted, file last.
fn preview_cmd(command: &str, args: &[String], path: &Path) -> String {
    let mut words = vec![sh_quote(command)];
    words.extend(args.iter().map(|a| sh_quote(a)));
    words.push(sh_quote(&path.to_string_lossy()));
    words.join(" ")
}

/// Preview `path` with `command args... path` in a tmux popup over the current
/// pane, titled with the file name. The popup closes when the command exits.
pub fn preview(command: &str, args: &[String], path: &Path) {
    popup(&file_name(path), &preview_cmd(command, args, path));
}

/// The directory to run git in for `path`: the path itself if it's a directory.
fn git_dir(path: &Path) -> &Path {
    if path.is_dir() { path } else { path.parent().unwrap_or(path) }
}

/// Whether `git diff HEAD` has anything to show for `path`, or git's
/// complaint (such as not being in a repository) if it can't tell.
fn has_changes(path: &Path) -> Result<bool, String> {
    let git = |args: &[&str]| {
        Command::new("git").arg("-C").arg(git_dir(path)).args(args).arg(path).output().map_err(|e| format!("git: {e}"))
    };
    let complaint = |out: std::process::Output| {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err.lines().next().unwrap_or("git diff failed");
        line.trim_start_matches("fatal: ").split(" (").next().unwrap_or(line).to_string()
    };
    // outside a repository git diff quietly compares files instead (--no-index)
    let out = git(&["rev-parse", "--show-toplevel", "--"])?;
    if !out.status.success() {
        return Err(complaint(out));
    }
    let out = git(&["diff", "--quiet", "HEAD", "--"])?;
    match out.status.code() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err(complaint(out)),
    }
}

/// The shell command showing `git diff HEAD` for `path` in `pager`. Piping
/// into the pager ourselves keeps git's default `less -F` from quitting, and
/// closing the popup, when the diff fits on one screen.
fn diff_cmd(path: &Path, pager: &str) -> String {
    let dir = sh_quote(&git_dir(path).to_string_lossy());
    format!("git -C {dir} diff --color=always HEAD -- {} | {pager}", sh_quote(&path.to_string_lossy()))
}

/// Show the staged and unstaged changes to `path` (everything below it, for a
/// directory) in the pager, in a popup or a new window. With nothing to show,
/// returns a message saying so instead.
pub fn diff(path: &Path, in_window: bool) -> Result<(), String> {
    if !has_changes(path)? {
        return Err("no changes".into());
    }
    let cmd = diff_cmd(path, &pager());
    let name = format!("diff {}", file_name(path));
    if in_window { window(&name, &cmd) } else { popup(&name, &cmd) }
    Ok(())
}

/// Put `path` in a tmux paste buffer, which tmux also passes on to the system
/// clipboard when `set-clipboard` allows. Whether tmux took it.
pub fn copy(path: &Path) -> bool {
    tmux(&["set-buffer", "-w", "--", &path.to_string_lossy()]).is_some()
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

    #[test]
    fn diff_command() {
        assert_eq!(diff_cmd(Path::new("/r/a b.rs"), "less -R"), "git -C '/r' diff --color=always HEAD -- '/r/a b.rs' | less -R");
    }

    #[test]
    fn diff_status() {
        let dir = std::env::temp_dir().join(format!("sbar-test-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(has_changes(&dir).is_err()); // not a repository
        let git = |args: &[&str]| assert!(Command::new("git").arg("-C").arg(&dir).args(args).output().unwrap().status.success());
        git(&["init", "-q"]);
        std::fs::write(dir.join("f"), "a\n").unwrap();
        git(&["add", "f"]);
        git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "init"]);
        assert_eq!(has_changes(&dir.join("f")), Ok(false));
        std::fs::write(dir.join("f"), "b\n").unwrap();
        assert_eq!(has_changes(&dir.join("f")), Ok(true));
        assert_eq!(has_changes(&dir), Ok(true));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn preview_command() {
        assert_eq!(preview_cmd("less", &[], Path::new("/a/b c.txt")), "'less' '/a/b c.txt'");
        assert_eq!(preview_cmd("bat", &["--style=plain".into()], Path::new("it's")), r"'bat' '--style=plain' 'it'\''s'");
    }
}

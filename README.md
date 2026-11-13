# sbar

A minimal, `tree`-style file browser meant to live in a tmux sidebar pane. Navigate the tree, fold and unfold directories, and open files in your editor in the pane to the right (or in a new tmux window).

Written in Rust with [ratatui](https://ratatui.rs).

## Build

```sh
cargo build --release
```

The binary is `target/release/sbar`. Run `sbar [DIR]` (defaults to the current directory).

## tmux setup

Add a binding that opens sbar as a 35-column sidebar on the left, then reload your config:

```tmux
bind e split-window -hb -l 35 -c "#{pane_current_path}" "/path/to/sbar/target/release/sbar"
set -g mouse on   # needed for mouse support
```

## Keys

Press `?` in sbar to see this list.

| Key | Action |
| --- | --- |
| `j` / `↓`, `k` / `↑` | move down / up |
| `g` / `Home`, `G` / `End` | go to top / bottom |
| `Ctrl-d` / `Ctrl-u` | down / up 10 rows |
| `h` / `←` | fold directory, or jump to parent |
| `l` / `→` | unfold directory, or step into it |
| `Space` | toggle directory / preview file in a tmux popup |
| `Tab` | toggle fold |
| `Enter` | toggle directory / open file |
| `o` | open file in `$EDITOR` (default `vim`) in the pane to the right |
| `O` | open file in `$EDITOR` in a new tmux window named after the file |
| `e` | edit file in `$EDITOR` in a tmux popup |
| `p` | view file in `$PAGER` (default `less -R`) in the pane to the right |
| `d` | show `git diff HEAD` for the file or directory in a tmux popup |
| `D` | show `git diff HEAD` in a new tmux window |
| `c` | copy the absolute path to the tmux buffer and system clipboard |
| `E` | expand all visible folders one level |
| `C` | collapse all folders |
| `r` | refresh |
| `.` | show / hide dotfiles (shown, dimmed, by default) |
| `?` | toggle help |
| `q` / `Esc` | quit |

### Mouse

Click selects a row and toggles a folder, double-click opens a file, and the wheel scrolls. Requires `set -g mouse on` in tmux.

## Colours and markers

Names are coloured like GNU `ls` with its default `LS_COLORS`, using the terminal's ANSI palette:

| Type | Colour | Marker |
| --- | --- | --- |
| directory | bold blue | `▸` / `▾` |
| symlink | bold cyan | `@` |
| broken symlink | bold red | `@` |
| executable | bold green | `*` |
| named pipe | yellow | `\|` |
| socket | bold magenta | `=` |
| block / character device | bold yellow | |
| setuid / setgid file | white on red / black on yellow | `*` |
| sticky / other-writable / both directory | white on blue / blue on green / black on green | `▸` / `▾` |
| regular file | terminal default | |

Markers after the name follow `ls -F`, except that directories keep their fold arrow instead of a trailing `/`. A symlink to a directory is cyan with `@`, and still folds like a directory. Dotfiles are dimmed.

## How files are opened

`o`, `Enter` and double-click on a file target the pane to the right of sbar:

- if it is running `vim`/`nvim`/`vi`, sbar sends `:e <file>`
- if it is a shell, sbar types `$EDITOR '<file>'` (`vim` if `$EDITOR` is unset) and presses Enter
- if it is busy in another editor, or there is no pane to the right, the file opens in a new window

Focus then moves to the pane (or window) holding the file.

`O` always opens a new window named after the file.

`p` views the file in `$PAGER`, or `less -R` if it is unset, in the pane to the right. If that pane is already running the pager, sbar quits it first; if it is running an editor, or there is no pane to the right, the file opens in a new window instead.

`e` edits the file in `$EDITOR` in a popup like `Space`'s, for a quick change without leaving the sidebar; the popup closes when you quit the editor.

`d` shows the staged and unstaged changes to the selected file, or to everything below a directory, as `git diff HEAD` piped into the pager, in a popup like `Space`'s. `D` shows the same in a new window. With nothing to show, or outside a git repository, the footer says so instead. Untracked files have no diff.

`c` copies the selected path into a tmux paste buffer (`set-buffer -w`), which tmux passes on to the system clipboard when its `set-clipboard` option is `on` or `external` (the default).

`Space` on a file previews it in a tmux popup covering 80% of the window, running `less` by default; quit the previewer to close the popup. The command and its arguments are set under `[preview]` in the config.

## Config

sbar reads `~/.config/sbar/config.toml` (or `$XDG_CONFIG_HOME/sbar/config.toml`) if it exists:

```toml
# Names of files and directories to leave out of the tree.
# Default: [".DS_Store"]. Setting it replaces the default.
ignore = [".DS_Store", ".git", "target", "node_modules"]

# Command run by Space to preview a file in a tmux popup. The file is passed
# as the last argument. Default: command = "less", args = [].
[preview]
command = "bat"
args = ["--paging=always", "--style=plain"]
```

Ignore names are matched exactly against each entry's file name. An unknown key or invalid TOML is an error at startup.

## Development

```sh
cargo test
```

Layout: `src/config.rs` (config file), `src/tree.rs` (tree model, fold state, guide prefixes), `src/tmux.rs` (opening files via the `tmux` CLI), `src/main.rs` (state, keys, mouse, rendering).

## Roadmap

See [TODO.md](TODO.md).

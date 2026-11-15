# TODO

## Config file
- Done: `~/.config/sbar/config.toml` with an exact-name `ignore` list (default `[".DS_Store"]`)
- Glob patterns in the ignore list
- Default sort mode, show-hidden default, sidebar width
- Keybindings and the open command (editor, per file type)
- Shared by the features below, so it likely comes first

## Watch for changes
- Use the `notify` crate to refresh automatically, debounced (about 100-200ms)
- Reuse `App::refresh()`, which already keeps fold state and cursor
- Watch only expanded directories, not the whole tree

## Sorting
- Modes: name, modified time, size, extension; a key to cycle them, indicator in the footer
- Dirs-first as a separate toggle
- Sorting lives in `Node::load` in `src/tree.rs`

## Git status gutter
- Done: `M`, `A`, `?`, `U` from `git status --porcelain`, green when staged and red when not, rolled up to directories; refreshed on `r`, focus and after `e`
- Refresh alongside the file watcher

## `/` filter
- Filter mode on `/`: query shown in the footer, Backspace edits, `Enter` accepts, `Esc` clears
- Search the whole tree (loading everything below the root), skipping the ignore list; symlinked dirs are followed, so guard against link cycles (track canonical paths)
- Matches keep their parent folders visible so the tree guides still make sense
- Start with smart-case substring matching on filenames; maybe full relative paths and fuzzy matching (`nucleo`) later
- Highlight matches

## Maybe
- `sbar --toggle`, or a focus-aware toggle, so tmux bindings need no helper script
- Ship the toggle script in the repo (`contrib/toggle.sh`)
- Vim-style fold keys (`zc`, `zo`, `za`, `zM`, `zR`)

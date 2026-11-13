use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What changed. Variants are in rising order of importance, so a directory
/// shows the greatest change found below it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Change {
    Untracked,
    Added,
    Modified,
    Conflict,
}

/// A file's git status as the gutter shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark {
    pub change: Change,
    /// Whether all of the change is in the index, leaving nothing to stage.
    pub staged: bool,
}

impl Mark {
    /// Map a porcelain `XY` code (index, worktree) to a mark.
    fn from_xy(x: u8, y: u8) -> Mark {
        let change = match (x, y) {
            (b'?', b'?') => Change::Untracked,
            (b'U', _) | (_, b'U') | (b'A', b'A') | (b'D', b'D') => Change::Conflict,
            // a rename or copy is new at the path the tree shows
            (b'A' | b'R' | b'C', _) => Change::Added,
            _ => Change::Modified,
        };
        Mark { change, staged: y == b' ' }
    }

    /// The mark of a directory holding both: the greater change, staged only
    /// if both are.
    fn merge(self, other: Mark) -> Mark {
        Mark { change: self.change.max(other.change), staged: self.staged && other.staged }
    }

    pub fn symbol(self) -> char {
        match self.change {
            Change::Untracked => '?',
            Change::Added => 'A',
            Change::Modified => 'M',
            Change::Conflict => 'U',
        }
    }
}

/// The checked-out branch and how far it is from its upstream, as of the
/// last fetch.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Branch {
    /// The branch name, or `HEAD` when detached.
    pub name: String,
    pub ahead: u32,
    pub behind: u32,
}

impl Branch {
    /// Parse the `## ` header of `git status --porcelain=v1 --branch`, such as
    /// `main...origin/main [ahead 2, behind 1]`.
    fn parse(header: &str) -> Branch {
        let header = header.strip_prefix("No commits yet on ").unwrap_or(header);
        let (head, counts) = header.split_once(" [").unwrap_or((header, ""));
        let name = match head.split_once("...") {
            Some((name, _upstream)) => name,
            None if head == "HEAD (no branch)" => "HEAD",
            None => head,
        };
        let count = |key: &str| {
            counts.trim_end_matches(']').split(", ").find_map(|c| c.strip_prefix(key)?.parse().ok()).unwrap_or(0)
        };
        Branch { name: name.to_string(), ahead: count("ahead "), behind: count("behind ") }
    }
}

/// The git status of everything below a directory.
#[derive(Default)]
pub struct Status {
    pub branch: Branch,
    /// Marks for the paths git reported, and for every directory above them.
    marks: HashMap<PathBuf, Mark>,
    /// Untracked directories git reported whole, whose contents are untracked too.
    dirs: Vec<(PathBuf, Mark)>,
}

impl Status {
    /// Run `git status` for `root`, or `None` outside a work tree.
    pub fn load(root: &Path) -> Option<Status> {
        let git = |args: &[&str]| {
            // optional locks off, so a refresh never fights the user's own git commands over index.lock
            let out = Command::new("git").arg("--no-optional-locks").arg("-C").arg(root).args(args).output().ok()?;
            out.status.success().then_some(out.stdout)
        };
        let top = git(&["rev-parse", "--show-toplevel"])?;
        let top = PathBuf::from(String::from_utf8_lossy(&top).trim_end());
        // root is canonical, so match git's top-level to it (/tmp vs /private/tmp)
        let top = top.canonicalize().unwrap_or(top);
        let out = git(&["status", "--porcelain=v1", "-z", "--branch", "--", "."])?;
        Some(Status::parse(&top, root, &out))
    }

    /// Parse `git status --porcelain=v1 -z --branch` output, whose paths are relative to
    /// `top`, rolling marks up to the directories between each path and `root`.
    fn parse(top: &Path, root: &Path, out: &[u8]) -> Status {
        let mut status = Status::default();
        let mut fields = out.split(|&b| b == 0).filter(|f| !f.is_empty());
        while let Some(f) = fields.next() {
            if let Some(header) = f.strip_prefix(b"## ") {
                status.branch = Branch::parse(&String::from_utf8_lossy(header));
                continue;
            }
            if f.len() < 4 {
                continue;
            }
            let mark = Mark::from_xy(f[0], f[1]);
            if matches!(f[0], b'R' | b'C') {
                fields.next(); // the original path
            }
            let name = String::from_utf8_lossy(&f[3..]);
            let path = top.join(name.trim_end_matches('/'));
            if name.ends_with('/') {
                status.dirs.push((path.clone(), mark));
            }
            for p in path.ancestors().take_while(|p| p.starts_with(root)) {
                let m = status.marks.entry(p.to_path_buf()).or_insert(mark);
                *m = m.merge(mark);
            }
        }
        status
    }

    pub fn get(&self, path: &Path) -> Option<Mark> {
        self.marks.get(path).copied().or_else(|| self.dirs.iter().find(|(d, _)| path.starts_with(d)).map(|&(_, m)| m))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_roll_up() {
        let top = Path::new("/r");
        let out = b"## main...origin/main [ahead 1]\0 M src/a.rs\0AM src/new.rs\0?? notes/\0R  lib/b.rs\0lib/old.rs\0M  lib/c.rs\0 D gone.txt\0UU c.rs\0";
        let s = Status::parse(top, top, out);
        // the symbol, upper case when staged
        let get = |p: &str| {
            s.get(&top.join(p)).map(|m| if m.staged { m.symbol() } else { m.symbol().to_ascii_lowercase() })
        };
        assert_eq!(get("src/a.rs"), Some('m'));
        assert_eq!(get("src/new.rs"), Some('a')); // added, then changed again
        assert_eq!(get("src"), Some('m'));
        assert_eq!(get("lib/b.rs"), Some('A'));
        assert_eq!(get("lib/old.rs"), None);
        assert_eq!(get("lib/c.rs"), Some('M'));
        assert_eq!(get("lib"), Some('M')); // everything below is staged
        assert_eq!(get("notes"), Some('?'));
        assert_eq!(get("notes/deep/x"), Some('?'));
        assert_eq!(get("README.md"), None);
        assert_eq!(get(""), Some('u')); // the root takes the conflict
        assert_eq!(get("c.rs"), Some('u'));
        assert_eq!(s.branch, Branch { name: "main".into(), ahead: 1, behind: 0 });

        // a root below the top-level only rolls up as far as itself
        let s = Status::parse(top, &top.join("src"), b" M src/a.rs\0");
        assert_eq!(s.get(&top.join("src")).map(|m| m.change), Some(Change::Modified));
        assert_eq!(s.get(top), None);
    }

    #[test]
    fn branch_header() {
        let b = |name: &str, ahead, behind| Branch { name: name.into(), ahead, behind };
        assert_eq!(Branch::parse("main"), b("main", 0, 0));
        assert_eq!(Branch::parse("main...origin/main"), b("main", 0, 0));
        assert_eq!(Branch::parse("main...origin/main [behind 3]"), b("main", 0, 3));
        assert_eq!(Branch::parse("feat/x...origin/feat/x [ahead 2, behind 1]"), b("feat/x", 2, 1));
        assert_eq!(Branch::parse("main...origin/main [gone]"), b("main", 0, 0));
        assert_eq!(Branch::parse("No commits yet on main"), b("main", 0, 0));
        assert_eq!(Branch::parse("HEAD (no branch)"), b("HEAD", 0, 0));
    }

    #[test]
    fn load_from_repo() {
        let dir = std::env::temp_dir().join(format!("sbar-test-status-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let dir = dir.canonicalize().unwrap();
        let git = |args: &[&str]| assert!(Command::new("git").arg("-C").arg(&dir).args(args).output().unwrap().status.success());
        git(&["init", "-q"]);
        std::fs::write(dir.join("sub/f"), "a").unwrap();
        std::fs::write(dir.join(".gitignore"), "ign\n").unwrap();
        git(&["add", "."]);
        git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "init"]);
        std::fs::write(dir.join("sub/f"), "b").unwrap();
        std::fs::write(dir.join("ign"), "").unwrap();
        std::fs::write(dir.join("new"), "").unwrap();
        std::fs::write(dir.join("staged"), "").unwrap();
        git(&["add", "staged"]);

        let s = Status::load(&dir).unwrap();
        let get = |p: &str| s.get(&dir.join(p)).map(|m| (m.change, m.staged));
        assert_eq!(get("sub/f"), Some((Change::Modified, false)));
        assert_eq!(get("sub"), Some((Change::Modified, false)));
        assert_eq!(get("staged"), Some((Change::Added, true)));
        assert_eq!(get("new"), Some((Change::Untracked, false)));
        assert_eq!(get("ign"), None);
        assert_eq!(get(".gitignore"), None);
        assert!(!s.branch.name.is_empty());
        // from a subdirectory, only what's below it
        let s = Status::load(&dir.join("sub")).unwrap();
        assert_eq!(s.get(&dir.join("new")), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

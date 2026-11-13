use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// What an entry is, as far as `ls` colours and `ls -F` markers care. Where
/// several apply, the one `ls` gives precedence to wins (setuid over
/// executable, sticky + other-writable over either alone).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Exec,
    Setuid,
    Setgid,
    Dir,
    Sticky,
    OtherWritable,
    StickyOtherWritable,
    Symlink,
    BrokenLink,
    Fifo,
    Socket,
    Device,
}

impl Kind {
    /// Classify `path` without following a final symlink, and say whether it
    /// (or what it links to) is a directory.
    pub fn of(path: &Path) -> (Kind, bool) {
        let Ok(meta) = fs::symlink_metadata(path) else { return (Kind::File, false) };
        let ft = meta.file_type();
        if ft.is_symlink() {
            // fs::metadata follows the link, so a link to a directory is a directory
            return match fs::metadata(path) {
                Ok(target) => (Kind::Symlink, target.is_dir()),
                Err(_) => (Kind::BrokenLink, false),
            };
        }
        let mode = meta.permissions().mode();
        let kind = if ft.is_dir() {
            match (mode & 0o1000 != 0, mode & 0o002 != 0) {
                (true, true) => Kind::StickyOtherWritable,
                (false, true) => Kind::OtherWritable,
                (true, false) => Kind::Sticky,
                (false, false) => Kind::Dir,
            }
        } else if ft.is_fifo() {
            Kind::Fifo
        } else if ft.is_socket() {
            Kind::Socket
        } else if ft.is_block_device() || ft.is_char_device() {
            Kind::Device
        } else if mode & 0o4000 != 0 {
            Kind::Setuid
        } else if mode & 0o2000 != 0 {
            Kind::Setgid
        } else if mode & 0o111 != 0 {
            Kind::Exec
        } else {
            Kind::File
        };
        (kind, ft.is_dir())
    }

    /// The `ls -F` suffix, left off regular files and directories.
    pub fn marker(self) -> Option<char> {
        match self {
            Kind::Exec | Kind::Setuid | Kind::Setgid => Some('*'),
            Kind::Symlink | Kind::BrokenLink => Some('@'),
            Kind::Fifo => Some('|'),
            Kind::Socket => Some('='),
            _ => None,
        }
    }
}

pub struct Node {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub kind: Kind,
    pub expanded: bool,
    loaded: bool,
    children: Vec<Node>,
}

pub struct Row {
    pub prefix: String,
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub kind: Kind,
    pub expanded: bool,
}

impl Node {
    pub fn new(path: PathBuf) -> Self {
        let (kind, is_dir) = Kind::of(&path);
        Node::with_kind(path, kind, is_dir)
    }

    fn with_kind(path: PathBuf, kind: Kind, is_dir: bool) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        Node { path, name, is_dir, kind, expanded: false, loaded: false, children: Vec::new() }
    }

    /// (Re)read this directory, skipping names in `ignore` and keeping the
    /// state of children that still exist.
    pub fn load(&mut self, ignore: &[String]) {
        let mut old: HashMap<String, Node> =
            std::mem::take(&mut self.children).into_iter().map(|n| (n.name.clone(), n)).collect();
        let mut entries: Vec<(String, PathBuf, Kind, bool)> = fs::read_dir(&self.path)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| !ignore.iter().any(|i| *i == *e.file_name().to_string_lossy()))
            .map(|e| {
                // A link to a directory is a directory (a broken link is a file).
                // Loading is lazy, so a link cycle only goes as deep as the user
                // unfolds it.
                let (kind, is_dir) = Kind::of(&e.path());
                (e.file_name().to_string_lossy().into_owned(), e.path(), kind, is_dir)
            })
            .collect();
        entries.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
        self.children = entries
            .into_iter()
            .map(|(name, path, kind, is_dir)| {
                let mut n = match old.remove(&name) {
                    Some(o) if o.is_dir == is_dir => Node { kind, ..o },
                    _ => Node::with_kind(path, kind, is_dir),
                };
                if n.is_dir && n.expanded {
                    n.load(ignore);
                }
                n
            })
            .collect();
        self.loaded = true;
    }

    pub fn expand(&mut self, ignore: &[String]) {
        if self.is_dir {
            if !self.loaded {
                self.load(ignore);
            }
            self.expanded = true;
        }
    }

    /// Expand every collapsed directory among the visible rows, revealing one more level.
    pub fn expand_next_level(&mut self, show_hidden: bool, ignore: &[String]) {
        for c in &mut self.children {
            if !show_hidden && c.name.starts_with('.') {
                continue;
            }
            if c.expanded {
                c.expand_next_level(show_hidden, ignore);
            } else {
                c.expand(ignore);
            }
        }
    }

    /// Collapse every directory below this node, leaving this node itself as is.
    pub fn collapse_all(&mut self) {
        for c in &mut self.children {
            c.collapse();
            c.collapse_all();
        }
    }

    pub fn collapse(&mut self) {
        self.expanded = false;
    }

    pub fn find_mut(&mut self, path: &Path) -> Option<&mut Node> {
        if self.path == path {
            return Some(self);
        }
        if !path.starts_with(&self.path) {
            return None;
        }
        self.children.iter_mut().find_map(|c| c.find_mut(path))
    }

    /// Flatten the visible part of the tree, with `tree`-style guide prefixes.
    pub fn rows(&self, show_hidden: bool) -> Vec<Row> {
        let mut rows = vec![self.row(String::new())];
        if self.expanded {
            self.push_children(show_hidden, &mut Vec::new(), &mut rows);
        }
        rows
    }

    fn row(&self, prefix: String) -> Row {
        Row {
            prefix,
            path: self.path.clone(),
            name: self.name.clone(),
            is_dir: self.is_dir,
            kind: self.kind,
            expanded: self.expanded,
        }
    }

    fn push_children(&self, show_hidden: bool, ancestors_last: &mut Vec<bool>, rows: &mut Vec<Row>) {
        let visible: Vec<&Node> =
            self.children.iter().filter(|c| show_hidden || !c.name.starts_with('.')).collect();
        for (i, child) in visible.iter().enumerate() {
            let last = i + 1 == visible.len();
            let mut prefix: String =
                ancestors_last.iter().map(|&l| if l { "    " } else { "│   " }).collect();
            prefix.push_str(if last { "└── " } else { "├── " });
            rows.push(child.row(prefix));
            if child.expanded {
                ancestors_last.push(last);
                child.push_children(show_hidden, ancestors_last, rows);
                ancestors_last.pop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guides_and_fold() {
        let dir = std::env::temp_dir().join(format!("sbar-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("a")).unwrap();
        fs::write(dir.join("a/x"), "").unwrap();
        fs::write(dir.join("b"), "").unwrap();
        fs::write(dir.join(".hidden"), "").unwrap();
        fs::write(dir.join(".DS_Store"), "").unwrap();
        let ignore = [".DS_Store".to_string()];

        let mut root = Node::new(dir.clone());
        root.expand(&ignore);
        let names = |r: &[Row]| r.iter().map(|r| format!("{}{}", r.prefix, r.name)).collect::<Vec<_>>();
        assert_eq!(&names(&root.rows(false))[1..], ["├── a", "└── b"]);

        root.find_mut(&dir.join("a")).unwrap().expand(&ignore);
        assert_eq!(&names(&root.rows(false))[1..], ["├── a", "│   └── x", "└── b"]);
        assert_eq!(root.rows(true).len(), 5); // .hidden shown, .DS_Store ignored

        root.find_mut(&dir.join("a")).unwrap().collapse();
        assert_eq!(root.rows(false).len(), 3);

        fs::create_dir_all(dir.join("a/deep/deeper")).unwrap();
        root.find_mut(&dir.join("a")).unwrap().load(&ignore);
        root.expand_next_level(false, &ignore);
        assert_eq!(root.rows(false).len(), 5); // a expanded, deep shown but collapsed
        root.expand_next_level(false, &ignore);
        assert_eq!(root.rows(false).len(), 6); // deep expanded, deeper shown

        root.collapse_all();
        assert_eq!(&names(&root.rows(false))[1..], ["├── a", "└── b"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn follows_symlinks() {
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!("sbar-test-links-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("real")).unwrap();
        fs::write(dir.join("real/x"), "").unwrap();
        symlink(dir.join("real"), dir.join("link")).unwrap();
        symlink(&dir, dir.join("real/up")).unwrap(); // cycle back to the root
        symlink(dir.join("missing"), dir.join("broken")).unwrap();

        let mut root = Node::new(dir.clone());
        root.expand(&[]);
        let names = |r: &[Row]| r.iter().map(|r| format!("{}{}", r.prefix, r.name)).collect::<Vec<_>>();
        // link and real are directories (sorted first); the broken link is a file
        assert_eq!(&names(&root.rows(false))[1..], ["├── link", "├── real", "└── broken"]);

        let link = root.find_mut(&dir.join("link")).unwrap();
        link.expand(&[]);
        assert_eq!(link.rows(false).iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["link", "up", "x"]);

        // following the cycle only goes as deep as it is unfolded
        root.find_mut(&dir.join("link/up")).unwrap().expand(&[]);
        root.load(&[]);
        assert_eq!(root.rows(false).len(), 1 + 3 + 2 + 3);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn kinds_and_markers() {
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!("sbar-test-kinds-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // a sandbox may refuse the setuid/setgid bits, so report whether the mode stuck
        let mode = |name: &str, m: u32| {
            let _ = fs::set_permissions(dir.join(name), fs::Permissions::from_mode(m));
            fs::metadata(dir.join(name)).unwrap().permissions().mode() & 0o7777 == m
        };
        for name in ["plain", "exec", "suid", "sgid"] {
            fs::write(dir.join(name), "").unwrap();
        }
        mode("exec", 0o755);
        let suid = mode("suid", 0o4755);
        let sgid = mode("sgid", 0o2755);
        for (name, m) in [("dir", 0o755), ("sticky", 0o1755), ("ow", 0o777), ("tmp", 0o1777)] {
            fs::create_dir(dir.join(name)).unwrap();
            mode(name, m);
        }
        symlink(dir.join("dir"), dir.join("dirlink")).unwrap();
        symlink(dir.join("missing"), dir.join("broken")).unwrap();
        assert!(std::process::Command::new("mkfifo").arg(dir.join("fifo")).status().unwrap().success());
        // binding a socket can be refused in a sandbox; skip that case if so
        let sock = std::os::unix::net::UnixListener::bind(dir.join("sock")).ok();

        let k = |name: &str| Kind::of(&dir.join(name));
        assert_eq!(k("plain"), (Kind::File, false));
        assert_eq!(k("exec"), (Kind::Exec, false));
        assert_eq!(k("suid"), (if suid { Kind::Setuid } else { Kind::Exec }, false));
        assert_eq!(k("sgid"), (if sgid { Kind::Setgid } else { Kind::Exec }, false));
        assert_eq!(k("dir"), (Kind::Dir, true));
        assert_eq!(k("sticky"), (Kind::Sticky, true));
        assert_eq!(k("ow"), (Kind::OtherWritable, true));
        assert_eq!(k("tmp"), (Kind::StickyOtherWritable, true));
        assert_eq!(k("dirlink"), (Kind::Symlink, true));
        assert_eq!(k("broken"), (Kind::BrokenLink, false));
        assert_eq!(k("fifo"), (Kind::Fifo, false));
        if sock.is_some() {
            assert_eq!(k("sock"), (Kind::Socket, false));
        }
        assert_eq!(Kind::of(Path::new("/dev/null")), (Kind::Device, false));

        let mut root = Node::new(dir.clone());
        root.expand(&[]);
        let shown: Vec<String> = root.rows(false)[1..]
            .iter()
            .filter(|r| r.name != "sock")
            .map(|r| format!("{}{}", r.name, r.kind.marker().unwrap_or(' ')))
            .collect();
        assert_eq!(shown, ["dir ", "dirlink@", "ow ", "sticky ", "tmp ", "broken@", "exec*", "fifo|", "plain ", "sgid*", "suid*"]);
        assert_eq!(Kind::Socket.marker(), Some('='));

        // a refresh picks up a mode change on a node it keeps
        mode("plain", 0o755);
        root.load(&[]);
        assert_eq!(root.find_mut(&dir.join("plain")).unwrap().kind, Kind::Exec);
        fs::remove_dir_all(&dir).unwrap();
    }
}

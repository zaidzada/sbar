use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub struct Node {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub expanded: bool,
    loaded: bool,
    children: Vec<Node>,
}

pub struct Row {
    pub prefix: String,
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub expanded: bool,
}

impl Node {
    pub fn new(path: PathBuf, is_dir: bool) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        Node { path, name, is_dir, expanded: false, loaded: false, children: Vec::new() }
    }

    /// (Re)read this directory, skipping names in `ignore` and keeping the
    /// state of children that still exist.
    pub fn load(&mut self, ignore: &[String]) {
        let mut old: HashMap<String, Node> =
            std::mem::take(&mut self.children).into_iter().map(|n| (n.name.clone(), n)).collect();
        let mut entries: Vec<(String, PathBuf, bool)> = fs::read_dir(&self.path)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| !ignore.iter().any(|i| *i == *e.file_name().to_string_lossy()))
            .map(|e| {
                // file_type() doesn't follow symlinks, so symlinked dirs can't cause loops
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                (e.file_name().to_string_lossy().into_owned(), e.path(), is_dir)
            })
            .collect();
        entries.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
        self.children = entries
            .into_iter()
            .map(|(name, path, is_dir)| {
                let mut n = match old.remove(&name) {
                    Some(o) if o.is_dir == is_dir => o,
                    _ => Node::new(path, is_dir),
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

        let mut root = Node::new(dir.clone(), true);
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
}

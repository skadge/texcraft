//! The in-memory file system the module sees: directories and files under
//! `/`. A file of the TeX tree is listed with its size and a source, and
//! its bytes are loaded the first time the module reads it.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

/// What a file holds: its bytes, or where to load them from.
#[derive(Debug)]
pub(crate) enum Data {
    Bytes(Vec<u8>),
    /// Not loaded yet: `size` bytes from `source` (a key the host's
    /// [`Platform::load`](crate::Platform::load) understands).
    Lazy {
        size: u64,
        source: Rc<str>,
    },
}

#[derive(Debug)]
pub(crate) struct File {
    pub(crate) data: Data,
    /// Nanoseconds since the epoch. Files of the TeX tree keep 0, so the
    /// caches made from them stay valid from one visit to the next.
    pub(crate) mtime: u64,
}

impl File {
    pub(crate) fn size(&self) -> u64 {
        match &self.data {
            Data::Bytes(b) => b.len() as u64,
            Data::Lazy { size, .. } => *size,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct Dir {
    pub(crate) children: BTreeMap<String, Node>,
    pub(crate) mtime: u64,
}

/// A file or a directory, shared by its directory and the descriptors
/// open on it (an unlinked file stays readable through them).
#[derive(Debug, Clone)]
pub(crate) enum Node {
    File(Rc<RefCell<File>>),
    Dir(Rc<RefCell<Dir>>),
}

impl Node {
    pub(crate) fn is_dir(&self) -> bool {
        matches!(self, Node::Dir(_))
    }

    pub(crate) fn mtime(&self) -> u64 {
        match self {
            Node::File(f) => f.borrow().mtime,
            Node::Dir(d) => d.borrow().mtime,
        }
    }
}

/// The components of `path`, with `.` dropped and `..` applied (never
/// above `/`).
pub(crate) fn split(path: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            p => out.push(p),
        }
    }
    out
}

/// The file system of the module: a tree under `/`.
#[derive(Debug, Clone)]
pub struct FileSystem {
    pub(crate) root: Rc<RefCell<Dir>>,
}

impl Default for FileSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystem {
    pub fn new() -> FileSystem {
        FileSystem { root: Rc::new(RefCell::new(Dir::default())) }
    }

    pub(crate) fn lookup_parts(&self, parts: &[&str]) -> Option<Node> {
        let mut node = Node::Dir(self.root.clone());
        for part in parts {
            let next = match &node {
                Node::Dir(d) => d.borrow().children.get(*part).cloned()?,
                Node::File(_) => return None,
            };
            node = next;
        }
        Some(node)
    }

    pub(crate) fn lookup(&self, path: &str) -> Option<Node> {
        self.lookup_parts(&split(path))
    }

    /// The directory at `parts`, made with its parents where missing;
    /// `None` when a file is in the way.
    pub(crate) fn mkdirs_parts(&self, parts: &[&str], mtime: u64) -> Option<Rc<RefCell<Dir>>> {
        let mut dir = self.root.clone();
        for part in parts {
            let next = {
                let mut d = dir.borrow_mut();
                match d.children.get(*part) {
                    Some(Node::Dir(sub)) => sub.clone(),
                    Some(Node::File(_)) => return None,
                    None => {
                        let sub = Rc::new(RefCell::new(Dir { children: BTreeMap::new(), mtime }));
                        d.children.insert((*part).to_owned(), Node::Dir(sub.clone()));
                        sub
                    }
                }
            };
            dir = next;
        }
        Some(dir)
    }

    fn put(&self, path: &str, file: File) -> bool {
        let parts = split(path);
        let Some((name, parents)) = parts.split_last() else { return false };
        let mtime = file.mtime;
        match self.mkdirs_parts(parents, mtime) {
            Some(dir) => {
                dir.borrow_mut().children.insert((*name).to_owned(), Node::File(Rc::new(RefCell::new(file))));
                true
            }
            None => false,
        }
    }

    /// Adds (or replaces) the file at `path` with `bytes`. Returns false
    /// when a file is in the way of its directories.
    pub fn add(&self, path: &str, bytes: Vec<u8>, mtime: u64) -> bool {
        self.put(path, File { data: Data::Bytes(bytes), mtime })
    }

    /// Adds a file of `size` bytes loaded from `source` when it is first
    /// read, with the time 0.
    pub fn add_lazy(&self, path: &str, size: u64, source: Rc<str>) -> bool {
        self.put(path, File { data: Data::Lazy { size, source }, mtime: 0 })
    }

    /// Makes the directory `path` and its parents.
    pub fn mkdirs(&self, path: &str, mtime: u64) -> bool {
        self.mkdirs_parts(&split(path), mtime).is_some()
    }

    /// Removes the file or directory at `path`, if there is one.
    pub fn remove(&self, path: &str) {
        let parts = split(path);
        let Some((name, parents)) = parts.split_last() else { return };
        if let Some(Node::Dir(d)) = self.lookup_parts(parents) {
            d.borrow_mut().children.remove(*name);
        }
    }

    /// Whether there is a file or directory at `path`.
    pub fn exists(&self, path: &str) -> bool {
        self.lookup(path).is_some()
    }

    /// The bytes of the file at `path`, if it is there and loaded (a file
    /// the module wrote, or one of the tree it read).
    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        match self.lookup(path)? {
            Node::File(f) => match &f.borrow().data {
                Data::Bytes(b) => Some(b.clone()),
                Data::Lazy { .. } => None,
            },
            Node::Dir(_) => None,
        }
    }

    /// The files under the directory `path`, at any depth, as their
    /// absolute paths with their time, loaded or not.
    pub fn files(&self, path: &str) -> Vec<(String, u64)> {
        let mut out = Vec::new();
        let parts = split(path);
        if let Some(Node::Dir(d)) = self.lookup_parts(&parts) {
            let mut base = String::new();
            for p in &parts {
                base.push('/');
                base.push_str(p);
            }
            walk(&d.borrow(), &base, &mut out);
        }
        out
    }
}

fn walk(dir: &Dir, base: &str, out: &mut Vec<(String, u64)>) {
    for (name, child) in &dir.children {
        let path = format!("{base}/{name}");
        match child {
            Node::Dir(d) => walk(&d.borrow(), &path, out),
            Node::File(f) => out.push((path, f.borrow().mtime)),
        }
    }
}

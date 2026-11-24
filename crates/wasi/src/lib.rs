//! A WASI preview 1 host for retex's WebAssembly module (`wasm32-wasip1`):
//! the functions the module imports, over an in-memory [`FileSystem`].
//!
//! It is plain Rust with no I/O of its own. The module's memory is a
//! [`Memory`]; the clock, randomness, output and the loading of the TeX
//! tree's files come from a [`Platform`]. In the browser, a binding hands
//! these functions to `WebAssembly.instantiate`; in tests, a byte vector is
//! the memory.
//!
//! A call either returns an [`Errno`] to the module or stops the run
//! ([`Stop`]): at `proc_exit`, or when a file must be loaded that the
//! platform cannot give at once (a browser can't always wait for a
//! download inside a call), in which case the host fetches it and runs the
//! module again.
//!
//! One preopened directory, `/` (descriptor 3): wasi-libc resolves relative
//! paths from its own current directory, `/` at the start.

mod fs;
#[cfg(target_arch = "wasm32")]
pub mod web;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

pub use fs::FileSystem;
use fs::{Data, File, Node, split};

/// A WASI error number, returned to the module (0 is success).
pub type Errno = u16;

pub mod errno {
    use super::Errno;
    pub const SUCCESS: Errno = 0;
    pub const BADF: Errno = 8;
    pub const EXIST: Errno = 20;
    pub const FAULT: Errno = 21;
    pub const INVAL: Errno = 28;
    pub const ISDIR: Errno = 31;
    pub const NOENT: Errno = 44;
    pub const NOTDIR: Errno = 54;
    pub const NOTEMPTY: Errno = 55;
    pub const NOTSUP: Errno = 58;
}

const FILETYPE_CHARACTER_DEVICE: u8 = 2;
const FILETYPE_DIRECTORY: u8 = 3;
const FILETYPE_REGULAR: u8 = 4;
const OFLAGS_CREAT: u16 = 1;
const OFLAGS_DIRECTORY: u16 = 2;
const OFLAGS_EXCL: u16 = 4;
const OFLAGS_TRUNC: u16 = 8;
const FDFLAGS_APPEND: u16 = 1;
const FSTFLAGS_MTIM: u16 = 4;
const FSTFLAGS_MTIM_NOW: u16 = 8;

/// Why a run stops before the module returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// `proc_exit` with this status.
    Exit(u32),
    /// The file of this source must be fetched first; the run is over, and
    /// is made again once the platform has it.
    Fetch(Rc<str>),
    /// The module handed the host an address outside its memory.
    Fault,
}

/// What a call gives back: an error number for the module, or the end of
/// the run.
pub type Outcome = Result<Errno, Stop>;

/// The module's linear memory.
pub trait Memory {
    /// Copies `out.len()` bytes at `at` into `out`; false when outside.
    fn read(&self, at: u32, out: &mut [u8]) -> bool;
    /// Copies `data` to `at`; false when outside.
    fn write(&mut self, at: u32, data: &[u8]) -> bool;
}

impl Memory for Vec<u8> {
    fn read(&self, at: u32, out: &mut [u8]) -> bool {
        let at = at as usize;
        match self.get(at..at + out.len()) {
            Some(src) => {
                out.copy_from_slice(src);
                true
            }
            None => false,
        }
    }

    fn write(&mut self, at: u32, data: &[u8]) -> bool {
        let at = at as usize;
        match self.get_mut(at..at + data.len()) {
            Some(dst) => {
                dst.copy_from_slice(data);
                true
            }
            None => false,
        }
    }
}

/// What the host takes from where it runs.
pub trait Platform {
    /// Nanoseconds since the epoch.
    fn realtime_ns(&self) -> u64;
    /// Nanoseconds from some fixed point, never going back.
    fn monotonic_ns(&self) -> u64;
    /// Fills `buf` with random bytes.
    fn random(&mut self, buf: &mut [u8]);
    /// The `size` bytes of `source`, or `None` when they can't be had
    /// now (the run then stops with [`Stop::Fetch`]).
    fn load(&mut self, source: &str, size: u64) -> Option<Vec<u8>>;
    /// The module wrote to its standard output.
    fn stdout(&mut self, bytes: &[u8]);
    /// The module wrote to its standard error.
    fn stderr(&mut self, bytes: &[u8]);
}

/// A path argument: a string in memory, relative to directory `fd`.
#[derive(Debug, Clone, Copy)]
pub struct PathArg {
    pub fd: u32,
    pub ptr: u32,
    pub len: u32,
}

/// The arguments of `path_open` the host uses.
#[derive(Debug, Clone, Copy)]
pub struct PathOpen {
    pub path: PathArg,
    pub oflags: u16,
    pub fdflags: u16,
    /// Where the new descriptor is written.
    pub at: u32,
}

enum Fd {
    Stdin,
    Stdout,
    Stderr,
    Open { node: Node, pos: u64, append: bool, preopen: Option<String>, path: String },
}

/// The host of one run of the module.
pub struct Wasi<P> {
    pub fs: FileSystem,
    pub platform: P,
    args: Vec<String>,
    env: Vec<String>,
    fds: BTreeMap<u32, Fd>,
    next_fd: u32,
}

fn put_u32(m: &mut dyn Memory, at: u32, v: u32) -> Result<(), Stop> {
    m.write(at, &v.to_le_bytes()).then_some(()).ok_or(Stop::Fault)
}

fn put_u64(m: &mut dyn Memory, at: u32, v: u64) -> Result<(), Stop> {
    m.write(at, &v.to_le_bytes()).then_some(()).ok_or(Stop::Fault)
}

fn put(m: &mut dyn Memory, at: u32, data: &[u8]) -> Result<(), Stop> {
    m.write(at, data).then_some(()).ok_or(Stop::Fault)
}

fn get(m: &dyn Memory, at: u32, len: u32) -> Result<Vec<u8>, Stop> {
    let mut out = vec![0; len as usize];
    m.read(at, &mut out).then_some(out).ok_or(Stop::Fault)
}

fn get_u32(m: &dyn Memory, at: u32) -> Result<u32, Stop> {
    let mut b = [0; 4];
    m.read(at, &mut b).then_some(u32::from_le_bytes(b)).ok_or(Stop::Fault)
}

fn get_u64(m: &dyn Memory, at: u32) -> Result<u64, Stop> {
    let mut b = [0; 8];
    m.read(at, &mut b).then_some(u64::from_le_bytes(b)).ok_or(Stop::Fault)
}

fn iovecs(m: &dyn Memory, ptr: u32, len: u32) -> Result<Vec<(u32, u32)>, Stop> {
    (0..len).map(|i| Ok((get_u32(m, ptr + 8 * i)?, get_u32(m, ptr + 8 * i + 4)?))).collect()
}

/// The `/`-separated absolute form of `parts`.
fn join(parts: &[&str]) -> String {
    let mut s = String::new();
    for p in parts {
        s.push('/');
        s.push_str(p);
    }
    if s.is_empty() { "/".into() } else { s }
}

impl<P: Platform> Wasi<P> {
    /// A host for a run with these arguments (the first is the program's
    /// name) and environment.
    pub fn new(fs: FileSystem, platform: P, args: Vec<String>, env: Vec<(String, String)>) -> Self {
        let mut fds = BTreeMap::new();
        fds.insert(0, Fd::Stdin);
        fds.insert(1, Fd::Stdout);
        fds.insert(2, Fd::Stderr);
        fds.insert(3, Fd::Open { node: Node::Dir(fs.root.clone()), pos: 0, append: false, preopen: Some("/".into()), path: "/".into() });
        let env = env.into_iter().map(|(k, v)| format!("{k}={v}")).collect();
        Wasi { fs, platform, args, env, fds, next_fd: 4 }
    }

    /// The first descriptor not handed out yet. A run resumed from a memory
    /// image starts above the one the image was taken at
    /// ([`Wasi::skip_fds_below`]), since the image remembers those as open.
    pub fn next_fd(&self) -> u32 {
        self.next_fd
    }

    pub fn skip_fds_below(&mut self, fd: u32) {
        self.next_fd = self.next_fd.max(fd);
    }

    fn open(&self, fd: u32) -> Result<&Fd, Errno> {
        self.fds.get(&fd).ok_or(errno::BADF)
    }

    /// The bytes of a file, loaded now if it was not.
    fn bytes(&mut self, file: &Rc<RefCell<File>>) -> Result<(), Stop> {
        let (size, source) = match &file.borrow().data {
            Data::Bytes(_) => return Ok(()),
            Data::Lazy { size, source } => (*size, source.clone()),
        };
        match self.platform.load(&source, size) {
            Some(bytes) => {
                file.borrow_mut().data = Data::Bytes(bytes);
                Ok(())
            }
            None => Err(Stop::Fetch(source)),
        }
    }

    /// The absolute path of `path` (in memory) relative to directory `fd`.
    fn resolve(&self, m: &dyn Memory, fd: u32, ptr: u32, len: u32) -> Result<Result<String, Errno>, Stop> {
        let base = match self.fds.get(&fd) {
            Some(Fd::Open { node: Node::Dir(_), path, .. }) => path.clone(),
            _ => return Ok(Err(errno::BADF)),
        };
        let raw = get(m, ptr, len)?;
        let Ok(rel) = String::from_utf8(raw) else { return Ok(Err(errno::INVAL)) };
        let full = if rel.starts_with('/') { rel } else { format!("{base}/{rel}") };
        Ok(Ok(join(&split(&full))))
    }

    fn filestat(&self, m: &mut dyn Memory, at: u32, node: &Node) -> Result<(), Stop> {
        let mut b = [0u8; 64];
        b[16] = if node.is_dir() { FILETYPE_DIRECTORY } else { FILETYPE_REGULAR };
        b[24..32].copy_from_slice(&1u64.to_le_bytes());
        if let Node::File(f) = node {
            b[32..40].copy_from_slice(&f.borrow().size().to_le_bytes());
        }
        let t = node.mtime().to_le_bytes();
        for at in [40, 48, 56] {
            b[at..at + 8].copy_from_slice(&t);
        }
        put(m, at, &b)
    }

    fn strings(list: &[String], m: &mut dyn Memory, ptrs: u32, buf: u32) -> Outcome {
        let mut at = buf;
        for (i, s) in list.iter().enumerate() {
            put_u32(m, ptrs + 4 * i as u32, at)?;
            put(m, at, s.as_bytes())?;
            put(m, at + s.len() as u32, &[0])?;
            at += s.len() as u32 + 1;
        }
        Ok(errno::SUCCESS)
    }

    fn sizes(list: &[String], m: &mut dyn Memory, count: u32, size: u32) -> Outcome {
        put_u32(m, count, list.len() as u32)?;
        put_u32(m, size, list.iter().map(|s| s.len() as u32 + 1).sum())?;
        Ok(errno::SUCCESS)
    }

    pub fn args_get(&mut self, m: &mut dyn Memory, ptrs: u32, buf: u32) -> Outcome {
        Self::strings(&self.args, m, ptrs, buf)
    }

    pub fn args_sizes_get(&mut self, m: &mut dyn Memory, count: u32, size: u32) -> Outcome {
        Self::sizes(&self.args, m, count, size)
    }

    pub fn environ_get(&mut self, m: &mut dyn Memory, ptrs: u32, buf: u32) -> Outcome {
        Self::strings(&self.env, m, ptrs, buf)
    }

    pub fn environ_sizes_get(&mut self, m: &mut dyn Memory, count: u32, size: u32) -> Outcome {
        Self::sizes(&self.env, m, count, size)
    }

    pub fn clock_time_get(&mut self, m: &mut dyn Memory, id: u32, _precision: u64, at: u32) -> Outcome {
        let ns = if id == 0 { self.platform.realtime_ns() } else { self.platform.monotonic_ns() };
        put_u64(m, at, ns)?;
        Ok(errno::SUCCESS)
    }

    pub fn random_get(&mut self, m: &mut dyn Memory, at: u32, len: u32) -> Outcome {
        let mut buf = vec![0; len as usize];
        self.platform.random(&mut buf);
        put(m, at, &buf)?;
        Ok(errno::SUCCESS)
    }

    pub fn sched_yield(&mut self) -> Outcome {
        Ok(errno::SUCCESS)
    }

    pub fn proc_exit(&mut self, code: u32) -> Outcome {
        Err(Stop::Exit(code))
    }

    /// Only clocks are waited for (sleeps), and they are over at once.
    pub fn poll_oneoff(&mut self, m: &mut dyn Memory, subs: u32, events: u32, n: u32, nevents: u32) -> Outcome {
        for i in 0..n {
            let s = subs + 48 * i;
            let e = events + 32 * i;
            let userdata = get_u64(m, s)?;
            let kind = get(m, s + 8, 1)?[0];
            let mut ev = [0u8; 32];
            ev[..8].copy_from_slice(&userdata.to_le_bytes());
            ev[10] = kind;
            put(m, e, &ev)?;
        }
        put_u32(m, nevents, n)?;
        Ok(errno::SUCCESS)
    }

    pub fn fd_prestat_get(&mut self, m: &mut dyn Memory, fd: u32, at: u32) -> Outcome {
        let Some(Fd::Open { preopen: Some(name), .. }) = self.fds.get(&fd) else { return Ok(errno::BADF) };
        let len = name.len() as u32;
        put(m, at, &[0, 0, 0, 0])?;
        put_u32(m, at + 4, len)?;
        Ok(errno::SUCCESS)
    }

    pub fn fd_prestat_dir_name(&mut self, m: &mut dyn Memory, fd: u32, at: u32, len: u32) -> Outcome {
        let Some(Fd::Open { preopen: Some(name), .. }) = self.fds.get(&fd) else { return Ok(errno::BADF) };
        let b = name.as_bytes();
        put(m, at, &b[..b.len().min(len as usize)])?;
        Ok(errno::SUCCESS)
    }

    pub fn fd_fdstat_get(&mut self, m: &mut dyn Memory, fd: u32, at: u32) -> Outcome {
        let (kind, append) = match self.open(fd) {
            Err(e) => return Ok(e),
            Ok(Fd::Open { node, append, .. }) => (if node.is_dir() { FILETYPE_DIRECTORY } else { FILETYPE_REGULAR }, *append),
            Ok(_) => (FILETYPE_CHARACTER_DEVICE, false),
        };
        let mut b = [0u8; 24];
        b[0] = kind;
        b[2..4].copy_from_slice(&(if append { FDFLAGS_APPEND } else { 0 }).to_le_bytes());
        b[8..16].copy_from_slice(&0xffff_ffffu64.to_le_bytes());
        b[16..24].copy_from_slice(&0xffff_ffffu64.to_le_bytes());
        put(m, at, &b)?;
        Ok(errno::SUCCESS)
    }

    pub fn fd_fdstat_set_flags(&mut self, fd: u32, flags: u16) -> Outcome {
        match self.fds.get_mut(&fd) {
            Some(Fd::Open { append, .. }) => *append = flags & FDFLAGS_APPEND != 0,
            Some(_) => {}
            None => return Ok(errno::BADF),
        }
        Ok(errno::SUCCESS)
    }

    pub fn fd_filestat_get(&mut self, m: &mut dyn Memory, fd: u32, at: u32) -> Outcome {
        match self.open(fd) {
            Err(e) => Ok(e),
            Ok(Fd::Open { node, .. }) => {
                let node = node.clone();
                self.filestat(m, at, &node)?;
                Ok(errno::SUCCESS)
            }
            Ok(_) => {
                let mut b = [0u8; 64];
                b[16] = FILETYPE_CHARACTER_DEVICE;
                put(m, at, &b)?;
                Ok(errno::SUCCESS)
            }
        }
    }

    /// Only the modification time is kept (retex's figure cache trims by
    /// age).
    pub fn fd_filestat_set_times(&mut self, fd: u32, _atim: u64, mtim: u64, flags: u16) -> Outcome {
        let now = self.platform.realtime_ns();
        let Fd::Open { node, .. } = (match self.open(fd) {
            Err(e) => return Ok(e),
            Ok(f) => f,
        }) else {
            return Ok(errno::SUCCESS);
        };
        let t = if flags & FSTFLAGS_MTIM_NOW != 0 {
            now
        } else if flags & FSTFLAGS_MTIM != 0 {
            mtim
        } else {
            return Ok(errno::SUCCESS);
        };
        match node {
            Node::File(f) => f.borrow_mut().mtime = t,
            Node::Dir(d) => d.borrow_mut().mtime = t,
        }
        Ok(errno::SUCCESS)
    }

    pub fn fd_filestat_set_size(&mut self, fd: u32, size: u64) -> Outcome {
        let file = match self.open(fd) {
            Ok(Fd::Open { node: Node::File(f), .. }) => f.clone(),
            Ok(_) => return Ok(errno::BADF),
            Err(e) => return Ok(e),
        };
        self.bytes(&file)?;
        let now = self.platform.realtime_ns();
        let mut f = file.borrow_mut();
        if let Data::Bytes(b) = &mut f.data {
            b.resize(size as usize, 0);
        }
        f.mtime = now;
        Ok(errno::SUCCESS)
    }

    pub fn fd_close(&mut self, fd: u32) -> Outcome {
        match self.fds.get(&fd) {
            None | Some(Fd::Open { preopen: Some(_), .. }) => Ok(errno::BADF),
            Some(_) => {
                self.fds.remove(&fd);
                Ok(errno::SUCCESS)
            }
        }
    }

    pub fn fd_renumber(&mut self, from: u32, to: u32) -> Outcome {
        match self.fds.remove(&from) {
            None => Ok(errno::BADF),
            Some(f) => {
                self.fds.insert(to, f);
                Ok(errno::SUCCESS)
            }
        }
    }

    pub fn fd_read(&mut self, m: &mut dyn Memory, fd: u32, iovs: u32, len: u32, nread: u32) -> Outcome {
        let (file, mut pos) = match self.open(fd) {
            Err(e) => return Ok(e),
            Ok(Fd::Open { node: Node::File(f), pos, .. }) => (f.clone(), *pos),
            Ok(Fd::Open { .. }) => return Ok(errno::ISDIR),
            Ok(_) => {
                put_u32(m, nread, 0)?;
                return Ok(errno::SUCCESS);
            }
        };
        self.bytes(&file)?;
        let mut total = 0u32;
        {
            let f = file.borrow();
            let Data::Bytes(data) = &f.data else { return Ok(errno::INVAL) };
            for (ptr, n) in iovecs(m, iovs, len)? {
                let start = (pos as usize).min(data.len());
                let end = (start + n as usize).min(data.len());
                put(m, ptr, &data[start..end])?;
                let k = (end - start) as u32;
                pos += u64::from(k);
                total += k;
                if k < n {
                    break;
                }
            }
        }
        if let Some(Fd::Open { pos: p, .. }) = self.fds.get_mut(&fd) {
            *p = pos;
        }
        put_u32(m, nread, total)?;
        Ok(errno::SUCCESS)
    }

    pub fn fd_write(&mut self, m: &mut dyn Memory, fd: u32, iovs: u32, len: u32, nwritten: u32) -> Outcome {
        let chunks: Vec<Vec<u8>> = iovecs(m, iovs, len)?.into_iter().map(|(p, n)| get(m, p, n)).collect::<Result<_, _>>()?;
        let total: u32 = chunks.iter().map(|c| c.len() as u32).sum();
        let target = match self.open(fd) {
            Err(e) => return Ok(e),
            Ok(Fd::Stdout) => {
                chunks.iter().for_each(|c| self.platform.stdout(c));
                None
            }
            Ok(Fd::Stderr) => {
                chunks.iter().for_each(|c| self.platform.stderr(c));
                None
            }
            Ok(Fd::Stdin) | Ok(Fd::Open { node: Node::Dir(_), .. }) => return Ok(errno::BADF),
            Ok(Fd::Open { node: Node::File(f), pos, append, .. }) => Some((f.clone(), *pos, *append)),
        };
        if let Some((file, mut pos, append)) = target {
            self.bytes(&file)?;
            let now = self.platform.realtime_ns();
            {
                let mut f = file.borrow_mut();
                if let Data::Bytes(data) = &mut f.data {
                    if append {
                        pos = data.len() as u64;
                    }
                    for c in &chunks {
                        let end = pos as usize + c.len();
                        if end > data.len() {
                            data.resize(end, 0);
                        }
                        data[pos as usize..end].copy_from_slice(c);
                        pos = end as u64;
                    }
                }
                f.mtime = now;
            }
            if let Some(Fd::Open { pos: p, .. }) = self.fds.get_mut(&fd) {
                *p = pos;
            }
        }
        put_u32(m, nwritten, total)?;
        Ok(errno::SUCCESS)
    }

    pub fn fd_seek(&mut self, m: &mut dyn Memory, fd: u32, offset: i64, whence: u8, at: u32) -> Outcome {
        let Some(Fd::Open { node: Node::File(f), pos, .. }) = self.fds.get_mut(&fd) else {
            return Ok(errno::BADF);
        };
        let base = match whence {
            0 => 0,
            1 => *pos as i64,
            2 => f.borrow().size() as i64,
            _ => return Ok(errno::INVAL),
        };
        let Some(new) = base.checked_add(offset).filter(|p| *p >= 0) else { return Ok(errno::INVAL) };
        *pos = new as u64;
        put_u64(m, at, new as u64)?;
        Ok(errno::SUCCESS)
    }

    pub fn fd_readdir(&mut self, m: &mut dyn Memory, fd: u32, buf: u32, len: u32, cookie: u64, used: u32) -> Outcome {
        let dir = match self.open(fd) {
            Ok(Fd::Open { node: Node::Dir(d), .. }) => d.clone(),
            Ok(_) => return Ok(errno::NOTDIR),
            Err(e) => return Ok(e),
        };
        let mut out = Vec::new();
        for (i, (name, node)) in dir.borrow().children.iter().enumerate().skip(cookie as usize) {
            let mut entry = Vec::with_capacity(24 + name.len());
            entry.extend_from_slice(&(i as u64 + 1).to_le_bytes());
            entry.extend_from_slice(&(i as u64 + 1).to_le_bytes());
            entry.extend_from_slice(&(name.len() as u32).to_le_bytes());
            entry.push(if node.is_dir() { FILETYPE_DIRECTORY } else { FILETYPE_REGULAR });
            entry.extend_from_slice(&[0, 0, 0]);
            entry.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&entry);
            if out.len() >= len as usize {
                break;
            }
        }
        out.truncate(len as usize);
        put(m, buf, &out)?;
        put_u32(m, used, out.len() as u32)?;
        Ok(errno::SUCCESS)
    }

    /// `path_open`, with its arguments that matter here (the rights are
    /// not checked).
    pub fn path_open(&mut self, m: &mut dyn Memory, open: PathOpen) -> Outcome {
        let PathOpen { path: PathArg { fd: dirfd, ptr, len }, oflags, fdflags, at } = open;
        let path = match self.resolve(m, dirfd, ptr, len)? {
            Ok(p) => p,
            Err(e) => return Ok(e),
        };
        let now = self.platform.realtime_ns();
        let node = match self.fs.lookup(&path) {
            Some(_) if oflags & OFLAGS_CREAT != 0 && oflags & OFLAGS_EXCL != 0 => return Ok(errno::EXIST),
            Some(n) => n,
            None if oflags & OFLAGS_CREAT == 0 => return Ok(errno::NOENT),
            None => {
                let parts = split(&path);
                let Some((name, parents)) = parts.split_last() else { return Ok(errno::EXIST) };
                let Some(Node::Dir(parent)) = self.fs.lookup_parts(parents) else { return Ok(errno::NOENT) };
                let file = Rc::new(RefCell::new(File { data: Data::Bytes(Vec::new()), mtime: now }));
                parent.borrow_mut().children.insert((*name).to_owned(), Node::File(file.clone()));
                Node::File(file)
            }
        };
        if oflags & OFLAGS_DIRECTORY != 0 && !node.is_dir() {
            return Ok(errno::NOTDIR);
        }
        if oflags & OFLAGS_TRUNC != 0
            && let Node::File(f) = &node
        {
            let mut f = f.borrow_mut();
            f.data = Data::Bytes(Vec::new());
            f.mtime = now;
        }
        let fd = self.next_fd;
        self.next_fd += 1;
        self.fds.insert(fd, Fd::Open { node, pos: 0, append: fdflags & FDFLAGS_APPEND != 0, preopen: None, path });
        put_u32(m, at, fd)?;
        Ok(errno::SUCCESS)
    }

    pub fn path_filestat_get(&mut self, m: &mut dyn Memory, fd: u32, _flags: u32, ptr: u32, len: u32, at: u32) -> Outcome {
        let path = match self.resolve(m, fd, ptr, len)? {
            Ok(p) => p,
            Err(e) => return Ok(e),
        };
        match self.fs.lookup(&path) {
            None => Ok(errno::NOENT),
            Some(node) => {
                self.filestat(m, at, &node)?;
                Ok(errno::SUCCESS)
            }
        }
    }

    pub fn path_create_directory(&mut self, m: &mut dyn Memory, fd: u32, ptr: u32, len: u32) -> Outcome {
        let path = match self.resolve(m, fd, ptr, len)? {
            Ok(p) => p,
            Err(e) => return Ok(e),
        };
        if self.fs.exists(&path) {
            return Ok(errno::EXIST);
        }
        let now = self.platform.realtime_ns();
        Ok(if self.fs.mkdirs(&path, now) { errno::SUCCESS } else { errno::NOTDIR })
    }

    pub fn path_remove_directory(&mut self, m: &mut dyn Memory, fd: u32, ptr: u32, len: u32) -> Outcome {
        let path = match self.resolve(m, fd, ptr, len)? {
            Ok(p) => p,
            Err(e) => return Ok(e),
        };
        match self.fs.lookup(&path) {
            None => Ok(errno::NOENT),
            Some(Node::File(_)) => Ok(errno::NOTDIR),
            Some(Node::Dir(d)) if !d.borrow().children.is_empty() => Ok(errno::NOTEMPTY),
            Some(Node::Dir(_)) => {
                self.fs.remove(&path);
                Ok(errno::SUCCESS)
            }
        }
    }

    pub fn path_unlink_file(&mut self, m: &mut dyn Memory, fd: u32, ptr: u32, len: u32) -> Outcome {
        let path = match self.resolve(m, fd, ptr, len)? {
            Ok(p) => p,
            Err(e) => return Ok(e),
        };
        match self.fs.lookup(&path) {
            None => Ok(errno::NOENT),
            Some(Node::Dir(_)) => Ok(errno::ISDIR),
            Some(Node::File(_)) => {
                self.fs.remove(&path);
                Ok(errno::SUCCESS)
            }
        }
    }

    pub fn path_rename(&mut self, m: &mut dyn Memory, from: PathArg, to: PathArg) -> Outcome {
        let from = match self.resolve(m, from.fd, from.ptr, from.len)? {
            Ok(p) => p,
            Err(e) => return Ok(e),
        };
        let to = match self.resolve(m, to.fd, to.ptr, to.len)? {
            Ok(p) => p,
            Err(e) => return Ok(e),
        };
        let Some(node) = self.fs.lookup(&from) else { return Ok(errno::NOENT) };
        let parts = split(&to);
        let Some((name, parents)) = parts.split_last() else { return Ok(errno::INVAL) };
        let Some(Node::Dir(parent)) = self.fs.lookup_parts(parents) else { return Ok(errno::NOENT) };
        self.fs.remove(&from);
        parent.borrow_mut().children.insert((*name).to_owned(), node);
        Ok(errno::SUCCESS)
    }

    pub fn path_readlink(&mut self) -> Outcome {
        Ok(errno::INVAL)
    }

    pub fn path_link(&mut self) -> Outcome {
        Ok(errno::NOTSUP)
    }
}

#[cfg(test)]
mod tests;

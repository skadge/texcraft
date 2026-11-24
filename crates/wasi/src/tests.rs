use super::*;
use std::collections::HashMap;

#[derive(Default)]
struct Fake {
    sources: HashMap<String, Vec<u8>>,
    loads: Vec<String>,
    out: Vec<u8>,
}

impl Platform for Fake {
    fn realtime_ns(&self) -> u64 {
        1_000
    }
    fn monotonic_ns(&self) -> u64 {
        1
    }
    fn random(&mut self, buf: &mut [u8]) {
        buf.fill(7);
    }
    fn load(&mut self, source: &str, _size: u64) -> Option<Vec<u8>> {
        self.loads.push(source.to_owned());
        self.sources.get(source).cloned()
    }
    fn stdout(&mut self, bytes: &[u8]) {
        self.out.extend_from_slice(bytes);
    }
    fn stderr(&mut self, _bytes: &[u8]) {}
}

/// A host over `fs` with a memory of 64 KiB; strings and buffers go above
/// 1 KiB, the results the calls write below it.
struct Run {
    wasi: Wasi<Fake>,
    m: Vec<u8>,
    next: u32,
}

const OUT: u32 = 16;

impl Run {
    fn new(fs: FileSystem, fake: Fake) -> Run {
        Run { wasi: Wasi::new(fs, fake, vec!["retex".into(), "doc.tex".into()], vec![]), m: vec![0; 65536], next: 1024 }
    }

    fn place(&mut self, data: &[u8]) -> (u32, u32) {
        let at = self.next;
        self.m[at as usize..at as usize + data.len()].copy_from_slice(data);
        self.next += data.len() as u32 + 8;
        (at, data.len() as u32)
    }

    fn open(&mut self, path: &str, oflags: u16, fdflags: u16) -> Result<u32, Errno> {
        let (ptr, len) = self.place(path.as_bytes());
        let open = PathOpen { path: PathArg { fd: 3, ptr, len }, oflags, fdflags, at: OUT };
        match self.wasi.path_open(&mut self.m, open).unwrap() {
            errno::SUCCESS => Ok(get_u32(&self.m, OUT).unwrap()),
            e => Err(e),
        }
    }

    fn write(&mut self, fd: u32, data: &[u8]) -> Errno {
        let (ptr, len) = self.place(data);
        let (iov, _) = self.place(&[ptr.to_le_bytes(), len.to_le_bytes()].concat());
        self.wasi.fd_write(&mut self.m, fd, iov, 1, OUT).unwrap()
    }

    fn read(&mut self, fd: u32, n: u32) -> Result<Vec<u8>, Stop> {
        let (buf, _) = self.place(&vec![0; n as usize]);
        let (iov, _) = self.place(&[buf.to_le_bytes(), n.to_le_bytes()].concat());
        assert_eq!(self.wasi.fd_read(&mut self.m, fd, iov, 1, OUT)?, errno::SUCCESS);
        let got = get_u32(&self.m, OUT).unwrap();
        Ok(self.m[buf as usize..(buf + got) as usize].to_vec())
    }
}

#[test]
fn a_written_file_reads_back_and_is_visible_to_the_host() {
    let fs = FileSystem::new();
    let mut r = Run::new(fs.clone(), Fake::default());
    let fd = r.open("out/doc.log", OFLAGS_CREAT, 0);
    assert_eq!(fd, Err(errno::NOENT), "no directory, no file");
    r.wasi.fs.mkdirs("/out", 0);
    let fd = r.open("out/doc.log", OFLAGS_CREAT, 0).unwrap();
    assert_eq!(r.write(fd, b"This is retex"), errno::SUCCESS);
    r.wasi.fd_close(fd).unwrap();
    let fd = r.open("/out/./doc.log", 0, 0).unwrap();
    assert_eq!(r.read(fd, 100).unwrap(), b"This is retex");
    assert_eq!(fs.read("/out/doc.log").unwrap(), b"This is retex");
}

#[test]
fn a_tree_file_is_loaded_when_read_and_stops_the_run_when_it_cannot_be() {
    let fs = FileSystem::new();
    fs.add_lazy("/texlive/article.cls", 5, "tree/article.cls".into());
    let mut r = Run::new(fs.clone(), Fake::default());
    // Its size is known without loading it.
    let (ptr, len) = r.place(b"/texlive/article.cls");
    r.wasi.path_filestat_get(&mut r.m, 3, 0, ptr, len, 512).unwrap();
    assert_eq!(get_u64(&r.m, 512 + 32).unwrap(), 5);
    assert!(r.wasi.platform.loads.is_empty());
    let fd = r.open("/texlive/article.cls", 0, 0).unwrap();
    assert_eq!(r.read(fd, 10), Err(Stop::Fetch("tree/article.cls".into())));

    // Once the host has fetched it, the next run reads it.
    let mut fake = Fake::default();
    fake.sources.insert("tree/article.cls".into(), b"\\cls!".to_vec());
    let mut r = Run::new(fs.clone(), fake);
    let fd = r.open("/texlive/article.cls", 0, 0).unwrap();
    assert_eq!(r.read(fd, 10).unwrap(), b"\\cls!");
    let fd = r.open("/texlive/article.cls", 0, 0).unwrap();
    r.read(fd, 10).unwrap();
    assert_eq!(r.wasi.platform.loads.len(), 1, "loaded once");
}

#[test]
fn a_directory_listed_in_small_steps_gives_every_entry_once() {
    let fs = FileSystem::new();
    let names: Vec<String> = (0..20).map(|i| format!("file-{i:02}.sty")).collect();
    for n in &names {
        fs.add(&format!("/tex/{n}"), vec![], 0);
    }
    let mut r = Run::new(fs, Fake::default());
    let fd = r.open("/tex", OFLAGS_DIRECTORY, 0).unwrap();
    let mut seen = Vec::new();
    let mut cookie = 0u64;
    loop {
        // As wasi-libc does: read a buffer, take the whole entries, go on
        // from the last one's cookie.
        r.wasi.fd_readdir(&mut r.m, fd, 2048, 100, cookie, OUT).unwrap();
        let used = get_u32(&r.m, OUT).unwrap() as usize;
        let buf = r.m[2048..2048 + used].to_vec();
        let mut at = 0;
        let mut whole = 0;
        while at + 24 <= buf.len() {
            let namlen = u32::from_le_bytes(buf[at + 16..at + 20].try_into().unwrap()) as usize;
            if at + 24 + namlen > buf.len() {
                break;
            }
            seen.push(String::from_utf8(buf[at + 24..at + 24 + namlen].to_vec()).unwrap());
            cookie = u64::from_le_bytes(buf[at..at + 8].try_into().unwrap());
            at += 24 + namlen;
            whole += 1;
        }
        if used < 100 {
            break;
        }
        assert!(whole > 0, "a buffer this size holds an entry");
    }
    assert_eq!(seen, names);
}

#[test]
fn appending_writes_at_the_end_and_truncating_empties() {
    let fs = FileSystem::new();
    fs.add("/a.aux", b"one".to_vec(), 0);
    let mut r = Run::new(fs.clone(), Fake::default());
    let fd = r.open("/a.aux", 0, FDFLAGS_APPEND).unwrap();
    r.write(fd, b"two");
    assert_eq!(fs.read("/a.aux").unwrap(), b"onetwo");
    let fd = r.open("/a.aux", OFLAGS_TRUNC, 0).unwrap();
    r.write(fd, b"3");
    assert_eq!(fs.read("/a.aux").unwrap(), b"3");
}

#[test]
fn creating_exclusively_refuses_an_existing_file() {
    let fs = FileSystem::new();
    fs.add("/lock", vec![], 0);
    let mut r = Run::new(fs, Fake::default());
    assert_eq!(r.open("/lock", OFLAGS_CREAT | OFLAGS_EXCL, 0), Err(errno::EXIST));
    assert!(r.open("/new", OFLAGS_CREAT | OFLAGS_EXCL, 0).is_ok());
}

#[test]
fn an_unlinked_file_stays_readable_through_its_descriptor() {
    let fs = FileSystem::new();
    fs.add("/tmp.pdf", b"%PDF".to_vec(), 0);
    let mut r = Run::new(fs.clone(), Fake::default());
    let fd = r.open("/tmp.pdf", 0, 0).unwrap();
    let (ptr, len) = r.place(b"/tmp.pdf");
    assert_eq!(r.wasi.path_unlink_file(&mut r.m, 3, ptr, len).unwrap(), errno::SUCCESS);
    assert!(!fs.exists("/tmp.pdf"));
    assert_eq!(r.read(fd, 4).unwrap(), b"%PDF");
}

#[test]
fn dot_dot_stays_under_the_root() {
    let fs = FileSystem::new();
    fs.add("/doc.tex", b"x".to_vec(), 0);
    let mut r = Run::new(fs, Fake::default());
    let fd = r.open("../../doc.tex", 0, 0).unwrap();
    assert_eq!(r.read(fd, 1).unwrap(), b"x");
}

#[test]
fn the_arguments_fit_the_sizes_announced() {
    let mut r = Run::new(FileSystem::new(), Fake::default());
    r.wasi.args_sizes_get(&mut r.m, OUT, OUT + 4).unwrap();
    let (count, size) = (get_u32(&r.m, OUT).unwrap(), get_u32(&r.m, OUT + 4).unwrap());
    r.wasi.args_get(&mut r.m, 512, 2048).unwrap();
    let last = get_u32(&r.m, 512 + 4 * (count - 1)).unwrap();
    let end = r.m[last as usize..].iter().position(|b| *b == 0).unwrap() as u32 + last + 1;
    assert_eq!(end - 2048, size);
    assert_eq!(&r.m[last as usize..end as usize - 1], b"doc.tex");
}

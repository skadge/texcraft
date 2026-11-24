//! The host in the browser: a [`Wasi`] handed to `WebAssembly.Instance` as the module's
//! `wasi_snapshot_preview1` imports, with retex's own `retex.snapshot` import, and the runs of
//! the module's `_start` and `retex_resume`.
//!
//! An import ends the run by storing its [`Stop`] and throwing a JavaScript exception through
//! the module, as retex's JavaScript host does at `proc_exit`. Nothing is borrowed when it
//! throws, since no destructor runs on that path.
//!
//! wasm-bindgen's closures take at most 8 arguments and `path_open` has 9, so `path_open` is a
//! `Proxy` of a function whose `apply` trap gets the arguments as an array.

use std::cell::RefCell;
use std::rc::Rc;

use js_sys::{Array, Function, Object, Proxy, Reflect, Uint8Array, WebAssembly};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::{Memory, PathArg, PathOpen, Platform, Stop, Wasi};

/// The module's memory, read and written through a fresh view each time (it moves when it
/// grows).
struct JsMemory(WebAssembly::Memory);

impl JsMemory {
    fn view(&self) -> Uint8Array {
        Uint8Array::new(&self.0.buffer())
    }
}

impl Memory for JsMemory {
    fn read(&self, at: u32, out: &mut [u8]) -> bool {
        let view = self.view();
        let end = u64::from(at) + out.len() as u64;
        if end > u64::from(view.length()) {
            return false;
        }
        view.subarray(at, end as u32).copy_to(out);
        true
    }

    fn write(&mut self, at: u32, data: &[u8]) -> bool {
        let view = self.view();
        let end = u64::from(at) + data.len() as u64;
        if end > u64::from(view.length()) {
            return false;
        }
        view.subarray(at, end as u32).copy_from(data);
        true
    }
}

/// A memory image of the module, taken at retex's `snapshot` call (the preamble done), and the
/// first descriptor its run had not handed out.
#[derive(Clone)]
pub struct Image {
    pub bytes: Uint8Array,
    pub next_fd: u32,
}

/// How a run of the module ended.
#[derive(Debug)]
pub enum Ended {
    /// The module returned (status 0) or called `proc_exit`.
    Exit(u32),
    /// A file has to be fetched first.
    Fetch(Rc<str>),
    /// The module failed in a way WASI does not report (a trap, a stack overflow): the message.
    Crashed(String),
}

struct Shared<P> {
    wasi: RefCell<Wasi<P>>,
    memory: RefCell<Option<WebAssembly::Memory>>,
    stop: RefCell<Option<Stop>>,
    image: RefCell<Option<Image>>,
    /// Whether `retex.snapshot` keeps an image (not for exports to another format).
    snapshots: bool,
}

impl<P: Platform> Shared<P> {
    /// Runs a call with the module's memory; returns its error number, or stores the stop and
    /// throws.
    fn call(&self, f: impl FnOnce(&mut Wasi<P>, &mut dyn Memory) -> crate::Outcome) -> u32 {
        let result = {
            let Some(memory) = self.memory.borrow().clone() else { return u32::from(crate::errno::FAULT) };
            let mut memory = JsMemory(memory);
            let mut wasi = self.wasi.borrow_mut();
            f(&mut wasi, &mut memory)
        };
        match result {
            Ok(errno) => u32::from(errno),
            Err(stop) => {
                *self.stop.borrow_mut() = Some(stop);
                wasm_bindgen::throw_str("texcraft-wasi: the run stops")
            }
        }
    }
}

/// An instance of the module with its host, ready to run once.
pub struct Instance<P> {
    shared: Rc<Shared<P>>,
    instance: WebAssembly::Instance,
    /// The imports' closures, alive as long as the instance.
    _keep: Vec<JsValue>,
}

fn set(obj: &Object, name: &str, f: &JsValue) {
    // Setting a property of a plain object does not fail.
    let _ = Reflect::set(obj, &JsValue::from_str(name), f);
}

macro_rules! import {
    ($obj:expr, $keep:expr, $shared:expr, $name:literal, |$w:ident, $m:ident $(, $a:ident: $t:ty)*| $body:expr) => {{
        let s = $shared.clone();
        let f = Closure::<dyn FnMut($($t),*) -> u32>::new(move |$($a: $t),*| s.call(|$w, $m| $body));
        set(&$obj, $name, f.as_ref());
        $keep.push(f.into_js_value());
    }};
}

/// An argument of a `Proxy` call as a `u32` (a WebAssembly `i32`).
fn arg_u32(args: &Array, i: u32) -> u32 {
    args.get(i).as_f64().map_or(0, |n| n as i64 as u32)
}

/// An argument as a `u64` (a WebAssembly `i64`, a `BigInt`).
fn arg_u64(args: &Array, i: u32) -> u64 {
    i64::try_from(args.get(i)).map_or(0, |n| n as u64)
}

impl<P: Platform + 'static> Instance<P> {
    /// Instantiates `module` over `wasi`. With `snapshots`, retex's `snapshot` call keeps a
    /// memory image ([`Instance::image`]).
    pub fn new(module: &WebAssembly::Module, wasi: Wasi<P>, snapshots: bool) -> Result<Instance<P>, JsValue> {
        let shared =
            Rc::new(Shared { wasi: RefCell::new(wasi), memory: RefCell::new(None), stop: RefCell::new(None), image: RefCell::new(None), snapshots });
        let mut keep = Vec::new();
        let w = Object::new();
        import!(w, keep, shared, "args_get", |h, m, a: u32, b: u32| h.args_get(m, a, b));
        import!(w, keep, shared, "args_sizes_get", |h, m, a: u32, b: u32| h.args_sizes_get(m, a, b));
        import!(w, keep, shared, "environ_get", |h, m, a: u32, b: u32| h.environ_get(m, a, b));
        import!(w, keep, shared, "environ_sizes_get", |h, m, a: u32, b: u32| h.environ_sizes_get(m, a, b));
        import!(w, keep, shared, "clock_time_get", |h, m, id: u32, p: u64, at: u32| h.clock_time_get(m, id, p, at));
        import!(w, keep, shared, "random_get", |h, m, at: u32, len: u32| h.random_get(m, at, len));
        import!(w, keep, shared, "sched_yield", |h, _m| h.sched_yield());
        import!(w, keep, shared, "proc_exit", |h, _m, code: u32| h.proc_exit(code));
        import!(w, keep, shared, "poll_oneoff", |h, m, s: u32, e: u32, n: u32, out: u32| h.poll_oneoff(m, s, e, n, out));
        import!(w, keep, shared, "fd_prestat_get", |h, m, fd: u32, at: u32| h.fd_prestat_get(m, fd, at));
        import!(w, keep, shared, "fd_prestat_dir_name", |h, m, fd: u32, at: u32, len: u32| h.fd_prestat_dir_name(m, fd, at, len));
        import!(w, keep, shared, "fd_fdstat_get", |h, m, fd: u32, at: u32| h.fd_fdstat_get(m, fd, at));
        import!(w, keep, shared, "fd_fdstat_set_flags", |h, _m, fd: u32, flags: u32| h.fd_fdstat_set_flags(fd, flags as u16));
        import!(w, keep, shared, "fd_filestat_get", |h, m, fd: u32, at: u32| h.fd_filestat_get(m, fd, at));
        import!(w, keep, shared, "fd_filestat_set_times", |h, _m, fd: u32, _a: u64, t: u64, flags: u32| h.fd_filestat_set_times(
            fd,
            _a,
            t,
            flags as u16
        ));
        import!(w, keep, shared, "fd_filestat_set_size", |h, _m, fd: u32, size: u64| h.fd_filestat_set_size(fd, size));
        import!(w, keep, shared, "fd_close", |h, _m, fd: u32| h.fd_close(fd));
        import!(w, keep, shared, "fd_renumber", |h, _m, from: u32, to: u32| h.fd_renumber(from, to));
        import!(w, keep, shared, "fd_read", |h, m, fd: u32, iovs: u32, n: u32, out: u32| h.fd_read(m, fd, iovs, n, out));
        import!(w, keep, shared, "fd_write", |h, m, fd: u32, iovs: u32, n: u32, out: u32| h.fd_write(m, fd, iovs, n, out));
        import!(w, keep, shared, "fd_seek", |h, m, fd: u32, off: i64, whence: u32, at: u32| h.fd_seek(m, fd, off, whence as u8, at));
        import!(w, keep, shared, "fd_readdir", |h, m, fd: u32, buf: u32, len: u32, cookie: u64, out: u32| h.fd_readdir(m, fd, buf, len, cookie, out));
        import!(w, keep, shared, "path_filestat_get", |h, m, fd: u32, fl: u32, p: u32, l: u32, at: u32| h.path_filestat_get(m, fd, fl, p, l, at));
        import!(w, keep, shared, "path_create_directory", |h, m, fd: u32, p: u32, l: u32| h.path_create_directory(m, fd, p, l));
        import!(w, keep, shared, "path_remove_directory", |h, m, fd: u32, p: u32, l: u32| h.path_remove_directory(m, fd, p, l));
        import!(w, keep, shared, "path_unlink_file", |h, m, fd: u32, p: u32, l: u32| h.path_unlink_file(m, fd, p, l));
        import!(w, keep, shared, "path_rename", |h, m, a: u32, b: u32, c: u32, d: u32, e: u32, f: u32| h.path_rename(
            m,
            PathArg { fd: a, ptr: b, len: c },
            PathArg { fd: d, ptr: e, len: f }
        ));
        import!(w, keep, shared, "path_readlink", |h, _m, _a: u32, _b: u32, _c: u32, _d: u32, _e: u32, _f: u32| h.path_readlink());
        import!(w, keep, shared, "path_link", |h, _m, _a: u32, _b: u32, _c: u32, _d: u32, _e: u32, _f: u32, _g: u32| h.path_link());

        // path_open(fd, dirflags, path, path_len, oflags, rights_base, rights_inheriting,
        // fdflags, fd_out): 9 arguments, through a Proxy's `apply`.
        {
            let s = shared.clone();
            let trap = Closure::<dyn FnMut(JsValue, JsValue, Array) -> u32>::new(move |_: JsValue, _: JsValue, a: Array| {
                let open = PathOpen {
                    path: PathArg { fd: arg_u32(&a, 0), ptr: arg_u32(&a, 2), len: arg_u32(&a, 3) },
                    oflags: arg_u32(&a, 4) as u16,
                    fdflags: arg_u32(&a, 7) as u16,
                    at: arg_u32(&a, 8),
                };
                let _rights = (arg_u64(&a, 5), arg_u64(&a, 6));
                s.call(|h, m| h.path_open(m, open))
            });
            let handler = Object::new();
            set(&handler, "apply", trap.as_ref());
            // The target only has to be callable; the trap does the work.
            let target = Closure::<dyn FnMut() -> u32>::new(|| 0).into_js_value();
            let proxy = Proxy::new(&target, &handler);
            set(&w, "path_open", &proxy);
            keep.push(trap.into_js_value());
            keep.push(target);
        }

        let r = Object::new();
        {
            let s = shared.clone();
            let f = Closure::<dyn FnMut()>::new(move || {
                if !s.snapshots {
                    return;
                }
                let Some(memory) = s.memory.borrow().clone() else { return };
                let bytes = Uint8Array::new(&memory.buffer()).slice(0, u32::MAX);
                let next_fd = s.wasi.borrow().next_fd();
                *s.image.borrow_mut() = Some(Image { bytes, next_fd });
            });
            set(&r, "snapshot", f.as_ref());
            keep.push(f.into_js_value());
        }

        let imports = Object::new();
        set(&imports, "wasi_snapshot_preview1", &w);
        set(&imports, "retex", &r);
        let instance = WebAssembly::Instance::new(module, &imports)?;
        let memory: WebAssembly::Memory = Reflect::get(&instance.exports(), &"memory".into())?.dyn_into()?;
        *shared.memory.borrow_mut() = Some(memory);
        Ok(Instance { shared, instance, _keep: keep })
    }

    fn export(&self, name: &str) -> Result<Function, String> {
        Reflect::get(&self.instance.exports(), &name.into())
            .ok()
            .and_then(|f| f.dyn_into::<Function>().ok())
            .ok_or_else(|| format!("the module has no `{name}`"))
    }

    fn run(&self, entry: &str) -> Ended {
        let f = match self.export(entry) {
            Ok(f) => f,
            Err(e) => return Ended::Crashed(e),
        };
        match f.call0(&JsValue::NULL) {
            Ok(_) => Ended::Exit(0),
            Err(e) => match self.shared.stop.borrow_mut().take() {
                Some(Stop::Exit(code)) => Ended::Exit(code),
                Some(Stop::Fetch(source)) => Ended::Fetch(source),
                Some(Stop::Fault) => Ended::Crashed("the module gave the host an address outside its memory".into()),
                None => Ended::Crashed(e.dyn_ref::<js_sys::Error>().map_or_else(|| format!("{e:?}"), |e| String::from(e.to_string()))),
            },
        }
    }

    /// Runs the module from its start.
    pub fn start(self) -> (Ended, Self) {
        let ended = self.run("_start");
        (ended, self)
    }

    /// Runs the module from `image` (retex's `retex_resume`): its memory becomes the image, and
    /// the descriptors the image's run had open are not handed out again.
    pub fn resume(self, image: &Image) -> (Ended, Self) {
        let Some(memory) = self.shared.memory.borrow().clone() else {
            return (Ended::Crashed("no memory".into()), self);
        };
        let have = Uint8Array::new(&memory.buffer()).length();
        let need = image.bytes.length();
        if need > have {
            memory.grow((need - have).div_ceil(65536));
        }
        Uint8Array::new(&memory.buffer()).set(&image.bytes, 0);
        self.shared.wasi.borrow_mut().skip_fds_below(image.next_fd);
        let ended = self.run("retex_resume");
        (ended, self)
    }

    /// The image retex's `snapshot` call took during the run, if it made one.
    pub fn image(&self) -> Option<Image> {
        self.shared.image.borrow_mut().take()
    }

    /// The host, after the run (its file system and platform).
    pub fn with_wasi<R>(&self, f: impl FnOnce(&mut Wasi<P>) -> R) -> R {
        f(&mut self.shared.wasi.borrow_mut())
    }
}

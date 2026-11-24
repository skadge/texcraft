//! `cargo xtask tree-site OUT`: the TeX tree of the browser build, so that TeXCraft's site serves
//! its own TeX files instead of retex's site (plan, phase 6). Ported from retex's
//! `xtask/src/wasm/tree.rs` (CC0), which builds the tree of retex's own site the same way.
//!
//! 1. **The files.** The documents `apps/texcraft-web/tree.txt` lists (a class or a package
//!    loaded alone, the blank project, every template of `OUT/templates/`) are typeset with a
//!    native retex of the pinned version and `-recorder`; the packages of the files they read
//!    go in whole (`tex/FORMAT/PACKAGE/`, `fonts/KIND/SUPPLIER/FAMILY/`), from the TeX
//!    installation's trees (`kpsewhich --expand-path '$TEXMF'`) merged in their order. Hence a
//!    TeX Live is needed: CI uses the `texlive/texlive` image, as retex's does.
//! 2. **The tree** goes to `OUT/tree/`: `index.txt` (a line per file: its size, its path in the
//!    browser, and for a small file its pack and offset), the small files of each folder in one
//!    pack (`packs/N`), the others alone (`texlive/texmf-dist/…`), with the tree's own
//!    `texmf.cnf` and `ls-R`.
//! 3. **The seed** (`OUT/seed/`): the indexes retex makes of the tree (`ls-R`, the fonts' names)
//!    and the files the blank project reads (`reads/blank.txt`), so that a first visit neither
//!    scans every font nor fetches files one after the other. retex keys its indexes on the
//!    tree's absolute paths and checks them against the files' sizes and times: they are made
//!    by the native retex with the tree at `/texlive/texmf-dist` and every time 0, as in the
//!    browser (directly when run as root, as in CI; otherwise in a bubblewrap sandbox). retex's
//!    site makes them by running the module in Node, which TeXCraft does not use (no
//!    handwritten JavaScript). The format is left to the browser: it depends on the engine's
//!    build.
//!
//! The native retex is `$RETEX` when set, else built with `cargo install` into
//! `target/retex-native`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where the tree is in the browser's file system.
const ROOT: &str = "/texlive/texmf-dist";

/// The environment the browser's worker gives the module (crates/compile/src/web_engine.rs).
const ENV: [(&str, &str); 7] = [
    ("HOME", "/home"),
    ("RETEX_CACHE", "/cache"),
    ("TEXMFCNF", "/texlive/texmf-dist/web2c"),
    ("SELFAUTOLOC", "/texlive/bin/wasm32-wasi"),
    ("SELFAUTODIR", "/texlive/bin"),
    ("SELFAUTOPARENT", "/texlive"),
    ("SELFAUTOGRANDPARENT", "/"),
];

/// Files up to this size are fetched with the other small files of their folder, in one pack;
/// larger ones (fonts, mostly) alone, as most documents use few of a folder's fonts.
const PACKED: u64 = 64 << 10;

/// The definitions that put the browser's tree in place of the installation's (kpathsea takes
/// the first definition of a variable).
const CNF: &str = "\
% The TeX tree of TeXCraft's browser build (cargo xtask tree-site),
% then the texmf.cnf files of the installation it was taken from.
TEXMFROOT = /texlive
TEXMFDIST = /texlive/texmf-dist
TEXMFMAIN = /texlive/texmf-dist
TEXMFLOCAL = /texlive/texmf-local
TEXMFSYSCONFIG = /texlive/texmf-config
TEXMFSYSVAR = /cache/texmf-var
TEXMFHOME = /home/texmf
TEXMFVAR = /cache/texmf-var
TEXMFCONFIG = /cache/texmf-config
TEXMFCACHE = /cache/texmf-var
TEXMF = {$TEXMFHOME,!!$TEXMFDIST}
TEXMFDBS = $TEXMFDIST
TEXMFCNF = /texlive/texmf-dist/web2c
OSFONTDIR = /texlive/no-system-fonts
";

type Result<T> = std::result::Result<T, String>;

fn io(path: &Path) -> impl Fn(std::io::Error) -> String + '_ {
    move |e| format!("{}: {e}", path.display())
}

pub fn run(root: &Path, out: &Path) -> Result<()> {
    let retex = native_retex(root)?;
    let work = out.join(".tree-work");
    if work.exists() {
        fs::remove_dir_all(&work).map_err(io(&work))?;
    }
    fs::create_dir_all(&work).map_err(io(&work))?;
    eprintln!("the TeX tree of apps/texcraft-web/tree.txt:");
    let tree = select(&root.join("apps/texcraft-web/tree.txt"), &retex, out, &work)?;
    let contents = contents(&tree)?;
    let index = write(&contents, &out.join("tree"))?;
    seed(&contents, &index, &tree.read, &retex, out, &work)?;
    fs::remove_dir_all(&work).map_err(io(&work))?;
    Ok(())
}

/// The native retex: `$RETEX`, or the pinned version built into `target/retex-native`.
fn native_retex(root: &Path) -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("RETEX") {
        return Ok(PathBuf::from(path));
    }
    let version = crate::retex_wasm::pinned_version(root)?;
    let dir = root.join("target/retex-native");
    let bin = dir.join("bin/retex");
    let stamp = dir.join("version.txt");
    if bin.is_file() && fs::read_to_string(&stamp).is_ok_and(|v| v.trim() == version) {
        return Ok(bin);
    }
    let spec = format!("retex@={version}");
    crate::run(env!("CARGO"), &["install", "--locked", "--force", "--root", &dir.display().to_string(), &spec])?;
    fs::write(&stamp, &version).map_err(io(&stamp))?;
    Ok(bin)
}

/// The trees of the installation, in kpathsea's order (not the home folder's).
fn installation_trees() -> Result<Vec<PathBuf>> {
    let out = Command::new("kpsewhich")
        .arg("--expand-path=$TEXMF")
        .output()
        .map_err(|e| format!("cannot run kpsewhich (a TeX installation is needed): {e}"))?;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    Ok(String::from_utf8_lossy(&out.stdout)
        .trim()
        .split(':')
        .map(PathBuf::from)
        .filter(|p| p.is_dir() && !home.as_ref().is_some_and(|h| p.starts_with(h)))
        .collect())
}

/// The installation's `texmf.cnf` files, the first one's definitions winning.
fn installation_cnf() -> Result<String> {
    let out = Command::new("kpsewhich").args(["-all", "texmf.cnf"]).output().map_err(|e| format!("cannot run kpsewhich: {e}"))?;
    let mut text = String::new();
    for file in String::from_utf8_lossy(&out.stdout).lines() {
        let cnf = fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
        text += &format!("\n% {file}\n{cnf}");
    }
    Ok(text)
}

/// The folder that holds the package of `rel` (`tex/latex/pgf`, `fonts/opentype/public/lm`), or
/// `rel` itself for other files.
fn package_of(rel: &str) -> String {
    let depth = match rel.split('/').next() {
        Some("tex") => 3,
        Some("fonts") => 4,
        _ => return rel.to_owned(),
    };
    let parts: Vec<&str> = rel.split('/').collect();
    if parts.len() > depth { parts[..depth].join("/") } else { rel.to_owned() }
}

/// Every file under `dir`, with its path relative to `base` (symbolic links followed: Debian's
/// trees link into others).
fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    let Ok(entries) = fs::read_dir(dir) else { return Ok(()) };
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            walk(base, &path, out)?;
        } else if path.is_file()
            && let Ok(rel) = path.strip_prefix(base)
        {
            out.push((rel.to_string_lossy().replace('\\', "/"), path));
        }
    }
    Ok(())
}

/// Copies the folder `from` into `to`.
fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    let mut files = Vec::new();
    walk(from, from, &mut files)?;
    for (rel, source) in files {
        let target = to.join(rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(io(parent))?;
        }
        fs::copy(&source, &target).map_err(io(&source))?;
    }
    Ok(())
}

/// The `.fls` recording under `dir` (retex writes it with the job's auxiliary files).
fn find_fls(dir: &Path) -> Option<PathBuf> {
    let mut files = Vec::new();
    walk(dir, dir, &mut files).ok()?;
    files.into_iter().map(|(_, p)| p).find(|p| p.extension().is_some_and(|e| e == "fls"))
}

/// The files the `-recorder` file `fls` lists as read, outside the job's folder `dir`.
fn inputs(fls: &Path, dir: &Path) -> Result<Vec<PathBuf>> {
    let text = fs::read_to_string(fls).map_err(io(fls))?;
    Ok(text.lines().filter_map(|l| l.strip_prefix("INPUT ")).map(PathBuf::from).filter(|p| p.is_absolute() && !p.starts_with(dir)).collect())
}

/// The files the document `main` (in its folder) reads, typeset with `retex` in a copy of the
/// folder under `work`.
fn recorded(retex: &Path, main: &Path, work: &Path) -> Result<Vec<PathBuf>> {
    let folder = main.parent().ok_or("a document is in a folder")?;
    let name = main.file_name().ok_or("a document has a name")?;
    let copy = work.join(format!("doc-{:016x}", crate::fnv(main.to_string_lossy().as_bytes())));
    copy_dir(folder, &copy)?;
    let status = Command::new(retex)
        .args(["--offline", "-recorder", "-interaction=nonstopmode"])
        .arg(name)
        .current_dir(&copy)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("cannot run {}: {e}", retex.display()))?;
    if !status.success() {
        eprintln!("warning: {} has errors", main.display());
    }
    let fls = find_fls(&copy).ok_or_else(|| format!("{}: no -recorder file", main.display()))?;
    inputs(&fls, &copy)
}

/// The files the LaTeX format reads (the documents' recordings leave them out: the format is
/// made before their runs).
fn format_recorded(retex: &Path, work: &Path) -> Result<Vec<PathBuf>> {
    let dir = work.join("format");
    fs::create_dir_all(&dir).map_err(io(&dir))?;
    let status = Command::new(retex)
        .args(["--offline", "-ini", "-recorder", "-interaction=nonstopmode", "-jobname=lualatex", "lualatex.ini"])
        .current_dir(&dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("cannot run {}: {e}", retex.display()))?;
    if !status.success() {
        return Err("the LaTeX format has errors".into());
    }
    inputs(&dir.join("lualatex.fls"), &dir)
}

/// A document loading the package or class `name` alone.
fn test_document(name: &str, class: bool) -> String {
    if class {
        format!(
            "\\documentclass{{{name}}}\n\\begin{{document}}\n\\title{{Title}}\\author{{Author}}\n\\maketitle\n\\section{{Section}}\nText, $x^2$, \\textbf{{bold}}, \\emph{{emphasis}}.\n\\end{{document}}\n"
        )
    } else {
        format!(
            "\\documentclass{{article}}\n\\usepackage{{{name}}}\n\\begin{{document}}\nText, $x^2$, \\textbf{{bold}}, \\emph{{emphasis}}.\n\\end{{document}}\n"
        )
    }
}

/// The blank project TeXCraft starts with, written to `dir`: its main document.
fn blank(dir: &Path) -> Result<PathBuf> {
    let project = texcraft_project::Project::blank();
    project.save_dir(dir).map_err(|e| e.to_string())?;
    let main = project.main().ok_or("the blank project has a main document")?;
    Ok(dir.join(main.as_str()))
}

/// The main documents of the templates in `site` (`cargo xtask templates-site`'s folder).
fn templates(site: &Path) -> Result<Vec<PathBuf>> {
    let catalog = texcraft_templates::Catalog::builtin().map_err(|e| e.to_string())?;
    let mut docs = Vec::new();
    for t in catalog.templates() {
        let main = site.join(&t.id).join(&t.main);
        if !main.is_file() {
            return Err(format!("{}: no template there (run `cargo xtask templates-site` first)", main.display()));
        }
        docs.push(main);
    }
    Ok(docs)
}

/// The tree's files: their path in it and where they are; and the folders (or files) whose files
/// are fetched before a first build (`read` lines).
struct Tree {
    files: BTreeMap<String, PathBuf>,
    read: Vec<String>,
}

/// The tree that `spec` describes, `retex` typesetting its documents in `work`.
fn select(spec: &Path, retex: &Path, out: &Path, work: &Path) -> Result<Tree> {
    let trees = installation_trees()?;
    if trees.is_empty() {
        return Err("no TeX installation found (kpsewhich --expand-path='$TEXMF')".into());
    }
    let text = fs::read_to_string(spec).map_err(io(spec))?;
    let mut packages = BTreeSet::new();
    let mut read = Vec::new();
    let mut docs = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (kind, rest) = line.split_once(' ').unwrap_or((line, ""));
        match kind {
            "dir" => {
                packages.insert(rest.trim().to_owned());
            }
            "read" => {
                packages.insert(rest.trim().to_owned());
                read.push(rest.trim().to_owned());
            }
            "blank" => docs.push(blank(&work.join("blank"))?),
            "templates" => docs.extend(templates(&out.join("templates"))?),
            "package" | "class" => {
                for name in rest.split_whitespace() {
                    let dir = work.join(format!("{kind}-{name}"));
                    fs::create_dir_all(&dir).map_err(io(&dir))?;
                    // Not `{name}.tex`: packages load files of that name (chemfig).
                    let doc = dir.join("test.tex");
                    fs::write(&doc, test_document(name, kind == "class")).map_err(io(&doc))?;
                    docs.push(doc);
                }
            }
            _ => return Err(format!("{}: cannot read `{line}`", spec.display())),
        }
    }
    eprintln!("  {} documents", docs.len());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let runs = work.join("runs");
    let mut recordings: Vec<Result<Vec<PathBuf>>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    let mut found = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(doc) = docs.get(i) else { break found };
                        found.push(recorded(retex, doc, &runs));
                    }
                })
            })
            .collect();
        workers.into_iter().flat_map(|w| w.join().unwrap_or_default()).collect()
    });
    recordings.push(format_recorded(retex, work));
    let mut outside = BTreeSet::new();
    for files in recordings {
        let files = files.unwrap_or_else(|e| {
            eprintln!("warning: {e}");
            Vec::new()
        });
        for file in files {
            match trees.iter().find_map(|t| file.strip_prefix(t).ok()) {
                Some(rel) => {
                    packages.insert(package_of(&rel.to_string_lossy()));
                }
                // The format is the browser's own.
                None if file.extension().is_some_and(|e| e == "fmt") => {}
                None => {
                    outside.insert(file);
                }
            }
        }
    }
    for p in &outside {
        eprintln!("warning: read outside the TeX trees, left out: {}", p.display());
    }
    let mut files = BTreeMap::new();
    for package in &packages {
        for tree in &trees {
            let mut found = Vec::new();
            let path = tree.join(package);
            if path.is_file() {
                found.push((package.clone(), path));
            } else {
                walk(tree, &path, &mut found)?;
            }
            for (rel, source) in found {
                files.entry(rel).or_insert(source);
            }
        }
    }
    // Made here.
    files.remove("ls-R");
    files.remove("web2c/texmf.cnf");
    Ok(Tree { files, read })
}

/// The tree's files with their bytes, its `texmf.cnf` and `ls-R` among them.
fn contents(tree: &Tree) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut contents = BTreeMap::new();
    for (rel, source) in &tree.files {
        contents.insert(rel.clone(), fs::read(source).map_err(io(source))?);
    }
    contents.insert("web2c/texmf.cnf".to_owned(), format!("{CNF}{}", installation_cnf()?).into_bytes());
    let ls_r = ls_r(contents.keys().map(String::as_str).chain(["ls-R"]));
    contents.insert("ls-R".to_owned(), ls_r);
    Ok(contents)
}

/// Where each file of the tree is on the site: alone (`tree/texlive/texmf-dist/…`) or in a pack
/// (`tree/packs/N`), by its path relative to the tree.
type Index = BTreeMap<String, String>;

/// Writes the tree to `dir` (`dir/texlive/texmf-dist/…` for the files fetched alone,
/// `dir/packs/N` for the others) with `dir/index.txt`, a line per file for the browser's lazy
/// files: its size and its path there, then for a packed file its pack's number and its offset.
fn write(contents: &BTreeMap<String, Vec<u8>>, dir: &Path) -> Result<Index> {
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(io(dir))?;
    }
    let mut packs: BTreeMap<&str, Vec<(&str, &[u8])>> = BTreeMap::new();
    let mut index = String::new();
    let mut site = Index::new();
    let base = dir.join(ROOT.trim_start_matches('/'));
    let mut bytes_total = 0u64;
    for (rel, bytes) in contents {
        bytes_total += bytes.len() as u64;
        if bytes.len() as u64 <= PACKED {
            let parent = rel.rsplit_once('/').map_or("", |(d, _)| d);
            packs.entry(parent).or_default().push((rel, bytes));
            continue;
        }
        let to = base.join(rel);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(io(parent))?;
        }
        fs::write(&to, bytes).map_err(io(&to))?;
        index += &format!("{} {ROOT}/{rel}\n", bytes.len());
        site.insert(rel.clone(), format!("tree{ROOT}/{rel}"));
    }
    let packs_dir = dir.join("packs");
    fs::create_dir_all(&packs_dir).map_err(io(&packs_dir))?;
    for (n, members) in packs.values().enumerate() {
        let mut pack = Vec::new();
        for (rel, bytes) in members {
            index += &format!("{} {ROOT}/{rel} {n} {}\n", bytes.len(), pack.len());
            site.insert((*rel).to_owned(), format!("tree/packs/{n}"));
            pack.extend_from_slice(bytes);
        }
        let to = packs_dir.join(n.to_string());
        fs::write(&to, pack).map_err(io(&to))?;
    }
    let to = dir.join("index.txt");
    fs::write(&to, index).map_err(io(&to))?;
    eprintln!("{}: {} files in {} packs and alone, {:.1} MB", dir.display(), contents.len(), packs.len(), bytes_total as f64 / 1e6);
    Ok(site)
}

/// The `ls-R` database of `files` (paths relative to the tree's root).
fn ls_r<'a>(files: impl Iterator<Item = &'a str>) -> Vec<u8> {
    let mut dirs: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    dirs.entry(String::new()).or_default();
    for rel in files {
        let mut parent = String::new();
        for part in rel.split('/') {
            dirs.entry(parent.clone()).or_default().insert(part.to_owned());
            parent = if parent.is_empty() { part.to_owned() } else { format!("{parent}/{part}") };
        }
    }
    let mut out = String::from("% ls-R -- filename database for kpathsea; do not change this line.\n");
    for (dir, entries) in &dirs {
        out += &format!("./{dir}:\n");
        for e in entries {
            out += e;
            out.push('\n');
        }
        out.push('\n');
    }
    out.into_bytes()
}

/// Whether this process may make `/texlive` and `/cache` itself (root, as in CI's container).
fn is_root() -> bool {
    Command::new("id").arg("-u").output().is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
}

/// Writes `contents` under `root` (`root/texlive/texmf-dist/…`) with every time 0, as the
/// browser's files have.
fn stage(contents: &BTreeMap<String, Vec<u8>>, root: &Path) -> Result<()> {
    let base = root.join(ROOT.trim_start_matches('/'));
    for (rel, bytes) in contents {
        let to = base.join(rel);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(io(parent))?;
        }
        fs::write(&to, bytes).map_err(io(&to))?;
        fs::File::options().write(true).open(&to).and_then(|f| f.set_modified(std::time::UNIX_EPOCH)).map_err(io(&to))?;
    }
    Ok(())
}

/// Makes the seed: retex's indexes of the tree, made by the native retex building the blank
/// project with the tree where the browser has it, and the files that build reads.
fn seed(contents: &BTreeMap<String, Vec<u8>>, index: &Index, read: &[String], retex: &Path, out: &Path, work: &Path) -> Result<()> {
    let root = is_root();
    // The system's root as root (CI); a folder bubblewrap mounts as `/texlive` and `/cache`.
    let stage_root = if root { PathBuf::from("/") } else { work.join("stage") };
    let cache = stage_root.join("cache");
    let job = work.join("seed-job");
    for dir in [stage_root.join("texlive"), cache.clone()] {
        if root && dir.exists() {
            fs::remove_dir_all(&dir).map_err(io(&dir))?;
        }
    }
    fs::create_dir_all(&cache).map_err(io(&cache))?;
    stage(contents, &stage_root)?;
    // Runs retex in `dir` with the tree and the cache where the browser has them.
    let in_tree = |dir: &Path, args: &[&str]| -> Result<bool> {
        let mut cmd = if root {
            let mut c = Command::new(retex);
            c.current_dir(dir).env_clear().envs(ENV);
            c
        } else {
            let mut c = Command::new("bwrap");
            c.args(["--ro-bind", "/usr", "/usr", "--symlink", "usr/lib", "/lib", "--symlink", "usr/lib64", "/lib64", "--symlink", "usr/bin", "/bin"]);
            c.args(["--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp", "--dir", "/home"]);
            c.arg("--bind").arg(stage_root.join("texlive")).arg("/texlive");
            c.arg("--bind").arg(&cache).arg("/cache");
            c.arg("--bind").arg(dir).arg("/job");
            c.arg("--ro-bind").arg(retex).arg("/retex");
            // The sandbox's environment is only the browser's: `--clearenv`, then each variable.
            c.args(["--chdir", "/job", "--clearenv"]);
            for (k, v) in ENV {
                c.args(["--setenv", k, v]);
            }
            c.arg("/retex");
            c
        };
        let status = cmd
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| format!("cannot run retex for the seed ({}): {e}", if root { "as root" } else { "bubblewrap" }))?;
        Ok(status.success())
    };
    // The format, as a first build makes it: what it reads goes in the read list too.
    let format = work.join("seed-format");
    fs::create_dir_all(&format).map_err(io(&format))?;
    if !in_tree(&format, &["--offline", "-ini", "-recorder", "-interaction=nonstopmode", "-jobname=lualatex", "lualatex.ini"])? {
        return Err("the LaTeX format does not build over the tree (the seed's run)".into());
    }
    let main = blank(&job)?;
    let name = main.file_name().ok_or("a main document has a name")?.to_string_lossy().into_owned();
    if !in_tree(&job, &["--offline", "-recorder", "-interaction=nonstopmode", &name])? {
        return Err("the blank project does not build over the tree (the seed's run)".into());
    }
    // The indexes.
    let seed = out.join("seed");
    if seed.exists() {
        fs::remove_dir_all(&seed).map_err(io(&seed))?;
    }
    let mut list = String::new();
    let mut made = Vec::new();
    walk(&cache, &cache.join("kpse"), &mut made)?;
    for (rel, source) in made {
        let to = seed.join("cache").join(&rel);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(io(parent))?;
        }
        fs::copy(&source, &to).map_err(io(&source))?;
        list += &format!("{} /cache/{rel}\n", fs::metadata(&to).map_err(io(&to))?.len());
    }
    if list.is_empty() {
        return Err("the seed's run made no index in /cache/kpse".into());
    }
    let to = seed.join("index.txt");
    fs::write(&to, list).map_err(io(&to))?;
    // What the format and the blank project read: their site's files, fetched before a first
    // build.
    let mut text = String::new();
    for fls in [format.join("lualatex.fls"), find_fls(&job).ok_or("the seed's run left no -recorder file")?] {
        text += &fs::read_to_string(&fls).map_err(io(&fls))?;
    }
    let prefix = format!("{ROOT}/");
    let mut reads: BTreeSet<&String> = text
        .lines()
        .filter_map(|l| l.strip_prefix("INPUT "))
        .filter_map(|p| p.strip_prefix(prefix.as_str()))
        .filter_map(|rel| index.get(rel))
        .collect();
    // And what Lua reads itself, which the recordings miss.
    reads.extend(index.iter().filter(|(rel, _)| read.iter().any(|r| *rel == r || rel.starts_with(&format!("{r}/")))).map(|(_, site)| site));
    let reads_dir = seed.join("reads");
    fs::create_dir_all(&reads_dir).map_err(io(&reads_dir))?;
    let to = reads_dir.join("blank.txt");
    fs::write(&to, reads.into_iter().map(|p| format!("{p}\n")).collect::<String>()).map_err(io(&to))?;
    if root {
        for dir in [stage_root.join("texlive"), cache] {
            let _ = fs::remove_dir_all(&dir);
        }
    }
    eprintln!("{}: the seed", seed.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_belongs_to_its_package_folder() {
        assert_eq!(package_of("tex/latex/pgf/frontendlayer/tikz.sty"), "tex/latex/pgf");
        assert_eq!(package_of("fonts/opentype/public/lm/lmroman10-regular.otf"), "fonts/opentype/public/lm");
        assert_eq!(package_of("web2c/texmf.cnf"), "web2c/texmf.cnf");
    }

    #[test]
    fn ls_r_lists_every_folder() {
        let text = String::from_utf8(ls_r(["tex/latex/base/article.cls", "ls-R"].into_iter())).unwrap_or_default();
        assert!(text.contains("./:\nls-R\ntex\n\n"));
        assert!(text.contains("./tex/latex:\nbase\n\n./tex/latex/base:\narticle.cls\n\n"));
    }
}

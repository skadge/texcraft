//! Downloading a TeX Live template, with TeX Live's checks.
//!
//! 0. The TeX Live year: the installed TeX Live's (as retex finds it), so a template matches the
//!    classes it will be built with; retex's own year without an installed TeX Live. Older years
//!    come from TeX Live's historic archive.
//! 1. TeX Live's package database (`tlpkg/texlive.tlpdb.xz`) of that year, kept for a week. Its
//!    SHA-512 (`texlive.tlpdb.sha512`) must carry a valid signature (`.sha512.asc`) by TeX Live's
//!    keys, and the decompressed database must have that SHA-512.
//! 2. The package's documentation archive (`archive/<package>.doc.tar.xz`), whose size and
//!    SHA-512 must be the database's (`doccontainersize`, `doccontainerchecksum`).
//! 3. Only the template's files are taken from the archive, each at most `MAX_FILE_BYTES`.
//!
//! Through CTAN's redirector each file may come from another mirror, and a mirror being updated
//! may serve a checksum and a database that do not match yet: each step is tried a few times.

use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use retex_texlive::openpgp;
use retex_texlive::repo::{self, Repository, sha512_hex};

use crate::{Error, MAX_FILE_BYTES, Result, Source, Store, Template};

const RETRIES: usize = 3;
/// How long a kept package database is used before fetching a new one.
const DATABASE_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

/// The TeX Live year templates are taken from: the installed one's, or retex's own.
pub(crate) fn texlive_year() -> u16 {
    let mut options = retex_kpath::Options::lualatex();
    options.cache_dir = Some(std::env::temp_dir().join("texcraft-kpse"));
    retex_kpath::Kpathsea::new(options).ok().and_then(|mut k| retex_texlive::year::installed(&mut k)).map_or(retex_texlive::DEFAULT_YEAR, |i| i.year)
}

/// The release year a package database describes (`depend release/2025` of `00texlive.config`).
fn release(database: &str) -> Option<u16> {
    let at = database.find("depend release/")? + "depend release/".len();
    database.get(at..at + 4)?.parse().ok()
}

fn get(repo: &Repository, rel: &str) -> Result<Vec<u8>> {
    repo.get(rel).map_err(|e| Error::Download(format!("{}: {e}", repo.url(rel))))
}

/// The verified package database of `year` and the repository it came from, from the store's copy
/// when it is recent.
fn database(store: &Store, year: u16) -> Result<(String, Repository)> {
    let repos = Repository::for_year(year, retex_texlive::DEFAULT_YEAR, None);
    let path = store.dir().join(format!("texlive-{year}.tlpdb"));
    let origin = store.dir().join(format!("texlive-{year}.repository"));
    let fresh =
        std::fs::metadata(&path).and_then(|m| m.modified()).is_ok_and(|t| SystemTime::now().duration_since(t).unwrap_or(DATABASE_AGE) < DATABASE_AGE);
    if fresh && let (Ok(text), Ok(base)) = (std::fs::read_to_string(&path), std::fs::read_to_string(&origin)) {
        return Ok((text, Repository::new(base.trim())));
    }
    let keys = openpgp::texlive_keys();
    let mut last = None;
    for repo in &repos {
        match database_from(repo, &keys, year) {
            Ok(text) => {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(e) = std::fs::write(&path, &text).and_then(|()| std::fs::write(&origin, repo.base())) {
                    log::warn!("keeping the package database: {e}");
                }
                return Ok((text, Repository::new(repo.base())));
            }
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| Error::Download("no repository answered".to_owned())))
}

fn database_from(repo: &Repository, keys: &[openpgp::PublicKey], year: u16) -> Result<String> {
    let mut last = None;
    for _ in 0..RETRIES {
        let sum = get(repo, "tlpkg/texlive.tlpdb.sha512")?;
        let asc = get(repo, "tlpkg/texlive.tlpdb.sha512.asc")?;
        if let Err(e) = openpgp::verify(&sum, &asc, keys) {
            last = Some(Error::Integrity(format!("the package database's signature: {e}")));
            continue;
        }
        let want = String::from_utf8_lossy(&sum).split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
        let xz = get(repo, "tlpkg/texlive.tlpdb.xz")?;
        let text = repo::unxz(&xz).map_err(|e| Error::Integrity(format!("the package database: {e}")))?;
        if sha512_hex(&text) != want {
            last = Some(Error::Integrity("the package database is not the signed one".to_owned()));
            continue;
        }
        let text = String::from_utf8_lossy(&text).into_owned();
        if release(&text) != Some(year) {
            return Err(Error::Download(format!("{} is not TeX Live {year}", repo.base())));
        }
        return Ok(text);
    }
    Err(last.unwrap_or_else(|| Error::Download("no mirror answered".to_owned())))
}

/// `package`'s documentation archive size and SHA-512, from the database.
fn doc_container(database: &str, package: &str) -> Result<(u64, String)> {
    let head = format!("name {package}\n");
    let stanza = database
        .split("\n\n")
        .find(|s| s.trim_start_matches('\n').starts_with(&head))
        .ok_or_else(|| Error::Download(format!("TeX Live has no package {package}")))?;
    let field = |name: &str| stanza.lines().find_map(|l| l.strip_prefix(name).map(str::trim).map(str::to_owned));
    let size = field("doccontainersize ").and_then(|s| s.parse().ok()).ok_or_else(|| Error::Download(format!("{package} has no documentation")))?;
    let checksum = field("doccontainerchecksum ").ok_or_else(|| Error::Download(format!("{package} has no documentation checksum")))?;
    Ok((size, checksum.to_ascii_lowercase()))
}

pub(crate) fn fetch(store: &Store, template: &Template) -> Result<PathBuf> {
    let Source::TexLive(package) = &template.source;
    let (database, repo) = database(store, texlive_year())?;
    let (size, checksum) = doc_container(&database, package)?;
    let rel = format!("archive/{package}.doc.tar.xz");
    let mut archive = None;
    for _ in 0..RETRIES {
        let data = get(&repo, &rel)?;
        if data.len() as u64 == size && sha512_hex(&data) == checksum {
            archive = Some(data);
            break;
        }
    }
    let data = archive.ok_or_else(|| Error::Integrity(format!("{}: not the archive the package database describes", repo.url(&rel))))?;
    let tar = repo::unxz(&data).map_err(|e| Error::Integrity(format!("{rel}: {e}")))?;
    let wanted = template.project_paths();
    let mut found: Vec<(String, Vec<u8>)> = Vec::new();
    let mut entries = tar::Archive::new(&tar[..]);
    let bad = |e: std::io::Error| Error::Integrity(format!("{rel}: {e}"));
    for entry in entries.entries().map_err(bad)? {
        let mut entry = entry.map_err(bad)?;
        let path = entry.path().map_err(bad)?.to_string_lossy().replace('\\', "/");
        // RELOC packages store `doc/latex/<package>/…`, others `texmf-dist/doc/latex/<package>/…`.
        let inside = path.split_once(&format!("doc/latex/{package}/")).map(|(_, rest)| rest.to_owned());
        let Some(inside) = inside else { continue };
        let Some((_, project)) = wanted.iter().find(|(source, _)| template.names_of(source).any(|name| name == inside)) else { continue };
        if entry.size() > MAX_FILE_BYTES {
            return Err(Error::Integrity(format!("{inside} is larger than a template file may be")));
        }
        let mut bytes = Vec::new();
        entry.by_ref().take(MAX_FILE_BYTES).read_to_end(&mut bytes).map_err(bad)?;
        found.push((project.clone(), bytes));
    }
    if let Some((missing, _)) = wanted.iter().find(|(_, project)| !found.iter().any(|(p, _)| p == project)) {
        return Err(Error::Download(format!("{package}'s documentation has no {missing}")));
    }
    store.keep(template, &found)
}

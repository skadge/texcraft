//! TeXCraft's own templates, embedded at build time from `templates/builtin/<id>/` (CC0-1.0):
//! offered offline and in the browser with no download.

/// Every file of every built-in template: the template's id, the file's path in the project, its
/// bytes.
static FILES: &[(&str, &str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/builtin.rs"));

/// The files of built-in template `id` (path in the project, bytes), or none for another id.
pub fn files(id: &str) -> Vec<(String, Vec<u8>)> {
    FILES.iter().filter(|(t, _, _)| *t == id).map(|(_, path, bytes)| ((*path).to_owned(), bytes.to_vec())).collect()
}

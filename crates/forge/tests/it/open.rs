//! Opening a project: its tree page after page, its files' blobs, what is left on the branch.

use texcraft_forge::{MAX_FILE, Request, Sending, Transport, open};

use crate::{block_on, client, client_with, paper};

#[test]
fn every_file_of_the_branch_is_loaded_with_its_blob_id() {
    let fake = paper();
    let loaded = block_on(open(&client(&fake).unwrap(), None)).unwrap();
    let files: Vec<&str> = loaded.files.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(files, ["chapters/intro.tex", "fig/plot.png", "main.tex"]);
    assert_eq!(loaded.link.branch, "main");
    assert_eq!(Some(loaded.link.base.clone()), fake.head("main"));
    for (path, bytes) in &loaded.files {
        assert_eq!(fake.files("main").get(path), Some(bytes), "{path} came back changed");
        assert_eq!(loaded.link.files.get(path), Some(&texcraft_history::store::blob_id(bytes).unwrap()));
    }
}

#[test]
fn a_tree_of_many_pages_is_read_to_its_end_either_way_gitlab_pages_it() {
    for link_header in [true, false] {
        let fake = paper();
        let more: Vec<(String, Vec<u8>)> = (0..7).map(|i| (format!("sections/s{i}.tex"), format!("Section {i}\n").into_bytes())).collect();
        let changes: Vec<(&str, Option<&[u8]>)> = more.iter().map(|(p, b)| (p.as_str(), Some(b.as_slice()))).collect();
        fake.push("main", &changes);
        fake.set_per_page(3);
        fake.set_link_header(link_header);
        let loaded = block_on(open(&client(&fake).unwrap(), None)).unwrap();
        assert_eq!(loaded.files.len(), fake.files("main").len(), "link header: {link_header}");
        let pages = fake.requests().iter().filter(|r| r.contains("/repository/tree")).count();
        assert!(pages > 1, "the tree was read in one page");
    }
}

#[test]
fn hidden_files_links_and_submodules_stay_on_the_branch() {
    let fake = paper();
    fake.push("main", &[(".gitlab-ci.yml", Some(b"build:\n".as_slice())), (".gitignore", Some(b"*.aux\n".as_slice()))]);
    fake.push_entry("main", "latest.pdf", "120000");
    fake.push_entry("main", "vendor/styles", "160000");
    let loaded = block_on(open(&client(&fake).unwrap(), None)).unwrap();
    let paths: Vec<&str> = loaded.files.iter().map(|(p, _)| p.as_str()).collect();
    for left in [".gitlab-ci.yml", ".gitignore", "latest.pdf", "vendor/styles"] {
        assert!(!paths.contains(&left), "{left} was loaded");
        assert!(!loaded.link.files.contains_key(left), "{left} is tracked");
    }
    assert!(loaded.link.skipped.iter().any(|s| s.path == "latest.pdf"), "the link is not said to be left out");
}

#[test]
fn a_file_over_the_limit_is_skipped_and_said() {
    let fake = paper();
    let big = vec![b'x'; MAX_FILE as usize + 1];
    fake.push("main", &[("data/big.csv", Some(big.as_slice()))]);
    let loaded = block_on(open(&client(&fake).unwrap(), None)).unwrap();
    assert!(!loaded.files.iter().any(|(p, _)| p == "data/big.csv"));
    assert!(!loaded.link.files.contains_key("data/big.csv"), "a skipped file would count as deleted");
    assert!(loaded.link.skipped.iter().any(|s| s.path == "data/big.csv"));
    assert!(loaded.files.iter().any(|(p, _)| p == "main.tex"), "the rest of the project was not loaded");
}

#[test]
fn another_branch_can_be_opened() {
    let fake = paper();
    fake.push("draft", &[("main.tex", Some(b"draft\n".as_slice()))]);
    let loaded = block_on(open(&client(&fake).unwrap(), Some("draft"))).unwrap();
    assert_eq!(loaded.link.branch, "draft");
    assert_eq!(loaded.files, vec![("main.tex".to_owned(), b"draft\n".to_vec())]);
}

/// The fake, with every file's contents changed on the way.
struct Corrupting(texcraft_forge::fake::FakeGitLab);

impl Transport for Corrupting {
    fn send(&self, request: Request) -> Sending<'_> {
        let raw = request.url.ends_with("/raw");
        Box::pin(async move {
            let mut response = self.0.send(request).await?;
            if raw {
                response.body.push(b'!');
            }
            Ok(response)
        })
    }
}

#[test]
fn contents_that_do_not_match_their_blob_id_are_refused() {
    let fake = paper();
    let result = block_on(open(&client_with(Box::new(Corrupting(fake)), crate::TOKEN).unwrap(), None));
    assert!(matches!(result, Err(texcraft_forge::Error::Invalid(_))), "{result:?}");
}

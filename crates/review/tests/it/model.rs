use texcraft_review::{Author, Error, Filter, Id, Message, State, format};

use super::{ada, at, one_thread};

#[test]
fn a_thread_survives_writing_and_reading_its_file() {
    let text = "Le chat 😀 est là.\nUne autre ligne.";
    let (mut r, id) = one_thread(text, 8, 13).unwrap();
    let bob = Author::new("Bob", "");
    r.reply(id, Message::new(Id::new(2_000, 1), bob.clone(), at(60), "Yes, “quite” right.\nSee §2.")).unwrap();
    r.edit(id, Id::new(1_000, 43), &ada(), "Is this really right?", at(90)).unwrap();
    r.resolve(id, &bob, at(120)).unwrap();
    let thread = r.get(id).unwrap();
    let file = format::write(thread).unwrap();
    assert!(file.contains("\"version\": 1"), "{file}");
    assert_eq!(&format::read(&file).unwrap(), thread);
    assert_eq!(format::file_name(id), format!("{id}.json"));
}

#[test]
fn a_file_of_a_newer_version_is_refused_with_a_message() {
    let (r, id) = one_thread("some text", 0, 4).unwrap();
    let file = format::write(r.get(id).unwrap()).unwrap().replace("\"version\": 1", "\"version\": 99");
    let err = format::read(&file).unwrap_err();
    assert_eq!(err, Error::Newer(99));
    assert!(err.to_string().contains("newer"), "{err}");
    assert!(format::read("{\"kind\": \"thread\"}").is_err(), "a file without a version was read");
    assert!(format::read("not json").is_err());
}

#[test]
fn ids_sort_by_time_and_read_back() {
    let (a, b) = (Id::new(1, u128::MAX), Id::new(2, 0));
    assert!(a < b);
    assert_eq!(Id::parse(&a.to_string()), Some(a));
    assert_eq!(Id::parse("not an id"), None);
}

#[test]
fn only_its_author_changes_or_deletes_a_message() {
    let (mut r, id) = one_thread("some text", 0, 4).unwrap();
    let first = Id::new(1_000, 43);
    let impostor = Author::new("Ada", "another key");
    assert_eq!(r.edit(id, first, &impostor, "changed", at(1)), Err(Error::NotAuthor), "the same name is not the same person");
    assert_eq!(r.remove(id, &impostor).map(|_| ()), Err(Error::NotAuthor));
    r.delete_message(id, first, &ada()).unwrap();
    let m = &r.get(id).unwrap().messages[0];
    assert!(m.deleted && m.text.is_empty());
    assert!(r.remove(id, &ada()).is_ok());
    assert!(r.get(id).is_none());
}

#[test]
fn the_filter_keeps_threads_by_file_state_and_author() {
    let (mut r, id) = one_thread("some text", 0, 4).unwrap();
    let bob = Author::new("Bob", "b0b");
    let other = Id::new(5_000, 9);
    r.comment(other, texcraft_review::Anchor::new("intro.tex", "x", 0, 1), Message::new(Id::new(5_000, 10), bob.clone(), at(5), "Hm.")).unwrap();
    r.resolve(other, &bob, at(6)).unwrap();
    let count = |f: Filter| r.list(&f).count();
    assert_eq!(count(Filter::default()), 2);
    assert_eq!(count(Filter { open: Some(true), ..Filter::default() }), 1);
    assert_eq!(count(Filter { open: Some(false), ..Filter::default() }), 1);
    assert_eq!(count(Filter { path: Some("intro.tex".into()), ..Filter::default() }), 1);
    assert_eq!(count(Filter { author: Some("Ada".into()), ..Filter::default() }), 1);
    r.reopen(other).unwrap();
    assert_eq!(r.get(other).unwrap().state, State::Open);
    assert!(r.reply(id, Message::new(Id::new(6_000, 1), bob, at(7), "  ")).is_err(), "an empty reply was taken");
}

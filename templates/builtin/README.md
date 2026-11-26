# TeXCraft's own templates

Written for TeXCraft (2026-11-25), not taken from any other template, and dedicated to the public
domain under [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/): use them as you like.
Each folder is one template, listed in `../catalog.toml` with `source = { builtin = true }`, and
compiled into the app (`crates/templates/build.rs`), so it needs no download. Each builds with
retex; a template using a class or package must keep to those of the browser build's TeX tree
(`apps/texcraft-web/tree.txt`).

`example/` is not a template but the example project the start page offers (Try an Example; its
catalogue category is `example`, which the gallery leaves out). Written for TeXCraft (2026-11-26)
under the same CC0-1.0 dedication: the text, the table and the TikZ figure are ours. Its one
citation is a real book, Britney C. Gallivan's *How to Fold Paper in Half Twelve Times* (2002),
whose loss formula it quotes as a fact with the reference in `references.bib`; nothing of the book
is copied. `crates/compile/tests/it/example.rs` builds it with retex and checks it is clean.

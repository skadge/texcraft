# Feature names

User-visible names are checked under standards/naming.md before they ship. Other editors' product and feature names are not used as ours.

| Our name | What it is | Status |
|---|---|---|
| TeXCraft | The app | **Unchecked.** "TeX" is a trademark of the American Mathematical Society; the rule 4 check must run before a release (docs/ROADMAP.md, open decisions). |
| Compile, Compile Automatically | Building the PDF | Plain descriptive words |
| Build Log | The list of messages of a build | Plain descriptive words |
| Outline | The section list | Plain descriptive words |
| Source, Formatted (planned) | The two ways the editor shows a file: as source, or with sections, emphasis, lists, formulas, figures and tables shown formatted | Plain descriptive words; other editors' names for this mode are not used |
| User Guide | The help pages (Help › User Guide, `docs/guide/`) | Plain descriptive words |
| DOCX Document (.docx), PPTX Presentation (.pptx) | Export formats in the PDF pane's Export menu | File format names (standards/naming.md rule 3 and its "name the file it writes" row); the word-processor and slide products those files open in are not named (UX review, 2026-11-26) |
| Import a Project, Import… | The dialog that brings a project in from elsewhere (File › Import a Project…, the start page) | Plain descriptive words |
| GitLab, GitHub, Overleaf (on Import a Project's cards) | The services a project is imported from | The services' names, as the services and protocols we connect to once the integration works (standards/naming.md rule 3). Our own labels beside them: Git Address, GitLab Server, Folder, Zip File, Remember on this computer |

## Services' logos

The Import a Project dialog's GitLab, GitHub and Overleaf cards show those services' own marks (`assets/icons/brand-*.svg`, from the Simple Icons project's release 16.37.0, CC0-1.0, drawn in one colour, the text's). This is the operator's decision of 2026-11-26 (docs/design/import-dialog.md, decision 8), taken as fair use to identify the service a card connects to, and to be removed if it ever becomes a problem. It departs from standards/naming.md rule 2, which says we SHOULD NOT use other companies' logos or icons: the reason is that a card naming the service it connects to is found faster by its mark, which users know, than by its name alone. Each mark is its owner's trademark (ATTRIBUTION.md), shown only to name that service, never for a feature of ours; our own features keep our own words and icons.

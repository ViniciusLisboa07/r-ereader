use std::path::PathBuf;

use r_ereader_library::testing::{write_epub, write_pdf};
use r_ereader_library::{
    BookQuery, Filter, Format, ImportOutcome, Library, MetadataEdit, Progress, SortOrder,
};
use tempfile::TempDir;

struct Fixture {
    _dir: TempDir,
    sources: PathBuf,
    library: Library,
}

fn fixture() -> Fixture {
    let dir = TempDir::new().unwrap();
    let sources = dir.path().join("origem");
    std::fs::create_dir_all(&sources).unwrap();
    let library = Library::open(dir.path().join("biblioteca")).unwrap();
    Fixture {
        _dir: dir,
        sources,
        library,
    }
}

fn all(library: &Library) -> Vec<String> {
    library
        .books(&BookQuery::default())
        .unwrap()
        .into_iter()
        .map(|b| b.title)
        .collect()
}

#[test]
fn imports_epub_into_managed_folder() {
    let mut f = fixture();
    let source = write_epub(
        &f.sources,
        "x.epub",
        "Absalão, Absalão!",
        "William Faulkner",
        &["Romance"],
    );

    let id = match f.library.import(&source).unwrap() {
        ImportOutcome::Added(id) => id,
        other => panic!("esperava Added, veio {other:?}"),
    };
    let book = f.library.book(id).unwrap();

    assert_eq!(book.summary.title, "Absalão, Absalão!");
    assert_eq!(book.summary.authors, ["William Faulkner"]);
    assert_eq!(book.summary.formats, [Format::Epub]);
    let series = book.summary.series.as_ref().unwrap();
    assert_eq!(
        (series.name.as_str(), series.index),
        ("Romances do Sul", Some(2.0))
    );
    assert_eq!(book.description.as_deref(), Some("Uma história antiga."));
    assert_eq!(book.isbn.as_deref(), Some("9788535932584"));
    assert_eq!(book.tags, ["Romance"]);

    let expected_folder = f
        .library
        .root()
        .join(format!("William Faulkner/Absalão, Absalão! ({id})"));
    assert_eq!(book.folder, expected_folder);
    assert!(book.files[0].path.exists());
    assert!(
        book.files[0]
            .path
            .ends_with("Absalão, Absalão! - William Faulkner.epub")
    );
    assert!(book.cover.as_ref().unwrap().exists());
    assert!(book.summary.thumbnail.as_ref().unwrap().exists());
    // O original continua onde estava.
    assert!(source.exists());
}

#[test]
fn detects_duplicates_by_content() {
    let mut f = fixture();
    let source = write_epub(&f.sources, "a.epub", "Livro", "Autor", &[]);
    let copy = f.sources.join("copia.epub");
    std::fs::copy(&source, &copy).unwrap();

    let first = f.library.import(&source).unwrap();
    assert_eq!(
        f.library.import(&copy).unwrap(),
        ImportOutcome::Duplicate(first.id())
    );
    assert_eq!(f.library.book_count().unwrap(), 1);
}

#[test]
fn imports_pdf_metadata() {
    let mut f = fixture();
    let id = f
        .library
        .import(&write_pdf(&f.sources, "manual.pdf"))
        .unwrap()
        .id();
    let book = f.library.book(id).unwrap();

    assert_eq!(book.summary.title, "Manual de Liturgia");
    assert_eq!(book.summary.authors, ["Fulano de Tál", "Beltrano"]);
    assert_eq!(book.tags, ["liturgia", "oração"]);
    assert_eq!(book.summary.formats, [Format::Pdf]);
    assert_eq!(book.cover, None);
    assert!(f.library.file_path(id, Format::Pdf).unwrap().unwrap().exists());
}

#[test]
fn finds_books_by_file_content() {
    let mut f = fixture();
    let source = write_epub(&f.sources, "a.epub", "Livro", "Autor", &[]);
    let other = write_epub(&f.sources, "b.epub", "Outro", "Autor", &[]);
    let id = f.library.import(&source).unwrap().id();
    assert_eq!(f.library.find_by_content(&source).unwrap(), Some(id));
    assert_eq!(f.library.find_by_content(&other).unwrap(), None);
}

#[test]
fn rejects_unknown_formats() {
    let mut f = fixture();
    let path = f.sources.join("notas.txt");
    std::fs::write(&path, "oi").unwrap();
    assert!(f.library.import(&path).is_err());
    assert_eq!(f.library.book_count().unwrap(), 0);
}

#[test]
fn searches_ignoring_accents_and_prefixes() {
    let mut f = fixture();
    f.library
        .import(&write_epub(
            &f.sources,
            "1.epub",
            "Absalão, Absalão!",
            "William Faulkner",
            &[],
        ))
        .unwrap();
    f.library
        .import(&write_epub(
            &f.sources,
            "2.epub",
            "Até que tenhamos rostos",
            "C. S. Lewis",
            &["Mito"],
        ))
        .unwrap();

    let search = |text: &str| -> Vec<String> {
        f.library
            .books(&BookQuery {
                text: text.into(),
                ..Default::default()
            })
            .unwrap()
            .into_iter()
            .map(|b| b.title)
            .collect()
    };
    assert_eq!(search("absalao"), ["Absalão, Absalão!"]);
    assert_eq!(search("faulk"), ["Absalão, Absalão!"]);
    assert_eq!(search("lewis mito"), ["Até que tenhamos rostos"]);
    assert_eq!(search("historia").len(), 2);
    assert!(search("inexistente").is_empty());
}

#[test]
fn filters_and_sorts() {
    let mut f = fixture();
    let a = f
        .library
        .import(&write_epub(
            &f.sources,
            "1.epub",
            "O Zebra",
            "Ana Souza",
            &["Animais"],
        ))
        .unwrap()
        .id();
    let b = f
        .library
        .import(&write_epub(&f.sources, "2.epub", "Ética", "Bruno Lima", &[]))
        .unwrap()
        .id();
    f.library.import(&write_pdf(&f.sources, "3.pdf")).unwrap();

    let by_title = f
        .library
        .books(&BookQuery {
            sort: SortOrder::Title,
            ..Default::default()
        })
        .unwrap();
    let titles: Vec<_> = by_title.iter().map(|b| b.title.as_str()).collect();
    assert_eq!(titles, ["Ética", "Manual de Liturgia", "O Zebra"]);

    let authors = f.library.authors().unwrap();
    let ana = authors.iter().find(|a| a.name == "Ana Souza").unwrap();
    let only_ana = f
        .library
        .books(&BookQuery {
            filter: Filter::Author(ana.id),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(only_ana.iter().map(|b| b.id).collect::<Vec<_>>(), [a]);

    let pdfs = f
        .library
        .books(&BookQuery {
            filter: Filter::Format(Format::Pdf),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(pdfs.len(), 1);
    assert_eq!(
        f.library.formats().unwrap(),
        [(Format::Epub, 2), (Format::Pdf, 1)]
    );

    f.library
        .save_progress(
            b,
            Progress {
                chapter: 3,
                fraction: 0.4,
            },
        )
        .unwrap();
    let reading = f
        .library
        .books(&BookQuery {
            filter: Filter::Reading,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(reading.iter().map(|b| b.id).collect::<Vec<_>>(), [b]);
    assert_eq!(reading[0].progress, Some(0.4));
    assert_eq!(
        f.library.progress(b).unwrap(),
        Some(Progress {
            chapter: 3,
            fraction: 0.4
        })
    );
    assert_eq!(f.library.reading_count().unwrap(), 1);
}

#[test]
fn editing_metadata_moves_folder_and_renames_files() {
    let mut f = fixture();
    let id = f
        .library
        .import(&write_epub(
            &f.sources,
            "1.epub",
            "Rascunho",
            "Fulano",
            &["velha"],
        ))
        .unwrap()
        .id();
    let old_folder = f.library.book(id).unwrap().folder;

    f.library
        .update_metadata(
            id,
            &MetadataEdit {
                title: "Título Final".into(),
                authors: vec!["Ciclano".into(), "Fulano".into()],
                series: Some("Nova Série".into()),
                series_index: Some(1.5),
                tags: vec!["nova".into()],
                description: Some("  ".into()),
                ..Default::default()
            },
        )
        .unwrap();

    let book = f.library.book(id).unwrap();
    assert_eq!(book.summary.title, "Título Final");
    assert_eq!(book.summary.authors, ["Ciclano", "Fulano"]);
    assert_eq!(book.summary.series.as_ref().unwrap().index, Some(1.5));
    assert_eq!(book.tags, ["nova"]);
    assert_eq!(book.description, None);
    assert!(book.folder.ends_with(format!("Ciclano/Título Final ({id})")));
    assert!(book.files[0].path.ends_with("Título Final - Ciclano.epub"));
    assert!(book.files[0].path.exists());
    assert!(book.cover.unwrap().exists());
    assert!(!old_folder.exists());
    // A pasta do autor antigo ficou vazia e foi removida.
    assert!(!old_folder.parent().unwrap().exists());
    // Tag e série antigas sem livros somem do navegador.
    assert!(f.library.tags().unwrap().iter().all(|t| t.name != "velha"));
    assert!(
        f.library
            .series()
            .unwrap()
            .iter()
            .all(|s| s.name != "Romances do Sul")
    );
    // A busca enxerga os dados novos.
    let found = f
        .library
        .books(&BookQuery {
            text: "ciclano".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(found.len(), 1);
}

#[test]
fn rejects_empty_title() {
    let mut f = fixture();
    let id = f
        .library
        .import(&write_epub(&f.sources, "1.epub", "Livro", "Autor", &[]))
        .unwrap()
        .id();
    assert!(f.library.update_metadata(id, &MetadataEdit::default()).is_err());
    assert_eq!(f.library.book(id).unwrap().summary.title, "Livro");
}

#[test]
fn manages_collections() {
    let mut f = fixture();
    let id = f
        .library
        .import(&write_epub(&f.sources, "1.epub", "Livro", "Autor", &[]))
        .unwrap()
        .id();

    let reading = f.library.create_collection("  Lendo   agora ").unwrap();
    let empty = f.library.create_collection("Teologia").unwrap();
    assert!(f.library.create_collection("lendo agora").is_err());
    assert!(f.library.create_collection("   ").is_err());

    f.library.add_to_collection(reading, id).unwrap();
    f.library.add_to_collection(reading, id).unwrap();
    let collections = f.library.collections().unwrap();
    let counts: Vec<_> = collections.iter().map(|c| (c.name.as_str(), c.count)).collect();
    assert_eq!(counts, [("Lendo agora", 1), ("Teologia", 0)]);

    let filtered = f
        .library
        .books(&BookQuery {
            filter: Filter::Collection(reading),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(f.library.book(id).unwrap().collections[0].name, "Lendo agora");

    assert!(f.library.rename_collection(empty, "Lendo agora").is_err());
    f.library.rename_collection(empty, "Filosofia").unwrap();
    f.library.remove_from_collection(reading, id).unwrap();
    f.library.delete_collection(reading).unwrap();
    let names: Vec<_> = f
        .library
        .collections()
        .unwrap()
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert_eq!(names, ["Filosofia"]);
}

#[test]
fn deleting_removes_files_and_orphans() {
    let mut f = fixture();
    let keep = f
        .library
        .import(&write_epub(&f.sources, "1.epub", "Fica", "Autor A", &[]))
        .unwrap()
        .id();
    let gone = f
        .library
        .import(&write_epub(&f.sources, "2.epub", "Sai", "Autor B", &["só dele"]))
        .unwrap()
        .id();
    let folder = f.library.book(gone).unwrap().folder;

    f.library.delete(gone).unwrap();

    assert!(!folder.exists());
    assert!(!folder.parent().unwrap().exists());
    assert_eq!(all(&f.library), ["Fica"]);
    assert_eq!(f.library.authors().unwrap().len(), 1);
    assert!(f.library.tags().unwrap().is_empty());
    assert!(
        f.library
            .books(&BookQuery {
                text: "sai".into(),
                ..Default::default()
            })
            .unwrap()
            .is_empty()
    );
    assert!(f.library.book(keep).is_ok());
}

#[test]
fn reopening_keeps_data() {
    let f = fixture();
    let root = f.library.root().to_path_buf();
    let mut library = f.library;
    library
        .import(&write_epub(&f.sources, "1.epub", "Persistente", "Autor", &[]))
        .unwrap();
    drop(library);

    let reopened = Library::open(&root).unwrap();
    assert_eq!(all(&reopened), ["Persistente"]);
}

#[test]
fn stores_and_edits_highlights() {
    use r_ereader_library::{HighlightColor, NewHighlight};

    let mut f = fixture();
    let id = f
        .library
        .import(&write_epub(&f.sources, "1.epub", "Livro", "Autor", &[]))
        .unwrap()
        .id();
    let new = |chapter, start, end, quote: &str| NewHighlight {
        chapter,
        chapter_title: None,
        start,
        end,
        quote: quote.into(),
        color: HighlightColor::Yellow,
    };

    let later = f.library.add_highlight(id, &new(2, 0, 3, "Fim")).unwrap();
    let first = f.library.add_highlight(id, &new(0, 4, 7, "Era")).unwrap();
    assert!(f.library.add_highlight(id, &new(0, 5, 5, "")).is_err());

    let all = f.library.highlights(id).unwrap();
    assert_eq!(all.iter().map(|h| h.id).collect::<Vec<_>>(), [first, later]);

    f.library
        .set_highlight_color(first, HighlightColor::Pink)
        .unwrap();
    f.library.set_highlight_note(first, "  lembrar disso  ").unwrap();
    f.library.move_highlight(first, 10, 13).unwrap();
    let h = &f.library.highlights(id).unwrap()[0];
    assert_eq!(
        (h.color, h.note.as_deref(), h.start, h.end),
        (HighlightColor::Pink, Some("lembrar disso"), 10, 13)
    );

    f.library.set_highlight_note(first, "   ").unwrap();
    assert_eq!(f.library.highlights(id).unwrap()[0].note, None);
    assert!(f.library.set_highlight_note(9999, "x").is_err());

    f.library.delete_highlight(later).unwrap();
    assert_eq!(f.library.highlights(id).unwrap().len(), 1);

    // Apagar o livro leva os destaques junto.
    f.library.delete(id).unwrap();
    assert!(f.library.highlights(id).unwrap().is_empty());
}

#[test]
fn migrates_existing_v1_database() {
    let f = fixture();
    let root = f.library.root().to_path_buf();
    drop(f.library);
    // Simula um banco criado antes dos destaques.
    let conn = rusqlite::Connection::open(root.join("metadata.db")).unwrap();
    conn.execute_batch("DROP TABLE highlights; DROP TABLE settings; PRAGMA user_version = 1;")
        .unwrap();
    drop(conn);

    let library = Library::open(&root).unwrap();
    assert!(library.highlights(1).unwrap().is_empty());
}

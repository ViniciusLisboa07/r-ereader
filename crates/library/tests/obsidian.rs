use std::fs;
use std::path::{Path, PathBuf};

use r_ereader_library::obsidian::{self, FOLDER_SETTING};
use r_ereader_library::testing::write_epub;
use r_ereader_library::{BookId, HighlightColor, Library, NewHighlight, Progress};
use tempfile::TempDir;

struct Fixture {
    dir: TempDir,
    library: Library,
    book: BookId,
    /// `<vault>/Leituras`, com `<vault>/.obsidian` existente.
    folder: PathBuf,
}

fn fixture() -> Fixture {
    let dir = TempDir::new().unwrap();
    let sources = dir.path().join("origem");
    fs::create_dir_all(&sources).unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(vault.join(".obsidian")).unwrap();
    let mut library = Library::open(dir.path().join("biblioteca")).unwrap();
    let book = library
        .import(&write_epub(
            &sources,
            "a.epub",
            "Absalão, Absalão!",
            "William Faulkner",
            &["Ficção Americana"],
        ))
        .unwrap()
        .id();
    Fixture {
        folder: vault.join("Leituras"),
        dir,
        library,
        book,
    }
}

fn highlight(f: &Fixture, chapter: usize, title: &str, quote: &str, color: HighlightColor) -> i64 {
    f.library
        .add_highlight(
            f.book,
            &NewHighlight {
                chapter,
                chapter_title: Some(title.into()),
                start: 0,
                end: quote.len(),
                quote: quote.into(),
                color,
            },
        )
        .unwrap()
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

#[test]
fn writes_one_note_per_book_grouped_by_chapter() {
    let f = fixture();
    let first = highlight(&f, 0, "I", "Era uma vez", HighlightColor::Yellow);
    let second = highlight(
        &f,
        2,
        "III",
        "Primeiro parágrafo\nSegundo # parágrafo",
        HighlightColor::Pink,
    );
    f.library.set_highlight_note(second, "volta no fim").unwrap();
    f.library
        .save_progress(
            f.book,
            Progress {
                chapter: 1,
                fraction: 0.5,
            },
        )
        .unwrap();

    let path = obsidian::export_book(&f.library, f.book, &f.folder)
        .unwrap()
        .unwrap();
    assert_eq!(path, f.folder.join("Absalão, Absalão! — William Faulkner.md"));

    let note = read(&path);
    assert!(note.starts_with(
        "---\nr_ereader_id: 1\ntitulo: \"Absalão, Absalão!\"\nautores:\n  - \"[[William Faulkner]]\"\n"
    ));
    assert!(note.contains("serie: \"Romances do Sul\"\nvolume: 2\n"));
    assert!(note.contains("isbn: \"9788535932584\"\n"));
    assert!(note.contains("progresso: 50\ndestaques: 2\n"));
    assert!(note.contains("tags:\n  - livro\n  - ficção-americana\n---\n"));
    assert!(note.contains("## I\n\n> [!quote|amarelo] Destaque\n> Era uma vez\n\n^rr-"));
    assert!(note.contains(&format!("^rr-{first}\n")));
    assert!(note.contains(
        "## III\n\n> [!quote|rosa] Destaque\n> Primeiro parágrafo\n>\n> Segundo # parágrafo\n>\n> **Nota:** volta no fim\n"
    ));
    assert!(note.trim_end().ends_with("%% r-ereader:fim %%"));
}

#[test]
fn preserves_user_writing_when_resyncing() {
    let f = fixture();
    highlight(&f, 0, "I", "Era uma vez", HighlightColor::Yellow);
    let path = obsidian::export_book(&f.library, f.book, &f.folder)
        .unwrap()
        .unwrap();

    // O usuário escreve na nota, acrescenta uma propriedade e troca as tags.
    let edited = read(&path).replace(
        "tags:\n  - livro\n  - ficção-americana\n",
        "tags:\n  - favorito\nnota: 5\n",
    ) + "\n## Minhas reflexões\n\nIsso me lembrou Faulkner.\n";
    fs::write(&path, edited).unwrap();

    highlight(&f, 1, "II", "No meio", HighlightColor::Green);
    obsidian::export_book(&f.library, f.book, &f.folder).unwrap();

    let note = read(&path);
    assert!(note.contains("destaques: 2\n"));
    assert!(note.contains("tags:\n  - favorito\nnota: 5\n"));
    assert!(!note.contains("ficção-americana"));
    assert!(note.contains("> No meio"));
    assert!(note.ends_with("## Minhas reflexões\n\nIsso me lembrou Faulkner.\n"));
    assert_eq!(note.matches("%% r-ereader:inicio").count(), 1);
}

#[test]
fn finds_renamed_and_moved_notes_by_id() {
    let f = fixture();
    highlight(&f, 0, "I", "Era uma vez", HighlightColor::Yellow);
    let original = obsidian::export_book(&f.library, f.book, &f.folder)
        .unwrap()
        .unwrap();

    let moved = f.folder.join("Romances").join("Faulkner — favorito.md");
    fs::create_dir_all(moved.parent().unwrap()).unwrap();
    fs::rename(&original, &moved).unwrap();

    let path = obsidian::export_book(&f.library, f.book, &f.folder)
        .unwrap()
        .unwrap();
    assert_eq!(path, moved);
    assert!(!original.exists());
}

#[test]
fn does_not_overwrite_unrelated_note_with_same_name() {
    let f = fixture();
    fs::create_dir_all(&f.folder).unwrap();
    let other = f.folder.join("Absalão, Absalão! — William Faulkner.md");
    fs::write(&other, "nota escrita à mão").unwrap();
    highlight(&f, 0, "I", "Era uma vez", HighlightColor::Yellow);

    let path = obsidian::export_book(&f.library, f.book, &f.folder)
        .unwrap()
        .unwrap();
    assert_ne!(path, other);
    assert_eq!(read(&other), "nota escrita à mão");
}

#[test]
fn skips_books_without_highlights_until_they_have_a_note() {
    let f = fixture();
    assert_eq!(
        obsidian::export_book(&f.library, f.book, &f.folder).unwrap(),
        None
    );
    assert!(!f.folder.exists());

    let id = highlight(&f, 0, "I", "Era uma vez", HighlightColor::Yellow);
    let path = obsidian::export_book(&f.library, f.book, &f.folder)
        .unwrap()
        .unwrap();
    // Removido o último destaque, a nota continua e diz que está vazia.
    f.library.delete_highlight(id).unwrap();
    obsidian::export_book(&f.library, f.book, &f.folder).unwrap();
    assert!(read(&path).contains("*Nenhum destaque ainda.*"));
}

#[test]
fn installs_color_snippet_once() {
    let f = fixture();
    highlight(&f, 0, "I", "Era uma vez", HighlightColor::Yellow);
    obsidian::export_book(&f.library, f.book, &f.folder).unwrap();
    let snippet = f
        .dir
        .path()
        .join("vault/.obsidian/snippets/r-ereader-destaques.css");
    assert!(read(&snippet).contains("amarelo"));

    // Se o usuário editar o snippet, não sobrescrevemos.
    fs::write(&snippet, "/* meu */").unwrap();
    obsidian::export_book(&f.library, f.book, &f.folder).unwrap();
    assert_eq!(read(&snippet), "/* meu */");
}

#[test]
fn sync_respects_setting_and_export_all_counts_notes() {
    let f = fixture();
    highlight(&f, 0, "I", "Era uma vez", HighlightColor::Yellow);

    f.library.set_setting(FOLDER_SETTING, "").unwrap();
    assert_eq!(obsidian::sync_book(&f.library, f.book).unwrap(), None);

    f.library
        .set_setting(FOLDER_SETTING, &f.folder.to_string_lossy())
        .unwrap();
    assert!(obsidian::sync_book(&f.library, f.book).unwrap().is_some());
    assert_eq!(obsidian::export_all(&f.library, &f.folder).unwrap(), 1);
}

#[test]
fn default_configuration_is_saved_once() {
    let f = fixture();
    let vault = f.dir.path().join("vault");
    let detected = obsidian::configure_default_with(&f.library, || Some(vault.clone())).unwrap();
    assert_eq!(detected, Some(vault.join("Leituras")));

    // Depois de configurado, a detecção não roda de novo nem troca a escolha.
    f.library.set_setting(FOLDER_SETTING, "").unwrap();
    let again = obsidian::configure_default_with(&f.library, || panic!("não deveria detectar")).unwrap();
    assert_eq!(again, None);
}

#[test]
fn without_vault_export_starts_disabled() {
    let f = fixture();
    assert_eq!(
        obsidian::configure_default_with(&f.library, || None).unwrap(),
        None
    );
    assert_eq!(f.library.setting(FOLDER_SETTING).unwrap().as_deref(), Some(""));
}

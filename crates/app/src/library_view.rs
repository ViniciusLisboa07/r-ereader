//! Tela da biblioteca: navegador lateral, grade/lista de livros e painel de detalhes.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, ClickEvent, Context, Entity, EventEmitter, ExternalPaths, FocusHandle, Focusable, FontWeight,
    PathPromptOptions, ScrollStrategy, SharedString, Subscription, UniformListScrollHandle, Window, actions,
    div, prelude::*, px, relative, rgb, uniform_list,
};
use r_ereader_library::{
    BookDetails, BookId, BookQuery, BookSummary, Facet, Filter, Format, ImportOutcome, Library, MetadataEdit,
    SortOrder, obsidian, text::split_list,
};

use crate::components::{
    STATUS_HEIGHT, bar, button, catalog_number, chip, cover, danger_button, disabled_button, kbd, mono,
    outline_button, primary_button, progress_bar, section_label, status_segment,
};
use crate::input::{InputEvent, TextInput};
use crate::nav;
use crate::theme;

actions!(
    library,
    [
        ImportBooks,
        FocusSearch,
        Dismiss,
        ShowGrid,
        ShowList,
        EditBook,
        DeleteBook
    ]
);

pub const CONTEXT: &str = "Library";

const SIDEBAR_WIDTH: f32 = 248.;
const DETAILS_WIDTH: f32 = 340.;
/// Largura mínima de uma célula da gaveta; as células esticam para ocupar a linha.
const CELL_MIN_WIDTH: f32 = 176.;
const COVER_WIDTH: f32 = 116.;
const COVER_HEIGHT: f32 = 174.;
const GRID_ROW_HEIGHT: f32 = 314.;
const LIST_ROW_HEIGHT: f32 = 46.;

pub enum LibraryEvent {
    Read(BookId),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Section {
    Collections,
    Authors,
    Series,
    Tags,
    Formats,
}

impl Section {
    const ALL: [Section; 5] = [
        Section::Collections,
        Section::Authors,
        Section::Series,
        Section::Tags,
        Section::Formats,
    ];

    fn title(self) -> &'static str {
        match self {
            Section::Collections => "Coleções",
            Section::Authors => "Autores",
            Section::Series => "Séries",
            Section::Tags => "Tags",
            Section::Formats => "Formatos",
        }
    }
}

/// Linha do navegador lateral: um filtro ou o título de uma seção.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NavEntry {
    Filter(Filter),
    Section(Section),
}

struct NavRow {
    entry: NavEntry,
    label: SharedString,
    count: i64,
}

impl NavRow {
    fn filter(filter: Filter, label: impl Into<SharedString>, count: i64) -> Self {
        NavRow {
            entry: NavEntry::Filter(filter),
            label: label.into(),
            count,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Grid,
    List,
}

#[derive(Default)]
struct Sidebar {
    total: i64,
    reading: i64,
    collections: Vec<Facet>,
    authors: Vec<Facet>,
    series: Vec<Facet>,
    tags: Vec<Facet>,
    formats: Vec<(Format, i64)>,
    /// Pasta das notas no Obsidian; `None` = exportação desativada.
    obsidian: Option<PathBuf>,
}

enum NoticeKind {
    Info,
    Error,
}

struct Notice {
    text: SharedString,
    kind: NoticeKind,
}

struct Editor {
    title: Entity<TextInput>,
    authors: Entity<TextInput>,
    series: Entity<TextInput>,
    series_index: Entity<TextInput>,
    tags: Entity<TextInput>,
    publisher: Entity<TextInput>,
    language: Entity<TextInput>,
    published: Entity<TextInput>,
    _subscriptions: Vec<Subscription>,
}

pub struct LibraryView {
    library: Entity<Library>,
    focus_handle: FocusHandle,
    /// Foco da grade/lista: é onde as setas escolhem livros.
    books_focus: FocusHandle,
    sidebar_focus: FocusHandle,
    sidebar_cursor: usize,
    /// Colunas da grade no último desenho (as setas ↑↓ pulam uma linha inteira).
    columns: usize,
    search: Entity<TextInput>,
    query: BookQuery,
    books: Vec<BookSummary>,
    selected: Option<BookId>,
    details: Option<BookDetails>,
    sidebar: Sidebar,
    expanded: HashSet<Section>,
    view_mode: ViewMode,
    editor: Option<Editor>,
    new_collection: Option<(Entity<TextInput>, Subscription)>,
    collection_picker_open: bool,
    confirm_delete: bool,
    importing: Option<(usize, usize)>,
    notice: Option<Notice>,
    scroll: UniformListScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LibraryEvent> for LibraryView {}

impl Focusable for LibraryView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.books_focus.clone()
    }
}

impl LibraryView {
    /// Qual painel tem o foco (para testes de navegação).
    #[cfg(test)]
    pub fn focused_pane(&self, window: &Window, cx: &gpui::App) -> &'static str {
        if self.books_focus.is_focused(window) {
            "livros"
        } else if self.sidebar_focus.is_focused(window) {
            "navegador"
        } else if self.search.focus_handle(cx).is_focused(window) {
            "busca"
        } else {
            "outro"
        }
    }

    pub fn new(library: Entity<Library>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| TextInput::new("Buscar título, autor, série, tag…", cx));
        let subscription = cx.subscribe_in(
            &search,
            window,
            |this: &mut Self, search, event: &InputEvent, window, cx| match event {
                InputEvent::Changed => {
                    this.query.text = search.read(cx).text().to_owned();
                    this.reload(cx);
                    this.scroll_to_top();
                }
                InputEvent::Cancel => {
                    search.update(cx, |input, cx| input.set_text("", cx));
                    this.query.text.clear();
                    this.reload(cx);
                }
                // Enter leva aos resultados, já com o primeiro escolhido.
                InputEvent::Submit => {
                    if this.selected_index().is_none() {
                        this.move_selection(0, cx);
                    }
                    window.focus(&this.books_focus);
                }
            },
        );

        let mut view = LibraryView {
            library,
            focus_handle: cx.focus_handle(),
            books_focus: cx.focus_handle().tab_stop(true),
            sidebar_focus: cx.focus_handle().tab_stop(true),
            sidebar_cursor: 0,
            columns: 1,
            search,
            query: BookQuery::default(),
            books: Vec::new(),
            selected: None,
            details: None,
            sidebar: Sidebar::default(),
            expanded: HashSet::from([Section::Collections, Section::Authors]),
            view_mode: ViewMode::Grid,
            editor: None,
            new_collection: None,
            collection_picker_open: false,
            confirm_delete: false,
            importing: None,
            notice: None,
            scroll: UniformListScrollHandle::new(),
            _subscriptions: vec![subscription],
        };
        view.reload(cx);
        view
    }

    /// Recarrega livros, navegador e detalhes a partir do banco.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let library = self.library.read(cx);
        let result = (|| -> r_ereader_library::Result<_> {
            let sidebar = Sidebar {
                total: library.book_count()?,
                reading: library.reading_count()?,
                collections: library.collections()?,
                authors: library.authors()?,
                series: library.series()?,
                tags: library.tags()?,
                formats: library.formats()?,
                obsidian: obsidian::folder(library)?,
            };
            let books = library.books(&self.query)?;
            let details = match self.selected {
                Some(id) => library.book(id).ok(),
                None => None,
            };
            Ok((sidebar, books, details))
        })();

        match result {
            Ok((sidebar, books, details)) => {
                self.sidebar = sidebar;
                self.books = books;
                if details.is_none() {
                    self.selected = None;
                    self.editor = None;
                }
                self.details = details;
            }
            Err(e) => self.show_error(e.to_string(), cx),
        }
        cx.notify();
    }

    fn scroll_to_top(&self) {
        self.scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
    }

    #[cfg(test)]
    pub fn reading_count(&self) -> i64 {
        self.sidebar.reading
    }

    #[cfg(test)]
    pub fn has_error(&self) -> bool {
        matches!(
            self.notice,
            Some(Notice {
                kind: NoticeKind::Error,
                ..
            })
        )
    }

    pub fn show_error(&mut self, text: String, cx: &mut Context<Self>) {
        self.notice = Some(Notice {
            text: text.into(),
            kind: NoticeKind::Error,
        });
        cx.notify();
    }

    fn set_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        self.query.filter = filter;
        self.reload(cx);
        self.scroll_to_top();
    }

    fn select(&mut self, id: BookId, cx: &mut Context<Self>) {
        if self.selected != Some(id) {
            self.selected = Some(id);
            self.editor = None;
            self.collection_picker_open = false;
            self.confirm_delete = false;
            self.reload(cx);
        }
    }

    fn read(&mut self, id: BookId, cx: &mut Context<Self>) {
        cx.emit(LibraryEvent::Read(id));
    }

    // ── Importação ─────────────────────────────────────────────────────────

    fn import_books(&mut self, _: &ImportBooks, _: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Importar livros".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            this.update(cx, |this, cx| this.import_paths(paths, cx)).ok();
        })
        .detach();
    }

    pub fn import_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if self.importing.is_some() {
            self.show_error("Espere a importação atual terminar.".into(), cx);
            cx.notify();
            return;
        }
        let files = importable_files(&paths);
        if files.is_empty() {
            self.notice = Some(Notice {
                text: "Nenhum arquivo EPUB ou PDF encontrado.".into(),
                kind: NoticeKind::Info,
            });
            cx.notify();
            return;
        }

        let root = self.library.read(cx).root().to_path_buf();
        let total = files.len();
        self.importing = Some((0, total));
        self.notice = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let mut report = ImportReport::default();
            for (index, path) in files.into_iter().enumerate() {
                let root = root.clone();
                let source = path.clone();
                // Cada arquivo usa sua própria conexão numa thread de fundo; o WAL do
                // SQLite deixa a interface continuar lendo enquanto isso.
                let result = cx
                    .background_spawn(async move { Library::open(&root).and_then(|mut l| l.import(&source)) })
                    .await;
                report.record(&path, result);
                this.update(cx, |this, cx| {
                    this.importing = Some((index + 1, total));
                    this.reload(cx);
                })
                .ok();
            }
            this.update(cx, |this, cx| {
                this.importing = None;
                this.notice = Some(report.notice());
                this.reload(cx);
            })
            .ok();
        })
        .detach();
    }

    // ── Edição ─────────────────────────────────────────────────────────────

    fn start_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(details) = &self.details else { return };
        let summary = &details.summary;
        let series = summary.series.clone();

        let field = |cx: &mut Context<Self>, placeholder: &str, value: String| {
            cx.new(|cx| {
                let mut input = TextInput::new(placeholder.to_owned(), cx);
                input.set_text(value, cx);
                input
            })
        };
        let title = field(cx, "Título", summary.title.clone());
        let authors = field(cx, "Separe vários com ;", summary.authors.join("; "));
        let series_name = field(
            cx,
            "Nenhuma",
            series.as_ref().map(|s| s.name.clone()).unwrap_or_default(),
        );
        let series_index = field(
            cx,
            "—",
            series
                .and_then(|s| s.index)
                .map(format_series_index)
                .unwrap_or_default(),
        );
        let tags = field(cx, "Separe com vírgula", details.tags.join(", "));
        let publisher = field(cx, "—", details.publisher.clone().unwrap_or_default());
        let language = field(cx, "pt, en…", details.language.clone().unwrap_or_default());
        let published = field(cx, "AAAA-MM-DD", details.published.clone().unwrap_or_default());

        let inputs = [
            &title,
            &authors,
            &series_name,
            &series_index,
            &tags,
            &publisher,
            &language,
            &published,
        ];
        let subscriptions = inputs
            .iter()
            .map(|input| {
                cx.subscribe_in(
                    *input,
                    window,
                    |this: &mut Self, _, event: &InputEvent, window, cx| match event {
                        InputEvent::Submit => {
                            this.save_edit(cx);
                            if this.editor.is_none() {
                                window.focus(&this.books_focus);
                            }
                        }
                        InputEvent::Cancel => {
                            this.editor = None;
                            window.focus(&this.books_focus);
                            cx.notify();
                        }
                        InputEvent::Changed => {}
                    },
                )
            })
            .collect();

        window.focus(&title.focus_handle(cx));
        self.editor = Some(Editor {
            title,
            authors,
            series: series_name,
            series_index,
            tags,
            publisher,
            language,
            published,
            _subscriptions: subscriptions,
        });
        self.confirm_delete = false;
        cx.notify();
    }

    fn save_edit(&mut self, cx: &mut Context<Self>) {
        let (Some(editor), Some(details)) = (&self.editor, &self.details) else {
            return;
        };
        let value = |input: &Entity<TextInput>| input.read(cx).text().trim().to_owned();
        let optional = |input: &Entity<TextInput>| Some(value(input)).filter(|v| !v.is_empty());

        let index_text = value(&editor.series_index).replace(',', ".");
        let series_index = if index_text.is_empty() {
            None
        } else {
            match index_text.parse::<f64>() {
                Ok(index) => Some(index),
                Err(_) => {
                    self.show_error(format!("\"{index_text}\" não é um número de volume válido."), cx);
                    cx.notify();
                    return;
                }
            }
        };

        let edit = MetadataEdit {
            title: value(&editor.title),
            authors: value(&editor.authors)
                .split([';', '&'])
                .map(str::trim)
                .filter(|a| !a.is_empty())
                .map(str::to_owned)
                .collect(),
            series: optional(&editor.series),
            series_index,
            tags: split_list(&value(&editor.tags)),
            publisher: optional(&editor.publisher),
            language: optional(&editor.language),
            published: optional(&editor.published),
            description: details.description.clone(),
        };
        let id = details.summary.id;

        match self
            .library
            .update(cx, |library, _| library.update_metadata(id, &edit))
        {
            Ok(()) => {
                self.editor = None;
                self.notice = None;
            }
            Err(e) => self.show_error(e.to_string(), cx),
        }
        self.reload(cx);
    }

    fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else { return };
        match self.library.update(cx, |library, _| library.delete(id)) {
            Ok(()) => {
                self.selected = None;
                self.details = None;
                self.confirm_delete = false;
            }
            Err(e) => self.show_error(e.to_string(), cx),
        }
        self.reload(cx);
    }

    // ── Obsidian ───────────────────────────────────────────────────────────

    fn set_obsidian_folder(&mut self, folder: Option<PathBuf>, cx: &mut Context<Self>) {
        let value = folder
            .as_ref()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Err(e) = self
            .library
            .read(cx)
            .set_setting(obsidian::FOLDER_SETTING, &value)
        {
            self.show_error(e.to_string(), cx);
        }
        self.reload(cx);
    }

    fn choose_obsidian_folder(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Pasta das anotações no Obsidian".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(folder) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |this, cx| {
                this.set_obsidian_folder(Some(folder), cx);
                this.export_all_to_obsidian(cx);
            })
            .ok();
        })
        .detach();
    }

    fn export_all_to_obsidian(&mut self, cx: &mut Context<Self>) {
        let Some(folder) = self.sidebar.obsidian.clone() else {
            return;
        };
        match obsidian::export_all(self.library.read(cx), &folder) {
            Ok(count) => {
                self.notice = Some(Notice {
                    text: format!(
                        "{} no Obsidian ({}).",
                        plural(count, "nota atualizada", "notas atualizadas"),
                        display_path(&folder)
                    )
                    .into(),
                    kind: NoticeKind::Info,
                });
                cx.notify();
            }
            Err(e) => self.show_error(format!("Obsidian: {e}"), cx),
        }
    }

    // ── Coleções ───────────────────────────────────────────────────────────

    fn start_new_collection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new("Nome da coleção", cx));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, input, event: &InputEvent, window, cx| match event {
                InputEvent::Submit => {
                    let name = input.read(cx).text().to_owned();
                    match this
                        .library
                        .update(cx, |library, _| library.create_collection(&name))
                    {
                        Ok(_) => {
                            this.new_collection = None;
                            window.focus(&this.sidebar_focus);
                        }
                        Err(e) => this.show_error(e.to_string(), cx),
                    }
                    this.reload(cx);
                }
                InputEvent::Cancel => {
                    this.new_collection = None;
                    window.focus(&this.sidebar_focus);
                    cx.notify();
                }
                InputEvent::Changed => {}
            },
        );
        window.focus(&input.focus_handle(cx));
        self.expanded.insert(Section::Collections);
        self.new_collection = Some((input, subscription));
        cx.notify();
    }

    fn delete_collection(&mut self, id: i64, cx: &mut Context<Self>) {
        if let Err(e) = self
            .library
            .update(cx, |library, _| library.delete_collection(id))
        {
            self.show_error(e.to_string(), cx);
        }
        if self.query.filter == Filter::Collection(id) {
            self.query.filter = Filter::All;
        }
        self.reload(cx);
    }

    fn toggle_collection(&mut self, collection: i64, add: bool, cx: &mut Context<Self>) {
        let Some(book) = self.selected else { return };
        let result = self.library.update(cx, |library, _| {
            if add {
                library.add_to_collection(collection, book)
            } else {
                library.remove_from_collection(collection, book)
            }
        });
        if let Err(e) = result {
            self.show_error(e.to_string(), cx);
        }
        self.collection_picker_open = false;
        self.reload(cx);
    }

    // ── Teclado ────────────────────────────────────────────────────────────

    fn focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.search.focus_handle(cx));
    }

    fn dismiss(&mut self, _: &Dismiss, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.take().is_none() && !std::mem::take(&mut self.confirm_delete) {
            self.selected = None;
            self.details = None;
        }
        self.collection_picker_open = false;
        if !self.sidebar_focus.is_focused(window) {
            window.focus(&self.books_focus);
        }
        cx.notify();
    }

    fn show_grid(&mut self, _: &ShowGrid, _: &mut Window, cx: &mut Context<Self>) {
        self.set_view_mode(ViewMode::Grid, cx);
    }

    fn show_list(&mut self, _: &ShowList, _: &mut Window, cx: &mut Context<Self>) {
        self.set_view_mode(ViewMode::List, cx);
    }

    fn set_view_mode(&mut self, mode: ViewMode, cx: &mut Context<Self>) {
        self.view_mode = mode;
        if let Some(index) = self.selected_index() {
            self.reveal(index);
        }
        cx.notify();
    }

    fn edit_book(&mut self, _: &EditBook, window: &mut Window, cx: &mut Context<Self>) {
        if self.details.is_some() && self.editor.is_none() {
            self.start_edit(window, cx);
        }
    }

    /// Del arma a exclusão; um segundo Del (ou o botão) confirma.
    fn delete_book(&mut self, _: &DeleteBook, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_none() {
            return;
        }
        if self.confirm_delete {
            self.delete_selected(cx);
        } else {
            self.confirm_delete = true;
            self.editor = None;
            cx.notify();
        }
    }

    fn selected_index(&self) -> Option<usize> {
        let id = self.selected?;
        self.books.iter().position(|book| book.id == id)
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(index) = nav::step(self.selected_index(), self.books.len(), delta) else {
            return;
        };
        self.select(self.books[index].id, cx);
        self.reveal(index);
        cx.notify();
    }

    /// Rola o mínimo para mostrar o livro.
    fn reveal(&self, index: usize) {
        let item = match self.view_mode {
            ViewMode::Grid => index / self.columns.max(1),
            ViewMode::List => index,
        };
        self.scroll.scroll_to_item(item, ScrollStrategy::Top);
    }

    fn row_step(&self) -> isize {
        match self.view_mode {
            ViewMode::Grid => self.columns.max(1) as isize,
            ViewMode::List => 1,
        }
    }

    fn page_step(&self, window: &Window) -> isize {
        let row_height = match self.view_mode {
            ViewMode::Grid => GRID_ROW_HEIGHT,
            ViewMode::List => LIST_ROW_HEIGHT,
        };
        let visible = (window.viewport_size().height / px(row_height)).floor() as isize - 1;
        visible.max(1) * self.row_step()
    }

    fn books_up(&mut self, _: &nav::MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(-self.row_step(), cx);
    }

    fn books_down(&mut self, _: &nav::MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(self.row_step(), cx);
    }

    fn books_left(&mut self, _: &nav::MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(-1, cx);
    }

    fn books_right(&mut self, _: &nav::MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(1, cx);
    }

    fn books_page_up(&mut self, _: &nav::PageUp, window: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(-self.page_step(window), cx);
    }

    fn books_page_down(&mut self, _: &nav::PageDown, window: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(self.page_step(window), cx);
    }

    fn books_first(&mut self, _: &nav::MoveFirst, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(isize::MIN / 2, cx);
    }

    fn books_last(&mut self, _: &nav::MoveLast, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(isize::MAX / 2, cx);
    }

    fn books_confirm(&mut self, _: &nav::Confirm, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.selected {
            self.read(id, cx);
        }
    }

    // ── Navegador lateral ──────────────────────────────────────────────────

    /// Linhas navegáveis do navegador, na ordem em que aparecem.
    fn nav_rows(&self) -> Vec<NavRow> {
        let sidebar = &self.sidebar;
        let mut rows = vec![
            NavRow::filter(Filter::All, "Todos os livros", sidebar.total),
            NavRow::filter(Filter::Reading, "Em leitura", sidebar.reading),
        ];
        let facet_rows = |facets: &[Facet], to_filter: fn(i64) -> Filter| -> Vec<NavRow> {
            facets
                .iter()
                .map(|f| NavRow::filter(to_filter(f.id), f.name.clone(), f.count))
                .collect()
        };
        for section in Section::ALL {
            let children = match section {
                Section::Collections => facet_rows(&sidebar.collections, Filter::Collection),
                Section::Authors => facet_rows(&sidebar.authors, Filter::Author),
                Section::Series => facet_rows(&sidebar.series, Filter::Series),
                Section::Tags => facet_rows(&sidebar.tags, Filter::Tag),
                Section::Formats => sidebar
                    .formats
                    .iter()
                    .map(|(format, count)| NavRow::filter(Filter::Format(*format), format.as_str(), *count))
                    .collect(),
            };
            rows.push(NavRow {
                entry: NavEntry::Section(section),
                label: section.title().into(),
                count: children.len() as i64,
            });
            if self.expanded.contains(&section) {
                rows.extend(children);
            }
        }
        rows
    }

    fn toggle_section(&mut self, section: Section, cx: &mut Context<Self>) {
        if !self.expanded.remove(&section) {
            self.expanded.insert(section);
        }
        cx.notify();
    }

    fn sidebar_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.nav_rows().len();
        self.sidebar_cursor = nav::step(Some(self.sidebar_cursor), len, delta).unwrap_or(0);
        cx.notify();
    }

    fn sidebar_up(&mut self, _: &nav::MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_move(-1, cx);
    }

    fn sidebar_down(&mut self, _: &nav::MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_move(1, cx);
    }

    fn sidebar_page_up(&mut self, _: &nav::PageUp, _: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_move(-10, cx);
    }

    fn sidebar_page_down(&mut self, _: &nav::PageDown, _: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_move(10, cx);
    }

    fn sidebar_first(&mut self, _: &nav::MoveFirst, _: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_move(isize::MIN / 2, cx);
    }

    fn sidebar_last(&mut self, _: &nav::MoveLast, _: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_move(isize::MAX / 2, cx);
    }

    /// ← fecha a seção sob o cursor, ou sobe do item para o título da seção.
    fn sidebar_left(&mut self, _: &nav::MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        let rows = self.nav_rows();
        let Some(row) = rows.get(self.sidebar_cursor) else {
            return;
        };
        match row.entry {
            NavEntry::Section(section) if self.expanded.contains(&section) => {
                self.toggle_section(section, cx)
            }
            NavEntry::Section(_) => {}
            NavEntry::Filter(_) => {
                if let Some(header) = rows[..self.sidebar_cursor]
                    .iter()
                    .rposition(|r| matches!(r.entry, NavEntry::Section(_)))
                {
                    self.sidebar_cursor = header;
                    cx.notify();
                }
            }
        }
    }

    /// → abre a seção sob o cursor.
    fn sidebar_right(&mut self, _: &nav::MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(NavEntry::Section(section)) = self.nav_rows().get(self.sidebar_cursor).map(|r| r.entry)
            && !self.expanded.contains(&section)
        {
            self.toggle_section(section, cx);
        }
    }

    fn sidebar_confirm(&mut self, _: &nav::Confirm, _: &mut Window, cx: &mut Context<Self>) {
        match self.nav_rows().get(self.sidebar_cursor).map(|r| r.entry) {
            Some(NavEntry::Filter(filter)) => self.set_filter(filter, cx),
            Some(NavEntry::Section(section)) => self.toggle_section(section, cx),
            None => {}
        }
    }

    // ── Renderização ───────────────────────────────────────────────────────

    fn render_sidebar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let focused = self.sidebar_focus.is_focused(window);
        let rows = self.nav_rows();
        let cursor = self.sidebar_cursor.min(rows.len().saturating_sub(1));
        let filter = self.query.filter;

        let mut list = div().flex().flex_col().pb_2();
        for (index, row) in rows.iter().enumerate() {
            let is_cursor = focused && index == cursor;
            list = match row.entry {
                NavEntry::Section(section) => {
                    let expanded = self.expanded.contains(&section);
                    let empty_hint_text = match section {
                        Section::Collections if self.new_collection.is_none() => {
                            Some("Use + para criar uma estante")
                        }
                        Section::Collections => None,
                        _ => Some("Nada por aqui ainda"),
                    };
                    list.child(self.render_section_header(index, section, row, is_cursor, cx))
                        .when(expanded && row.count == 0, |this| {
                            this.children(empty_hint_text.map(empty_hint))
                        })
                }
                NavEntry::Filter(target) => {
                    list.child(self.render_nav_item(index, target, row, filter == target, is_cursor, cx))
                }
            };
        }

        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .bg(rgb(theme::PANEL))
            .border_r_1()
            .border_color(rgb(theme::BORDER))
            .child(
                bar()
                    .child(
                        div()
                            .font_family(theme::READING_FONT)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(17.))
                            .text_color(rgb(theme::TEXT))
                            .child("r-ereader"),
                    )
                    .child(div().flex_1())
                    .child(
                        button("new-collection", "+ Coleção").text_xs().on_click(
                            cx.listener(|this, _, window, cx| this.start_new_collection(window, cx)),
                        ),
                    ),
            )
            // O campo fica fora do contexto do navegador: ali Espaço e setas são atalhos.
            .when_some(self.new_collection.as_ref(), |this, (input, _)| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .p_3()
                        .border_b_1()
                        .border_color(rgb(theme::BORDER_SUBTLE))
                        .child(section_label("Nova coleção"))
                        .child(input.clone()),
                )
            })
            .child(
                div()
                    .id("sidebar")
                    .track_focus(&self.sidebar_focus)
                    .key_context(nav::SIDEBAR)
                    .on_action(cx.listener(Self::sidebar_up))
                    .on_action(cx.listener(Self::sidebar_down))
                    .on_action(cx.listener(Self::sidebar_left))
                    .on_action(cx.listener(Self::sidebar_right))
                    .on_action(cx.listener(Self::sidebar_page_up))
                    .on_action(cx.listener(Self::sidebar_page_down))
                    .on_action(cx.listener(Self::sidebar_first))
                    .on_action(cx.listener(Self::sidebar_last))
                    .on_action(cx.listener(Self::sidebar_confirm))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
            .child(self.render_obsidian(cx))
    }

    fn render_nav_item(
        &self,
        index: usize,
        target: Filter,
        row: &NavRow,
        active: bool,
        is_cursor: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let nested = !matches!(target, Filter::All | Filter::Reading);
        let collection = match target {
            Filter::Collection(id) => Some(id),
            _ => None,
        };
        div()
            .id(("nav", index))
            .group("nav-row")
            .relative()
            .flex()
            .items_center()
            .gap_2()
            .h(px(26.))
            .pl(px(if nested { 26. } else { 12. }))
            .pr_3()
            .border_1()
            .border_color(focus_ring(is_cursor))
            .text_sm()
            .cursor_pointer()
            .text_color(rgb(if active { theme::TEXT } else { theme::TEXT_MUTED }))
            .when(active, |this| {
                this.bg(rgb(theme::ELEMENT_SELECTED))
                    .font_weight(FontWeight::MEDIUM)
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .bottom_0()
                            .w(px(2.))
                            .bg(rgb(theme::ACCENT)),
                    )
            })
            .hover(|this| this.bg(rgb(theme::ELEMENT_HOVER)))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.sidebar_cursor = index;
                window.focus(&this.sidebar_focus);
                this.set_filter(target, cx);
            }))
            .child(div().flex_1().min_w_0().truncate().child(row.label.clone()))
            .when_some(collection, |this, id| {
                this.child(
                    div()
                        .id(("delete-collection", id as u64))
                        .invisible()
                        .group_hover("nav-row", |this| this.visible())
                        .px_1()
                        .text_xs()
                        .text_color(rgb(theme::TEXT_PLACEHOLDER))
                        .hover(|this| this.text_color(rgb(theme::ERROR)))
                        .child("✕")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.delete_collection(id, cx)
                        })),
                )
            })
            .child(mono(row.count.to_string()).flex_none())
    }

    fn render_section_header(
        &self,
        index: usize,
        section: Section,
        row: &NavRow,
        is_cursor: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let expanded = self.expanded.contains(&section);
        div()
            .mt_2()
            .border_t_1()
            .border_color(rgb(theme::BORDER_SUBTLE))
            .child(
                div()
                    .id(("nav", index))
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(28.))
                    .px_3()
                    .border_1()
                    .border_color(focus_ring(is_cursor))
                    .cursor_pointer()
                    .hover(|this| this.bg(rgb(theme::ELEMENT_HOVER)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.sidebar_cursor = index;
                        window.focus(&this.sidebar_focus);
                        this.toggle_section(section, cx);
                    }))
                    .child(
                        div()
                            .w(px(12.))
                            .text_xs()
                            .text_color(rgb(theme::TEXT_PLACEHOLDER))
                            .child(if expanded { "▾" } else { "▸" }),
                    )
                    .child(section_label(row.label.clone()).flex_1())
                    .child(mono(row.count.to_string())),
            )
    }

    fn render_obsidian(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let folder = self.sidebar.obsidian.clone();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(rgb(theme::BORDER))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(section_label("Obsidian"))
                    .child(mono(if folder.is_some() { "ligado" } else { "desligado" })),
            )
            .child(
                mono(match &folder {
                    Some(folder) => display_path(folder),
                    None => "As anotações ficam só aqui.".to_owned(),
                })
                .truncate(),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .ml(px(-6.))
                    .when(folder.is_some(), |this| {
                        this.child(
                            button("obsidian-export", "Exportar tudo")
                                .text_xs()
                                .on_click(cx.listener(|this, _, _, cx| this.export_all_to_obsidian(cx))),
                        )
                    })
                    .child(
                        button(
                            "obsidian-folder",
                            if folder.is_some() {
                                "Trocar pasta…"
                            } else {
                                "Escolher pasta…"
                            },
                        )
                        .text_xs()
                        .on_click(cx.listener(|this, _, _, cx| this.choose_obsidian_folder(cx))),
                    )
                    .when(folder.is_some(), |this| {
                        this.child(
                            button("obsidian-off", "Desligar")
                                .text_xs()
                                .on_click(cx.listener(|this, _, _, cx| this.set_obsidian_folder(None, cx))),
                        )
                    }),
            )
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let sort_label = match self.query.sort {
            SortOrder::RecentlyAdded => "Recentes",
            SortOrder::Title => "Título",
            SortOrder::Author => "Autor",
            SortOrder::RecentlyRead => "Lidos por último",
        };
        let segment = |id: &'static str, label: &'static str, mode: ViewMode, cx: &mut Context<Self>| {
            let active = self.view_mode == mode;
            div()
                .id(id)
                .px_2()
                .py(px(3.))
                .text_sm()
                .cursor_pointer()
                .text_color(rgb(if active { theme::TEXT } else { theme::TEXT_MUTED }))
                .bg(rgb(if active {
                    theme::ELEMENT_SELECTED
                } else {
                    theme::INPUT
                }))
                .hover(|this| this.text_color(rgb(theme::TEXT)))
                .on_click(cx.listener(move |this, _, _, cx| this.set_view_mode(mode, cx)))
                .child(label)
        };

        bar()
            .gap_3()
            .bg(rgb(theme::PANEL))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .flex_1()
                    .max_w(px(440.))
                    .child(div().flex_1().child(self.search.clone()))
                    .child(kbd("Ctrl+F")),
            )
            .child(div().flex_1())
            .child(
                button("sort", format!("Ordem: {sort_label}")).on_click(cx.listener(|this, _, _, cx| {
                    this.query.sort = match this.query.sort {
                        SortOrder::RecentlyAdded => SortOrder::Title,
                        SortOrder::Title => SortOrder::Author,
                        SortOrder::Author => SortOrder::RecentlyRead,
                        SortOrder::RecentlyRead => SortOrder::RecentlyAdded,
                    };
                    this.reload(cx);
                    this.scroll_to_top();
                })),
            )
            .child(
                div()
                    .flex()
                    .rounded_sm()
                    .overflow_hidden()
                    .border_1()
                    .border_color(rgb(theme::BORDER))
                    .child(segment("grid", "Grade", ViewMode::Grid, cx))
                    .child(div().w(px(1.)).bg(rgb(theme::BORDER)))
                    .child(segment("list", "Lista", ViewMode::List, cx)),
            )
            .child(
                div().flex_none().child(match self.importing {
                    Some((done, total)) => {
                        disabled_button(format!("Importando {done} de {total}…")).into_any_element()
                    }
                    None => primary_button("import", "Importar")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.import_books(&ImportBooks, window, cx)),
                        )
                        .into_any_element(),
                }),
            )
    }

    fn render_notice(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let notice = self.notice.as_ref()?;
        let color = match notice.kind {
            NoticeKind::Info => theme::SUCCESS,
            NoticeKind::Error => theme::ERROR,
        };
        Some(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap_3()
                .pl_3()
                .pr_2()
                .py(px(6.))
                .border_b_1()
                .border_color(rgb(theme::BORDER))
                .bg(rgb(theme::INPUT))
                .child(div().w(px(3.)).h(px(16.)).bg(rgb(color)))
                .child(
                    div()
                        .flex_1()
                        .text_sm()
                        .text_color(rgb(color))
                        .child(notice.text.clone()),
                )
                .child(
                    button("dismiss-notice", "✕").on_click(cx.listener(|this, _, _, cx| {
                        this.notice = None;
                        cx.notify();
                    })),
                ),
        )
    }

    fn render_books(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.sidebar.total == 0 && self.importing.is_none() {
            return self.render_welcome(cx).into_any_element();
        }
        if self.books.is_empty() {
            let message = if self.query.text.is_empty() {
                "Nenhum livro com esse filtro."
            } else {
                "Nenhum livro encontrado para essa busca."
            };
            return div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(rgb(theme::TEXT_PLACEHOLDER))
                .child(message)
                .into_any_element();
        }

        match self.view_mode {
            ViewMode::Grid => {
                let mut width = window.viewport_size().width - px(SIDEBAR_WIDTH);
                if self.details.is_some() {
                    width -= px(DETAILS_WIDTH);
                }
                self.columns = ((width / px(CELL_MIN_WIDTH)).floor() as usize).max(1);
                let columns = self.columns;
                // Largura exata: as células não podem crescer com o título.
                let cell_width = ((width - px(2.)) / columns as f32).floor();
                let rows = self.books.len().div_ceil(columns);
                uniform_list(
                    "grid",
                    rows,
                    cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
                        let focused = this.books_focus.is_focused(window);
                        range
                            .map(|row| this.render_grid_row(row, columns, cell_width, focused, cx))
                            .collect()
                    }),
                )
                .track_scroll(self.scroll.clone())
                .flex_1()
                .into_any_element()
            }
            ViewMode::List => div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .child(list_columns(
                    div()
                        .h(px(28.))
                        .bg(rgb(theme::PANEL))
                        .border_b_1()
                        .border_color(rgb(theme::BORDER))
                        .text_xs()
                        .text_color(rgb(theme::TEXT_PLACEHOLDER)),
                    [
                        div().child("Nº"),
                        div(),
                        div().child("Título"),
                        div().child("Autor"),
                        div().child("Série"),
                        div().child("Formato"),
                        div().child("Lido"),
                    ],
                ))
                .child(
                    uniform_list(
                        "list",
                        self.books.len(),
                        cx.processor(|this, range: std::ops::Range<usize>, window, cx| {
                            let focused = this.books_focus.is_focused(window);
                            range
                                .map(|index| this.render_list_row(index, focused, cx))
                                .collect()
                        }),
                    )
                    .track_scroll(self.scroll.clone())
                    .flex_1(),
                )
                .into_any_element(),
        }
    }

    fn render_grid_row(
        &self,
        row: usize,
        columns: usize,
        cell_width: gpui::Pixels,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let start = row * columns;
        div()
            .flex()
            .h(px(GRID_ROW_HEIGHT))
            .border_b_1()
            .border_color(rgb(theme::BORDER_SUBTLE))
            .children((start..start + columns).map(|index| {
                match self.books.get(index) {
                    Some(book) => self.render_cell(book, cell_width, focused, cx).into_any_element(),
                    // Células vazias mantêm as pautas da gaveta até o fim da linha.
                    None => div()
                        .flex_none()
                        .w(cell_width)
                        .border_r_1()
                        .border_color(rgb(theme::BORDER_SUBTLE))
                        .into_any_element(),
                }
            }))
            .into_any_element()
    }

    fn render_cell(
        &self,
        book: &BookSummary,
        width: gpui::Pixels,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = book.id;
        let selected = self.selected == Some(id);
        let formats = book
            .formats
            .iter()
            .map(|f| f.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        div()
            .id(("book", id as u64))
            .flex_none()
            .w(width)
            .h_full()
            .border_r_1()
            .border_color(rgb(theme::BORDER_SUBTLE))
            .cursor_pointer()
            .when(selected, |this| this.bg(rgb(theme::ELEMENT_SELECTED)))
            .hover(|this| this.bg(rgb(theme::ELEMENT_HOVER)))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                window.focus(&this.books_focus);
                this.select(id, cx);
                if event.click_count() == 2 {
                    this.read(id, cx);
                }
            }))
            .child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .border_1()
                    .border_color(focus_ring(selected && focused))
                    .child(
                        div()
                            .flex()
                            .w_full()
                            .justify_between()
                            .child(mono(catalog_number(id)))
                            .child(mono(formats)),
                    )
                    .child(cover(
                        book.thumbnail.clone(),
                        &book.title,
                        book.formats.first().map_or("", |f| f.as_str()),
                        px(COVER_WIDTH),
                        px(COVER_HEIGHT),
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w_full()
                            .gap(px(2.))
                            .pt_1()
                            .child(
                                div()
                                    .h(px(38.))
                                    .font_family(theme::READING_FONT)
                                    .text_size(px(14.))
                                    .line_height(px(19.))
                                    .text_color(rgb(theme::TEXT))
                                    .line_clamp(2)
                                    .child(book.title.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(theme::TEXT_MUTED))
                                    .truncate()
                                    .child(authors_label(&book.authors)),
                            ),
                    )
                    .child(div().flex_1())
                    .when_some(book.progress, |this, progress| {
                        this.child(
                            div()
                                .flex()
                                .w_full()
                                .items_center()
                                .gap_2()
                                .child(div().flex_1().child(progress_bar(progress)))
                                .child(mono(format!("{:.0}%", progress * 100.))),
                        )
                    }),
            )
    }

    fn render_list_row(&self, index: usize, focused: bool, cx: &mut Context<Self>) -> AnyElement {
        let book = &self.books[index];
        let id = book.id;
        let selected = self.selected == Some(id);
        div()
            .id(("row", id as u64))
            .h(px(LIST_ROW_HEIGHT))
            .border_b_1()
            .border_color(rgb(theme::BORDER_SUBTLE))
            .cursor_pointer()
            .when(selected, |this| this.bg(rgb(theme::ELEMENT_SELECTED)))
            .hover(|this| this.bg(rgb(theme::ELEMENT_HOVER)))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                window.focus(&this.books_focus);
                this.select(id, cx);
                if event.click_count() == 2 {
                    this.read(id, cx);
                }
            }))
            .child(list_columns(
                div()
                    .size_full()
                    .border_1()
                    .border_color(focus_ring(selected && focused)),
                [
                    mono(catalog_number(id)),
                    cover(
                        book.thumbnail.clone(),
                        &book.title,
                        book.formats.first().map_or("", |f| f.as_str()),
                        px(22.),
                        px(33.),
                    ),
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(theme::TEXT))
                        .truncate()
                        .child(book.title.clone()),
                    div()
                        .text_xs()
                        .text_color(rgb(theme::TEXT_MUTED))
                        .truncate()
                        .child(authors_label(&book.authors)),
                    div()
                        .text_xs()
                        .text_color(rgb(theme::TEXT_MUTED))
                        .truncate()
                        .child(book.series.as_ref().map(series_label).unwrap_or_default()),
                    mono(
                        book.formats
                            .iter()
                            .map(|f| f.as_str())
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
                    mono(
                        book.progress
                            .map(|p| format!("{:.0}%", p * 100.))
                            .unwrap_or_else(|| "—".into()),
                    ),
                ],
            ))
            .into_any_element()
    }

    fn render_welcome(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                div()
                    .font_family(theme::READING_FONT)
                    .text_size(px(28.))
                    .text_color(rgb(theme::TEXT))
                    .child("A estante está vazia"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme::TEXT_MUTED))
                    .child("Arraste arquivos ou pastas com EPUBs e PDFs para cá, ou importe com o botão."),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .mt_2()
                    .child(primary_button("import-welcome", "Importar livros").on_click(
                        cx.listener(|this, _, window, cx| this.import_books(&ImportBooks, window, cx)),
                    ))
                    .child(kbd("Ctrl+O")),
            )
    }

    fn render_details(&self, details: &BookDetails, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let summary = &details.summary;
        let id = summary.id;
        let has_epub = summary.formats.contains(&Format::Epub);

        let body = match &self.editor {
            Some(editor) => self.render_editor(editor, cx).into_any_element(),
            None => self.render_metadata(details, cx).into_any_element(),
        };

        let actions = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(if has_epub {
                let label = if summary.progress.is_some() {
                    "Continuar leitura"
                } else {
                    "Ler"
                };
                primary_button("read", label)
                    .child(
                        div()
                            .font_family(theme::MONO_FONT)
                            .text_xs()
                            .opacity(0.7)
                            .child("Enter"),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.read(id, cx)))
                    .into_any_element()
            } else {
                disabled_button("Leitor de PDF em breve").into_any_element()
            })
            .when_some(summary.progress, |this, progress| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().flex_1().child(progress_bar(progress)))
                        .child(mono(format!("{:.0}% lido", progress * 100.))),
                )
            })
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        outline_button("edit", "Editar")
                            .flex_1()
                            .child(kbd("F2"))
                            .on_click(cx.listener(|this, _, window, cx| this.start_edit(window, cx))),
                    )
                    .child(if self.confirm_delete {
                        danger_button("confirm-delete", "Confirmar exclusão")
                            .flex_1()
                            .child(kbd("Del"))
                            .on_click(cx.listener(|this, _, _, cx| this.delete_selected(cx)))
                            .into_any_element()
                    } else {
                        danger_button("delete", "Excluir")
                            .flex_1()
                            .child(kbd("Del"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.confirm_delete = true;
                                cx.notify();
                            }))
                            .into_any_element()
                    }),
            )
            .when(self.confirm_delete, |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme::ERROR))
                        .child("O livro e seus arquivos saem da biblioteca. Esc cancela."),
                )
            });

        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(DETAILS_WIDTH))
            .h_full()
            .bg(rgb(theme::PANEL))
            .border_l_1()
            .border_color(rgb(theme::BORDER))
            .child(
                bar()
                    .child(section_label("Ficha"))
                    .child(mono(catalog_number(id)))
                    .child(div().flex_1())
                    .child(
                        button("close-details", "✕").on_click(cx.listener(|this, _, window, cx| {
                            this.editor = None;
                            this.confirm_delete = false;
                            this.dismiss(&Dismiss, window, cx)
                        })),
                    ),
            )
            .child(
                div()
                    .id("details")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .gap_4()
                    .p_4()
                    .overflow_y_scroll()
                    .child(div().flex().justify_center().pt_1().child(cover(
                        details.cover.clone(),
                        &summary.title,
                        summary.formats.first().map_or("", |f| f.as_str()),
                        px(156.),
                        px(234.),
                    )))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .font_family(theme::READING_FONT)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_size(px(20.))
                                    .line_height(px(26.))
                                    .text_color(rgb(theme::TEXT))
                                    .child(summary.title.clone()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(theme::TEXT_MUTED))
                                    .child(authors_label(&summary.authors)),
                            )
                            .when_some(summary.series.as_ref(), |this, series| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .italic()
                                        .text_color(rgb(theme::TEXT_MUTED))
                                        .child(series_label(series)),
                                )
                            }),
                    )
                    .when(self.editor.is_none(), |this| this.child(actions))
                    .child(body),
            )
    }

    fn render_metadata(&self, details: &BookDetails, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        // Ficha pautada: uma linha por campo, como nas fichas de catálogo.
        let row = |label: &'static str, value: Option<String>, monospace: bool| {
            value.filter(|v| !v.is_empty()).map(|value| {
                div()
                    .flex()
                    .gap_3()
                    .py(px(5.))
                    .border_t_1()
                    .border_color(rgb(theme::BORDER_SUBTLE))
                    .text_xs()
                    .child(
                        div()
                            .w(px(76.))
                            .flex_none()
                            .text_color(rgb(theme::TEXT_PLACEHOLDER))
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(rgb(theme::TEXT))
                            .when(monospace, |this| this.font_family(theme::MONO_FONT))
                            .child(value),
                    )
            })
        };
        let files = details
            .files
            .iter()
            .map(|f| format!("{} {}", f.format.as_str(), human_size(f.size)))
            .collect::<Vec<_>>()
            .join(", ");

        let member_of: Vec<i64> = details.collections.iter().map(|c| c.id).collect();
        let available: Vec<&Facet> = self
            .sidebar
            .collections
            .iter()
            .filter(|c| !member_of.contains(&c.id))
            .collect();

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .border_b_1()
                    .border_color(rgb(theme::BORDER_SUBTLE))
                    .children(row("Editora", details.publisher.clone(), false))
                    .children(row("Publicação", details.published.clone(), true))
                    .children(row("Idioma", details.language.clone(), true))
                    .children(row("ISBN", details.isbn.clone(), true))
                    .children(row("Arquivos", Some(files), true))
                    .children(row(
                        "Na estante",
                        Some(details.summary.added_at.chars().take(10).collect()),
                        true,
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(section_label("Coleções"))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_1()
                            .children(details.collections.iter().map(|collection| {
                                let collection_id = collection.id;
                                chip(collection.name.clone()).child(
                                    div()
                                        .id(("remove-from", collection_id as u64))
                                        .cursor_pointer()
                                        .hover(|this| this.text_color(rgb(theme::ERROR)))
                                        .child("✕")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.toggle_collection(collection_id, false, cx)
                                        })),
                                )
                            }))
                            .when(!available.is_empty(), |this| {
                                this.child(
                                    button("add-to-collection", "+ Adicionar")
                                        .text_xs()
                                        .py_0()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.collection_picker_open = !this.collection_picker_open;
                                            cx.notify();
                                        })),
                                )
                            })
                            .when(self.sidebar.collections.is_empty(), |this| {
                                this.child(empty_hint("Crie coleções com “+ Coleção”"))
                            }),
                    )
                    .when(self.collection_picker_open, |this| {
                        this.child(
                            div()
                                .flex()
                                .flex_col()
                                .border_1()
                                .border_color(rgb(theme::BORDER))
                                .bg(rgb(theme::INPUT))
                                .children(available.iter().map(|collection| {
                                    let collection_id = collection.id;
                                    button(("pick", collection_id as u64), collection.name.clone())
                                        .justify_start()
                                        .rounded_none()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.toggle_collection(collection_id, true, cx)
                                        }))
                                })),
                        )
                    }),
            )
            .when(!details.tags.is_empty(), |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(section_label("Tags"))
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_1()
                                .children(details.tags.iter().map(|t| chip(t.clone()))),
                        ),
                )
            })
            .when_some(details.description.clone(), |this, description| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(section_label("Sinopse"))
                        .children(description.split("\n\n").map(|paragraph| {
                            div()
                                .text_sm()
                                .line_height(relative(1.55))
                                .font_family(theme::READING_FONT)
                                .text_color(rgb(theme::TEXT_MUTED))
                                .child(paragraph.to_owned())
                        })),
                )
            })
    }

    fn render_editor(&self, editor: &Editor, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let field = |label: &'static str, input: &Entity<TextInput>| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_xs().text_color(rgb(theme::TEXT_MUTED)).child(label))
                .child(input.clone())
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .pt_3()
            .border_t_1()
            .border_color(rgb(theme::BORDER))
            .child(section_label("Editar metadados"))
            .child(field("Título", &editor.title))
            .child(field("Autores", &editor.authors))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(div().flex_1().child(field("Série", &editor.series)))
                    .child(div().w(px(64.)).child(field("Volume", &editor.series_index))),
            )
            .child(field("Tags", &editor.tags))
            .child(field("Editora", &editor.publisher))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(div().w(px(90.)).child(field("Idioma", &editor.language)))
                    .child(div().flex_1().child(field("Publicação", &editor.published))),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .pt_1()
                    .child(
                        primary_button("save", "Salvar")
                            .flex_1()
                            .on_click(cx.listener(|this, _, _, cx| this.save_edit(cx))),
                    )
                    .child(outline_button("cancel", "Cancelar").on_click(cx.listener(
                        |this, _, window, cx| {
                            this.editor = None;
                            window.focus(&this.books_focus);
                            cx.notify();
                        },
                    ))),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .text_xs()
                    .text_color(rgb(theme::TEXT_PLACEHOLDER))
                    .child(kbd("Enter"))
                    .child("salva,")
                    .child(kbd("Esc"))
                    .child("cancela,")
                    .child(kbd("Tab"))
                    .child("vai ao próximo campo"),
            )
    }

    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let sidebar = &self.sidebar;
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(px(STATUS_HEIGHT))
            .bg(rgb(theme::PANEL))
            .border_t_1()
            .border_color(rgb(theme::BORDER))
            .text_xs()
            .text_color(rgb(theme::TEXT_MUTED))
            .child(
                div()
                    .px_3()
                    .child(plural(sidebar.total as usize, "livro", "livros")),
            )
            .child(status_segment(format!("{} em leitura", sidebar.reading)))
            .when(self.books.len() as i64 != sidebar.total, |this| {
                this.child(status_segment(format!("{} neste filtro", self.books.len())))
            })
            .when_some(self.importing, |this, (done, total)| {
                this.child(status_segment(format!("Importando {done} de {total}…")))
            })
            .child(div().flex_1())
            .child(status_segment(
                mono(display_path(self.library.read(cx).root()))
                    .max_w(px(420.))
                    .truncate(),
            ))
            .child(status_segment(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(kbd("F1"))
                    .child("Atalhos"),
            ))
    }
}

impl Render for LibraryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let books = self.render_books(window, cx);
        let details = self.details.as_ref().map(|d| self.render_details(d, cx));

        div()
            .id("library")
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::import_books))
            .on_action(cx.listener(Self::focus_search))
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::show_grid))
            .on_action(cx.listener(Self::show_list))
            .on_action(cx.listener(Self::edit_book))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.import_paths(paths.paths().to_vec(), cx);
            }))
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_sidebar(window, cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(self.render_toolbar(cx))
                            .children(self.render_notice(cx))
                            .child(
                                div()
                                    .id("books")
                                    .track_focus(&self.books_focus)
                                    .key_context(nav::BOOKS)
                                    .on_action(cx.listener(Self::books_up))
                                    .on_action(cx.listener(Self::books_down))
                                    .on_action(cx.listener(Self::books_left))
                                    .on_action(cx.listener(Self::books_right))
                                    .on_action(cx.listener(Self::books_page_up))
                                    .on_action(cx.listener(Self::books_page_down))
                                    .on_action(cx.listener(Self::books_first))
                                    .on_action(cx.listener(Self::books_last))
                                    .on_action(cx.listener(Self::books_confirm))
                                    .on_action(cx.listener(Self::delete_book))
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_h_0()
                                    .child(books),
                            ),
                    )
                    .children(details),
            )
            .child(self.render_status_bar(cx))
    }
}

/// Moldura do cursor de teclado: transparente quando o item não está sob ele, para o
/// layout não pular.
fn focus_ring(visible: bool) -> gpui::Hsla {
    if visible {
        rgb(theme::ACCENT).into()
    } else {
        gpui::transparent_black()
    }
}

/// Colunas da lista (cabeçalho e linhas usam as mesmas larguras).
fn list_columns(row: gpui::Div, cells: [gpui::Div; 7]) -> gpui::Div {
    let [number, thumb, title, author, series, format, progress] = cells;
    row.flex()
        .items_center()
        .gap_4()
        .px_4()
        .child(number.w(px(56.)).flex_none())
        .child(thumb.w(px(22.)).flex_none())
        .child(title.flex_1().min_w_0())
        .child(author.w(px(180.)).flex_none())
        .child(series.w(px(170.)).flex_none())
        .child(format.w(px(70.)).flex_none())
        .child(progress.w(px(40.)).flex_none())
}

#[derive(Default)]
struct ImportReport {
    added: usize,
    duplicates: usize,
    errors: Vec<String>,
}

impl ImportReport {
    fn record(&mut self, path: &Path, result: r_ereader_library::Result<ImportOutcome>) {
        match result {
            Ok(ImportOutcome::Added(_)) => self.added += 1,
            Ok(ImportOutcome::Duplicate(_)) => self.duplicates += 1,
            Err(e) => {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                self.errors.push(format!("{name}: {e}"));
            }
        }
    }

    fn notice(&self) -> Notice {
        let mut parts = vec![plural(self.added, "livro adicionado", "livros adicionados")];
        if self.duplicates > 0 {
            parts.push(plural(
                self.duplicates,
                "já estava na biblioteca",
                "já estavam na biblioteca",
            ));
        }
        if !self.errors.is_empty() {
            parts.push(plural(self.errors.len(), "falhou", "falharam"));
        }
        let mut text = parts.join(", ");
        if let Some(first) = self.errors.first() {
            text.push_str(&format!(" — {first}"));
        }
        Notice {
            text: text.into(),
            kind: if self.errors.is_empty() {
                NoticeKind::Info
            } else {
                NoticeKind::Error
            },
        }
    }
}

fn plural(count: usize, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}

/// Expande pastas (recursivamente) e mantém só arquivos que a biblioteca cataloga.
fn importable_files(paths: &[PathBuf]) -> Vec<PathBuf> {
    fn walk(path: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if path.is_dir() {
            if depth == 0 {
                return;
            }
            let Ok(entries) = std::fs::read_dir(path) else {
                return;
            };
            let mut children: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
            children.sort();
            for child in children {
                walk(&child, depth - 1, out);
            }
        } else if Format::from_path(path).is_some() {
            out.push(path.to_path_buf());
        }
    }
    let mut files = Vec::new();
    for path in paths {
        walk(path, 8, &mut files);
    }
    files
}

/// Caminho com `~` no lugar da home, para caber na barra lateral.
fn display_path(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home.as_deref().and_then(|home| path.strip_prefix(home).ok()) {
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

fn authors_label(authors: &[String]) -> String {
    if authors.is_empty() {
        "Autor desconhecido".to_owned()
    } else {
        authors.join(", ")
    }
}

fn series_label(series: &r_ereader_library::SeriesRef) -> String {
    match series.index {
        Some(index) => format!("{}, volume {}", series.name, format_series_index(index)),
        None => series.name.clone(),
    }
}

fn format_series_index(index: f64) -> String {
    if index.fract() == 0. {
        format!("{index:.0}")
    } else {
        index.to_string()
    }
}

fn human_size(bytes: u64) -> String {
    match bytes {
        b if b >= 1024 * 1024 => format!("{:.1} MB", b as f64 / (1024. * 1024.)),
        b => format!("{} KB", (b / 1024).max(1)),
    }
}

fn empty_hint(text: &'static str) -> impl IntoElement {
    div()
        .px_3()
        .py_1()
        .text_xs()
        .italic()
        .text_color(rgb(theme::TEXT_PLACEHOLDER))
        .child(text)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use gpui::{TestAppContext, VisualTestContext};
    use r_ereader_library::testing::{write_epub, write_pdf};
    use tempfile::TempDir;

    use super::*;

    /// Biblioteca com dois EPUBs e um PDF, e a pasta de origem para novas importações.
    fn library_with_books(dir: &Path) -> (Library, PathBuf) {
        let sources = dir.join("origem");
        std::fs::create_dir_all(&sources).unwrap();
        let mut library = Library::open(dir.join("biblioteca")).unwrap();
        library
            .import(&write_epub(
                &sources,
                "a.epub",
                "Absalão, Absalão!",
                "William Faulkner",
                &["Romance"],
            ))
            .unwrap();
        library
            .import(&write_epub(
                &sources,
                "b.epub",
                "Homem algum é uma ilha",
                "Thomas Merton",
                &[],
            ))
            .unwrap();
        library.import(&write_pdf(&sources, "c.pdf")).unwrap();
        (library, sources)
    }

    fn open_view(library: Library, cx: &mut TestAppContext) -> (Entity<LibraryView>, &mut VisualTestContext) {
        cx.update(crate::bind_keys);
        let (view, cx) = cx.add_window_view(|window, cx| {
            let library = cx.new(|_| library);
            LibraryView::new(library, window, cx)
        });
        cx.update(|window, cx| window.focus(&view.focus_handle(cx)));
        (view, cx)
    }

    fn book_id(view: &Entity<LibraryView>, cx: &mut VisualTestContext, title_prefix: &str) -> BookId {
        view.read_with(cx, |view, _| {
            view.books
                .iter()
                .find(|b| b.title.starts_with(title_prefix))
                .map(|b| b.id)
                .unwrap()
        })
    }

    fn titles(view: &Entity<LibraryView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, _| view.books.iter().map(|b| b.title.clone()).collect())
    }

    #[gpui::test]
    fn selecting_shows_details_and_escape_clears(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let (view, cx) = open_view(library_with_books(dir.path()).0, cx);

        let id = book_id(&view, cx, "Homem");
        view.update(cx, |view, cx| view.select(id, cx));
        let title = view.read_with(cx, |view, _| {
            view.details.as_ref().map(|d| d.summary.title.clone())
        });
        assert_eq!(title.as_deref(), Some("Homem algum é uma ilha"));

        cx.simulate_keystrokes("escape");
        assert_eq!(view.read_with(cx, |view, _| view.selected), None);
    }

    #[gpui::test]
    fn typing_in_search_filters_and_escape_resets(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let (view, cx) = open_view(library_with_books(dir.path()).0, cx);
        assert_eq!(titles(&view, cx).len(), 3);

        cx.simulate_keystrokes("ctrl-f");
        cx.simulate_input("absalao");
        assert_eq!(titles(&view, cx), ["Absalão, Absalão!"]);

        cx.simulate_keystrokes("escape");
        assert_eq!(titles(&view, cx).len(), 3);
    }

    #[gpui::test]
    fn sidebar_filters_by_format(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let (view, cx) = open_view(library_with_books(dir.path()).0, cx);

        view.update(cx, |view, cx| view.set_filter(Filter::Format(Format::Pdf), cx));
        assert_eq!(titles(&view, cx), ["Manual de Liturgia"]);
        view.update(cx, |view, cx| view.set_filter(Filter::All, cx));
        assert_eq!(titles(&view, cx).len(), 3);
    }

    #[gpui::test]
    fn editing_metadata_through_the_form(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let (view, cx) = open_view(library_with_books(dir.path()).0, cx);
        let id = book_id(&view, cx, "Absalão");
        view.update(cx, |view, cx| view.select(id, cx));

        cx.update(|window, cx| view.update(cx, |view, cx| view.start_edit(window, cx)));
        // O título começa focado e preenchido: substitui tudo.
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input("Absalão (edição revista)");
        // Tags: foca o campo e acrescenta uma.
        cx.update(|window, cx| {
            let tags = view.read(cx).editor.as_ref().unwrap().tags.clone();
            window.focus(&tags.focus_handle(cx));
        });
        cx.simulate_keystrokes("end");
        cx.simulate_input(", Clássico");
        cx.simulate_keystrokes("enter");

        view.read_with(cx, |view, _| {
            assert!(view.editor.is_none(), "o formulário deveria fechar ao salvar");
            let details = view.details.as_ref().unwrap();
            assert_eq!(details.summary.title, "Absalão (edição revista)");
            assert_eq!(details.tags, ["Clássico", "Romance"]);
            assert_eq!(details.summary.authors, ["William Faulkner"]);
        });
    }

    #[gpui::test]
    fn invalid_volume_keeps_form_open_with_error(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let (view, cx) = open_view(library_with_books(dir.path()).0, cx);
        let id = book_id(&view, cx, "Absalão");
        view.update(cx, |view, cx| view.select(id, cx));
        cx.update(|window, cx| view.update(cx, |view, cx| view.start_edit(window, cx)));

        cx.update(|window, cx| {
            let index = view.read(cx).editor.as_ref().unwrap().series_index.clone();
            window.focus(&index.focus_handle(cx));
        });
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input("dois");
        cx.simulate_keystrokes("enter");

        view.read_with(cx, |view, _| {
            assert!(view.editor.is_some());
            assert!(matches!(
                view.notice,
                Some(Notice {
                    kind: NoticeKind::Error,
                    ..
                })
            ));
        });
        // Esc no campo cancela a edição sem salvar.
        cx.simulate_keystrokes("escape");
        view.read_with(cx, |view, _| {
            assert!(view.editor.is_none());
            assert_eq!(
                view.details
                    .as_ref()
                    .unwrap()
                    .summary
                    .series
                    .as_ref()
                    .unwrap()
                    .index,
                Some(2.0)
            );
        });
    }

    #[gpui::test]
    fn creating_collections_and_adding_books(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let (view, cx) = open_view(library_with_books(dir.path()).0, cx);

        cx.update(|window, cx| view.update(cx, |view, cx| view.start_new_collection(window, cx)));
        cx.simulate_input("Lendo agora");
        cx.simulate_keystrokes("enter");
        let collection = view.read_with(cx, |view, _| {
            assert!(view.new_collection.is_none());
            view.sidebar
                .collections
                .iter()
                .find(|c| c.name == "Lendo agora")
                .unwrap()
                .id
        });

        let id = book_id(&view, cx, "Homem");
        view.update(cx, |view, cx| {
            view.select(id, cx);
            view.toggle_collection(collection, true, cx);
        });
        view.read_with(cx, |view, _| {
            assert_eq!(view.details.as_ref().unwrap().collections[0].name, "Lendo agora");
            assert_eq!(view.sidebar.collections[0].count, 1);
        });

        view.update(cx, |view, cx| view.set_filter(Filter::Collection(collection), cx));
        assert_eq!(titles(&view, cx), ["Homem algum é uma ilha"]);

        // Apagar a coleção filtrada volta para "Todos os livros".
        view.update(cx, |view, cx| view.delete_collection(collection, cx));
        view.read_with(cx, |view, _| {
            assert_eq!(view.query.filter, Filter::All);
            assert!(view.sidebar.collections.is_empty());
        });
    }

    #[gpui::test]
    fn duplicate_collection_name_shows_error(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let (view, cx) = open_view(library_with_books(dir.path()).0, cx);
        for _ in 0..2 {
            cx.update(|window, cx| view.update(cx, |view, cx| view.start_new_collection(window, cx)));
            cx.simulate_input("Teologia");
            cx.simulate_keystrokes("enter");
        }
        view.read_with(cx, |view, _| {
            assert_eq!(view.sidebar.collections.len(), 1);
            assert!(matches!(
                view.notice,
                Some(Notice {
                    kind: NoticeKind::Error,
                    ..
                })
            ));
            // O campo continua aberto para corrigir o nome.
            assert!(view.new_collection.is_some());
        });
    }

    #[gpui::test]
    fn deleting_requires_confirmation(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let (view, cx) = open_view(library_with_books(dir.path()).0, cx);
        let id = book_id(&view, cx, "Homem");
        view.update(cx, |view, cx| {
            view.select(id, cx);
            view.confirm_delete = true;
        });
        // Esc desarma a confirmação mas mantém o livro selecionado.
        cx.simulate_keystrokes("escape");
        view.read_with(cx, |view, _| {
            assert!(!view.confirm_delete);
            assert_eq!(view.selected, Some(id));
        });

        view.update(cx, |view, cx| view.delete_selected(cx));
        assert_eq!(titles(&view, cx).len(), 2);
        assert_eq!(view.read_with(cx, |view, _| view.selected), None);
    }

    #[gpui::test]
    fn importing_a_folder_in_background(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let (library, sources) = library_with_books(dir.path());
        let (view, cx) = open_view(library, cx);
        let extra = sources.join("novos/sub");
        std::fs::create_dir_all(&extra).unwrap();
        write_epub(&extra, "d.epub", "Livro Novo", "Autora Nova", &[]);
        std::fs::write(extra.join("notas.txt"), "ignorar").unwrap();

        // A pasta inteira: os três já importados viram duplicados.
        view.update(cx, |view, cx| view.import_paths(vec![sources.clone()], cx));
        cx.run_until_parked();

        view.read_with(cx, |view, _| {
            assert!(view.importing.is_none());
            assert_eq!(view.sidebar.total, 4);
            let notice = view.notice.as_ref().unwrap();
            assert_eq!(
                notice.text.as_ref(),
                "1 livro adicionado, 3 já estavam na biblioteca"
            );
        });
    }

    #[gpui::test]
    fn obsidian_section_exports_and_disables(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let (library, _) = library_with_books(dir.path());
        let book = library.books(&BookQuery::default()).unwrap()[0].id;
        library
            .add_highlight(
                book,
                &r_ereader_library::NewHighlight {
                    chapter: 0,
                    chapter_title: None,
                    start: 0,
                    end: 3,
                    quote: "Era".into(),
                    color: r_ereader_library::HighlightColor::Yellow,
                },
            )
            .unwrap();
        let folder = dir.path().join("vault/Leituras");
        let (view, cx) = open_view(library, cx);

        view.update(cx, |view, cx| {
            view.set_obsidian_folder(Some(folder.clone()), cx);
            view.export_all_to_obsidian(cx);
        });
        view.read_with(cx, |view, _| {
            assert_eq!(view.sidebar.obsidian.as_deref(), Some(folder.as_path()));
            let notice = view.notice.as_ref().unwrap();
            assert!(
                notice.text.starts_with("1 nota atualizada no Obsidian"),
                "{}",
                notice.text
            );
        });
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);

        view.update(cx, |view, cx| view.set_obsidian_folder(None, cx));
        assert_eq!(view.read_with(cx, |view, _| view.sidebar.obsidian.clone()), None);
    }

    fn selected_title(view: &Entity<LibraryView>, cx: &mut VisualTestContext) -> Option<String> {
        view.read_with(cx, |view, _| {
            view.details.as_ref().map(|d| d.summary.title.clone())
        })
    }

    #[gpui::test]
    fn arrows_pick_books_and_enter_opens(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let (view, cx) = open_view(library_with_books(dir.path()).0, cx);
        let order = titles(&view, cx);
        let opened = std::rc::Rc::new(std::cell::Cell::new(None));
        let flag = opened.clone();
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &LibraryEvent, _| match event {
                LibraryEvent::Read(id) => flag.set(Some(*id)),
            })
            .detach();
        });

        cx.simulate_keystrokes("right");
        assert_eq!(selected_title(&view, cx).as_ref(), Some(&order[0]));
        cx.simulate_keystrokes("right right right");
        assert_eq!(selected_title(&view, cx).as_ref(), order.last(), "para no último");
        cx.simulate_keystrokes("home");
        assert_eq!(selected_title(&view, cx).as_ref(), Some(&order[0]));

        // Na lista, ↓ anda um livro por vez.
        cx.simulate_keystrokes("ctrl-2 down");
        assert_eq!(selected_title(&view, cx).as_ref(), Some(&order[1]));

        cx.simulate_keystrokes("enter");
        assert_eq!(opened.get(), view.read_with(cx, |v, _| v.selected));
    }

    #[gpui::test]
    fn f2_edits_and_delete_needs_two_presses(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let (view, cx) = open_view(library_with_books(dir.path()).0, cx);
        cx.simulate_keystrokes("right f2");
        assert!(view.read_with(cx, |v, _| v.editor.is_some()));
        // Esc no formulário cancela e devolve o foco aos livros.
        cx.simulate_keystrokes("escape");
        assert!(view.read_with(cx, |v, _| v.editor.is_none()));

        cx.simulate_keystrokes("delete");
        assert!(view.read_with(cx, |v, _| v.confirm_delete));
        assert_eq!(titles(&view, cx).len(), 3);
        cx.simulate_keystrokes("delete");
        assert_eq!(titles(&view, cx).len(), 2);
    }

    #[gpui::test]
    fn search_enter_jumps_to_results(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let (view, cx) = open_view(library_with_books(dir.path()).0, cx);
        cx.simulate_keystrokes("ctrl-f");
        cx.simulate_input("homem");
        cx.simulate_keystrokes("enter");
        assert_eq!(
            selected_title(&view, cx).as_deref(),
            Some("Homem algum é uma ilha")
        );
        cx.update(|window, cx| {
            assert!(view.read(cx).books_focus.is_focused(window));
        });
    }

    #[gpui::test]
    fn sidebar_is_navigable_with_the_keyboard(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let (view, cx) = open_view(library_with_books(dir.path()).0, cx);
        cx.update(|window, cx| window.focus(&view.read(cx).sidebar_focus.clone()));

        // A última linha é o título "Formatos" (fechado): → abre, ↓ desce ao primeiro.
        cx.simulate_keystrokes("end right down enter");
        let filter = view.read_with(cx, |v, _| v.query.filter);
        assert!(matches!(filter, Filter::Format(_)), "{filter:?}");
        assert!(titles(&view, cx).len() < 3);

        // ← volta ao título da seção e fecha.
        cx.simulate_keystrokes("left left");
        assert!(view.read_with(cx, |v, _| !v.expanded.contains(&Section::Formats)));
        cx.simulate_keystrokes("home enter");
        assert_eq!(titles(&view, cx).len(), 3);
    }

    #[gpui::test]
    fn typing_spaces_in_a_new_collection(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let (view, cx) = open_view(library_with_books(dir.path()).0, cx);
        cx.update(|window, cx| view.update(cx, |view, cx| view.start_new_collection(window, cx)));
        cx.simulate_keystrokes("a space b enter");
        view.read_with(cx, |v, _| assert_eq!(v.sidebar.collections[0].name, "a b"));
    }

    #[test]
    fn collects_importable_files_recursively() {
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("a/b");
        std::fs::create_dir_all(&nested).unwrap();
        for name in ["x.epub", "y.PDF", "z.txt"] {
            std::fs::write(nested.join(name), "").unwrap();
        }
        let files = importable_files(&[dir.path().to_path_buf()]);
        let names: Vec<_> = files
            .iter()
            .map(|f| f.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, ["x.epub", "y.PDF"]);
    }
}

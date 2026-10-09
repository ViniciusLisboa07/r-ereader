use std::ops::Range;
use std::sync::Arc;

use gpui::{
    AnyElement, ClipboardItem, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    HighlightStyle, ListAlignment, ListOffset, ListState, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ObjectFit, Pixels, Point, ScrollHandle, ScrollStrategy, SharedString, StyledText,
    Subscription, TextLayout, UnderlineStyle, UniformListScrollHandle, Window, actions, anchored, deferred,
    div, img, list, point, prelude::*, px, relative, rgb, uniform_list,
};
use r_ereader_library::{
    BookId, Highlight, HighlightColor, Library, NewHighlight, Progress, anchor, obsidian,
};

use crate::book::{ChapterBlock, OpenBook};
use crate::chapter_text::{flatten_layers, word_at};
use crate::components::{STATUS_HEIGHT, bar, button, kbd, mono, section_label, status_segment};
use crate::input::{InputEvent, TextInput};
use crate::nav;
use crate::preview::Block;
use crate::search::{self, BookIndex, Results};
use crate::theme;

actions!(
    reader,
    [
        NextChapter,
        PreviousChapter,
        CloseReader,
        CopySelection,
        FindInBook,
        NextMatch,
        PreviousMatch,
        ToggleSidebar,
        ShowContents,
        ShowAnnotations,
        ScrollLineUp,
        ScrollLineDown,
        ScrollPageUp,
        ScrollPageDown,
        ScrollToStart,
        ScrollToEnd,
        IncreaseFontSize,
        DecreaseFontSize,
        ResetFontSize,
        FocusText,
    ]
);

pub const CONTEXT: &str = "Reader";
/// Contexto só da área do texto: ali Espaço e setas rolam a página. Os campos (busca,
/// nota) ficam fora dele para continuarem recebendo essas teclas.
pub const TEXT_CONTEXT: &str = "ReaderText";

const FONT_SIZE_SETTING: &str = "leitor.tamanho_fonte";
const DEFAULT_FONT_SIZE: f32 = 18.;
const MIN_FONT_SIZE: f32 = 13.;
const MAX_FONT_SIZE: f32 = 30.;
const LINE_SCROLL: f32 = 64.;

const SIDEBAR_WIDTH: f32 = 300.;
const READING_WIDTH: f32 = 680.;
const SEARCH_ROW_HEIGHT: f32 = 78.;

pub enum ReaderEvent {
    Close,
}

/// Livro aberto vindo da biblioteca: progresso e destaques são salvos nela.
pub struct LibraryLink {
    pub library: Entity<Library>,
    pub book_id: BookId,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SidebarTab {
    Contents,
    Annotations,
    Search,
}

/// Índice de busca do livro, montado em segundo plano na primeira busca.
enum SearchIndex {
    Missing,
    Building,
    Ready(Arc<BookIndex>),
    Failed(SharedString),
}

struct SearchPanel {
    input: Entity<TextInput>,
    scroll: UniformListScrollHandle,
    _subscription: Subscription,
}

/// Seleção em offsets do texto do capítulo; `head` acompanha o mouse.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Selection {
    anchor: usize,
    head: usize,
}

impl Selection {
    fn range(self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum MenuTarget {
    Selection,
    Highlight(i64),
}

struct Menu {
    position: Point<Pixels>,
    target: MenuTarget,
}

struct NoteEditor {
    highlight: i64,
    input: Entity<TextInput>,
    _subscription: Subscription,
}

/// Destaque do capítulo atual com a posição já resolvida no texto.
#[derive(Clone, Debug)]
struct VisibleHighlight {
    id: i64,
    range: Range<usize>,
    color: HighlightColor,
    has_note: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Layer {
    Highlight(HighlightColor, bool),
    /// Ocorrência da busca; `true` para a que está selecionada.
    Match(bool),
    Selection,
}

impl Layer {
    fn style(self) -> HighlightStyle {
        match self {
            Layer::Highlight(color, has_note) => HighlightStyle {
                background_color: Some(rgb(theme::highlight(color)).into()),
                underline: has_note.then(|| UnderlineStyle {
                    color: Some(rgb(theme::ACCENT).into()),
                    thickness: px(1.),
                    wavy: false,
                }),
                ..Default::default()
            },
            Layer::Match(false) => HighlightStyle {
                background_color: Some(rgb(theme::MATCH).into()),
                ..Default::default()
            },
            Layer::Match(true) => HighlightStyle {
                background_color: Some(rgb(theme::ACCENT).into()),
                color: Some(rgb(theme::ON_ACCENT).into()),
                ..Default::default()
            },
            Layer::Selection => HighlightStyle {
                background_color: Some(rgb(theme::SELECTION).into()),
                ..Default::default()
            },
        }
    }
}

pub struct ReaderView {
    focus_handle: FocusHandle,
    /// Lista virtualizada: só os blocos visíveis são medidos e desenhados.
    blocks: ListState,
    book: OpenBook,
    link: Option<LibraryLink>,
    sidebar_tab: SidebarTab,
    /// Todos os destaques do livro, na ordem de leitura.
    highlights: Vec<Highlight>,
    visible: Vec<VisibleHighlight>,
    selection: Option<Selection>,
    selecting: bool,
    menu: Option<Menu>,
    note_editor: Option<NoteEditor>,
    status: Option<SharedString>,
    search_panel: Option<SearchPanel>,
    search_index: SearchIndex,
    results: Results,
    active_hit: Option<usize>,
    /// Foco da lista da lateral (sumário, anotações ou resultados).
    panel_focus: FocusHandle,
    /// Item sob o cursor de teclado na lista da lateral.
    panel_cursor: usize,
    panel_scroll: ScrollHandle,
    sidebar_visible: bool,
    font_size: f32,
}

impl EventEmitter<ReaderEvent> for ReaderView {}

impl Focusable for ReaderView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ReaderView {
    pub fn new(mut book: OpenBook, link: Option<LibraryLink>, cx: &mut Context<Self>) -> Self {
        if let Some(link) = &link
            && let Ok(Some(progress)) = link.library.read(cx).progress(link.book_id)
        {
            book.go_to(progress.chapter);
        }
        let font_size = link
            .as_ref()
            .and_then(|link| link.library.read(cx).setting(FONT_SIZE_SETTING).ok().flatten())
            .and_then(|value| value.parse::<f32>().ok())
            .map_or(DEFAULT_FONT_SIZE, |size| size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE));
        let mut view = ReaderView {
            focus_handle: cx.focus_handle().tab_stop(true),
            blocks: ListState::new(book.blocks.len(), ListAlignment::Top, px(800.)),
            book,
            link,
            sidebar_tab: SidebarTab::Contents,
            highlights: Vec::new(),
            visible: Vec::new(),
            selection: None,
            selecting: false,
            menu: None,
            note_editor: None,
            status: None,
            search_panel: None,
            search_index: SearchIndex::Missing,
            results: Results::default(),
            active_hit: None,
            panel_focus: cx.focus_handle().tab_stop(true),
            panel_cursor: 0,
            panel_scroll: ScrollHandle::new(),
            sidebar_visible: true,
            font_size,
        };
        view.load_highlights(cx);
        view
    }

    // ── Navegação ──────────────────────────────────────────────────────────

    fn go_to(&mut self, chapter: usize, cx: &mut Context<Self>) {
        if chapter != self.book.current && self.book.go_to(chapter) {
            self.blocks.reset(self.book.blocks.len());
            self.clear_transient();
            self.resolve_chapter_highlights(cx);
            self.save_progress(cx);
        }
        cx.notify();
    }

    fn save_progress(&self, cx: &mut Context<Self>) {
        let Some(link) = &self.link else { return };
        let total = self.book.chapter_count().max(1);
        let progress = Progress {
            chapter: self.book.current,
            fraction: (self.book.current + 1) as f32 / total as f32,
        };
        // Falhar ao salvar o progresso não deve interromper a leitura.
        link.library.read(cx).save_progress(link.book_id, progress).ok();
    }

    fn next_chapter(&mut self, _: &NextChapter, _: &mut Window, cx: &mut Context<Self>) {
        self.go_to(self.book.current + 1, cx);
    }

    fn previous_chapter(&mut self, _: &PreviousChapter, _: &mut Window, cx: &mut Context<Self>) {
        self.go_to(self.book.current.saturating_sub(1), cx);
    }

    /// Esc fecha, nesta ordem: editor de nota, menu/seleção, e por fim o leitor.
    fn close(&mut self, _: &CloseReader, window: &mut Window, cx: &mut Context<Self>) {
        if self.note_editor.take().is_some() {
            window.focus(&self.focus_handle);
        } else if self.menu.is_some() || self.selection.is_some() {
            self.clear_transient();
        } else if !self.search_query(cx).is_empty() {
            self.clear_search(cx);
        } else {
            self.save_progress(cx);
            // Atualiza o progresso na nota (se o livro já tiver uma).
            self.sync_obsidian(cx);
            cx.emit(ReaderEvent::Close);
        }
        cx.notify();
    }

    fn go_to_highlight(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(chapter) = self.highlights.iter().find(|h| h.id == id).map(|h| h.chapter) else {
            return;
        };
        self.go_to(chapter, cx);
        if let Some(visible) = self.visible.iter().find(|h| h.id == id) {
            let block = self.book.text.block_at(visible.range.start);
            self.blocks.scroll_to(ListOffset {
                item_ix: block,
                offset_in_item: px(0.),
            });
        } else {
            self.status = Some("O trecho deste destaque não foi encontrado no capítulo.".into());
        }
        cx.notify();
    }

    // ── Teclado: texto ─────────────────────────────────────────────────────

    fn scroll_line_up(&mut self, _: &ScrollLineUp, _: &mut Window, cx: &mut Context<Self>) {
        self.blocks.scroll_by(px(-LINE_SCROLL));
        cx.notify();
    }

    fn scroll_line_down(&mut self, _: &ScrollLineDown, _: &mut Window, cx: &mut Context<Self>) {
        self.blocks.scroll_by(px(LINE_SCROLL));
        cx.notify();
    }

    fn page_height(&self) -> Pixels {
        // Deixa uma faixa da tela anterior à vista, para não perder o fio.
        (self.blocks.viewport_bounds().size.height - px(80.)).max(px(LINE_SCROLL))
    }

    fn scroll_page_up(&mut self, _: &ScrollPageUp, _: &mut Window, cx: &mut Context<Self>) {
        self.blocks.scroll_by(-self.page_height());
        cx.notify();
    }

    fn scroll_page_down(&mut self, _: &ScrollPageDown, _: &mut Window, cx: &mut Context<Self>) {
        self.blocks.scroll_by(self.page_height());
        cx.notify();
    }

    fn scroll_to_start(&mut self, _: &ScrollToStart, _: &mut Window, cx: &mut Context<Self>) {
        self.blocks.scroll_to(ListOffset::default());
        cx.notify();
    }

    fn scroll_to_end(&mut self, _: &ScrollToEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.blocks
            .scroll_to_reveal_item(self.book.blocks.len().saturating_sub(1));
        cx.notify();
    }

    fn set_font_size(&mut self, size: f32, cx: &mut Context<Self>) {
        let size = size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        if size == self.font_size {
            return;
        }
        self.font_size = size;
        // As alturas medidas mudaram: remede tudo, mantendo o bloco do topo no lugar.
        let top = self.blocks.logical_scroll_top();
        self.blocks.reset(self.book.blocks.len());
        self.blocks.scroll_to(ListOffset {
            item_ix: top.item_ix,
            offset_in_item: px(0.),
        });
        if let Some(link) = &self.link {
            link.library
                .read(cx)
                .set_setting(FONT_SIZE_SETTING, &size.to_string())
                .ok();
        }
        cx.notify();
    }

    fn increase_font_size(&mut self, _: &IncreaseFontSize, _: &mut Window, cx: &mut Context<Self>) {
        self.set_font_size(self.font_size + 1., cx);
    }

    fn decrease_font_size(&mut self, _: &DecreaseFontSize, _: &mut Window, cx: &mut Context<Self>) {
        self.set_font_size(self.font_size - 1., cx);
    }

    fn reset_font_size(&mut self, _: &ResetFontSize, _: &mut Window, cx: &mut Context<Self>) {
        self.set_font_size(DEFAULT_FONT_SIZE, cx);
    }

    // ── Teclado: lateral ───────────────────────────────────────────────────

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_visible = !self.sidebar_visible;
        if !self.sidebar_visible {
            window.focus(&self.focus_handle);
        }
        cx.notify();
    }

    fn open_tab(&mut self, tab: SidebarTab, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_visible = true;
        if tab == SidebarTab::Search {
            self.find_in_book(&FindInBook, window, cx);
            return;
        }
        if self.sidebar_tab != tab {
            self.sidebar_tab = tab;
            self.panel_cursor = match tab {
                SidebarTab::Contents => self.book.active_toc_row().unwrap_or(0),
                _ => 0,
            };
            self.panel_scroll.scroll_to_item(self.panel_cursor);
        }
        window.focus(&self.panel_focus);
        cx.notify();
    }

    fn show_contents(&mut self, _: &ShowContents, window: &mut Window, cx: &mut Context<Self>) {
        self.open_tab(SidebarTab::Contents, window, cx);
    }

    fn show_annotations(&mut self, _: &ShowAnnotations, window: &mut Window, cx: &mut Context<Self>) {
        self.open_tab(SidebarTab::Annotations, window, cx);
    }

    fn focus_text(&mut self, _: &FocusText, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn panel_len(&self) -> usize {
        match self.sidebar_tab {
            SidebarTab::Contents => self.book.toc.len(),
            SidebarTab::Annotations => self.highlights.len(),
            SidebarTab::Search => self.results.hits.len(),
        }
    }

    fn panel_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(cursor) = nav::step(Some(self.panel_cursor), self.panel_len(), delta) else {
            return;
        };
        self.panel_cursor = cursor;
        match self.sidebar_tab {
            SidebarTab::Search => {
                if let Some(panel) = &self.search_panel {
                    panel.scroll.scroll_to_item(cursor, ScrollStrategy::Top);
                }
            }
            _ => self.panel_scroll.scroll_to_item(cursor),
        }
        cx.notify();
    }

    fn panel_up(&mut self, _: &nav::MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.panel_move(-1, cx);
    }

    fn panel_down(&mut self, _: &nav::MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.panel_move(1, cx);
    }

    fn panel_page_up(&mut self, _: &nav::PageUp, _: &mut Window, cx: &mut Context<Self>) {
        self.panel_move(-10, cx);
    }

    fn panel_page_down(&mut self, _: &nav::PageDown, _: &mut Window, cx: &mut Context<Self>) {
        self.panel_move(10, cx);
    }

    fn panel_first(&mut self, _: &nav::MoveFirst, _: &mut Window, cx: &mut Context<Self>) {
        self.panel_move(isize::MIN / 2, cx);
    }

    fn panel_last(&mut self, _: &nav::MoveLast, _: &mut Window, cx: &mut Context<Self>) {
        self.panel_move(isize::MAX / 2, cx);
    }

    fn panel_left(&mut self, _: &nav::MoveLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.previous_chapter(&PreviousChapter, window, cx);
    }

    fn panel_right(&mut self, _: &nav::MoveRight, window: &mut Window, cx: &mut Context<Self>) {
        self.next_chapter(&NextChapter, window, cx);
    }

    /// Enter abre o item sob o cursor; o foco fica na lista para seguir navegando.
    fn panel_confirm(&mut self, _: &nav::Confirm, _: &mut Window, cx: &mut Context<Self>) {
        let cursor = self.panel_cursor;
        match self.sidebar_tab {
            SidebarTab::Contents => {
                if let Some(chapter) = self.book.toc.get(cursor).and_then(|row| row.chapter) {
                    self.go_to(chapter, cx);
                }
            }
            SidebarTab::Annotations => {
                if let Some(id) = self.highlights.get(cursor).map(|h| h.id) {
                    self.go_to_highlight(id, cx);
                }
            }
            SidebarTab::Search => self.go_to_hit(cursor, cx),
        }
    }

    // ── Busca ──────────────────────────────────────────────────────────────

    /// Abre a aba de busca e foca o campo; uma seleção curta vira a busca.
    fn find_in_book(&mut self, _: &FindInBook, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_visible = true;
        if self.sidebar_tab != SidebarTab::Search {
            self.panel_cursor = self.active_hit.unwrap_or(0);
        }
        self.sidebar_tab = SidebarTab::Search;
        let selected = self
            .selection
            .map(|s| self.book.text.text()[s.range()].trim().to_owned())
            .filter(|text| !text.is_empty() && text.len() <= 120 && !text.contains('\n'));
        let input = self.search_input(window, cx);
        if let Some(text) = selected {
            input.update(cx, |input, cx| input.set_text(text, cx));
            self.clear_transient();
            self.run_search(cx);
        }
        window.focus(&input.focus_handle(cx));
        self.ensure_index(cx);
        cx.notify();
    }

    fn search_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<TextInput> {
        if let Some(panel) = &self.search_panel {
            return panel.input.clone();
        }
        let input = cx.new(|cx| TextInput::new("Buscar no livro…", cx));
        let subscription = cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
            match event {
                InputEvent::Changed => this.run_search(cx),
                InputEvent::Submit => this.step_hit(1, cx),
                InputEvent::Cancel => window.focus(&this.focus_handle),
            }
            cx.notify();
        });
        self.search_panel = Some(SearchPanel {
            input: input.clone(),
            scroll: UniformListScrollHandle::new(),
            _subscription: subscription,
        });
        input
    }

    fn search_query(&self, cx: &gpui::App) -> String {
        self.search_panel
            .as_ref()
            .map(|panel| panel.input.read(cx).text().to_owned())
            .unwrap_or_default()
    }

    fn ensure_index(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.search_index, SearchIndex::Missing) {
            return;
        }
        self.search_index = SearchIndex::Building;
        let path = self.book.path.clone();
        cx.spawn(async move |this, cx| {
            let built = cx.background_spawn(async move { BookIndex::build(&path) }).await;
            this.update(cx, |this, cx| {
                this.search_index = match built {
                    Ok(index) => SearchIndex::Ready(Arc::new(index)),
                    Err(e) => SearchIndex::Failed(e.to_string().into()),
                };
                this.run_search(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn run_search(&mut self, cx: &mut Context<Self>) {
        let query = self.search_query(cx);
        self.results = match &self.search_index {
            SearchIndex::Ready(index) => index.search(&query),
            _ => Results::default(),
        };
        self.active_hit = None;
        self.panel_cursor = 0;
        if let Some(panel) = &self.search_panel {
            panel.scroll.scroll_to_item(0, ScrollStrategy::Top);
        }
    }

    fn clear_search(&mut self, cx: &mut Context<Self>) {
        if let Some(panel) = &self.search_panel {
            panel.input.update(cx, |input, cx| input.set_text("", cx));
        }
        self.results = Results::default();
        self.active_hit = None;
    }

    /// Próximo (`1`) ou anterior (`-1`) resultado. Sem resultado ativo, começa no
    /// primeiro a partir do capítulo atual.
    fn step_hit(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self.results.hits.len();
        if count == 0 {
            return;
        }
        let next = match self.active_hit {
            Some(active) => (active as isize + delta).rem_euclid(count as isize) as usize,
            None => {
                let current = self.book.current;
                let after = self.results.hits.iter().position(|hit| hit.chapter >= current);
                match (delta > 0, after) {
                    (true, Some(index)) => index,
                    (true, None) => 0,
                    (false, Some(index)) => index.checked_sub(1).unwrap_or(count - 1),
                    (false, None) => count - 1,
                }
            }
        };
        self.go_to_hit(next, cx);
    }

    fn next_match(&mut self, _: &NextMatch, _: &mut Window, cx: &mut Context<Self>) {
        self.step_hit(1, cx);
    }

    fn previous_match(&mut self, _: &PreviousMatch, _: &mut Window, cx: &mut Context<Self>) {
        self.step_hit(-1, cx);
    }

    fn go_to_hit(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(hit) = self.results.hits.get(index) else {
            return;
        };
        let (chapter, start) = (hit.chapter, hit.ranges.first().map_or(0, |r| r.start));
        self.active_hit = Some(index);
        if self.sidebar_tab == SidebarTab::Search {
            self.panel_cursor = index;
        }
        self.go_to(chapter, cx);
        self.blocks.scroll_to(ListOffset {
            item_ix: self.book.text.block_at(start),
            offset_in_item: px(0.),
        });
        if let Some(panel) = &self.search_panel {
            panel.scroll.scroll_to_item(index, ScrollStrategy::Center);
        }
        cx.notify();
    }

    /// Rótulo de um capítulo: o sumário, o primeiro título dele ou o número.
    fn chapter_label(&self, chapter: usize) -> SharedString {
        self.book
            .toc_title(chapter)
            .or_else(|| match &self.search_index {
                SearchIndex::Ready(index) => index.chapters.get(chapter)?.heading.clone().map(Into::into),
                _ => None,
            })
            .unwrap_or_else(|| format!("Capítulo {}", chapter + 1).into())
    }

    // ── Destaques ──────────────────────────────────────────────────────────

    fn load_highlights(&mut self, cx: &mut Context<Self>) {
        let Some(link) = &self.link else { return };
        match link.library.read(cx).highlights(link.book_id) {
            Ok(highlights) => self.highlights = highlights,
            Err(e) => self.status = Some(format!("Não foi possível ler os destaques: {e}").into()),
        }
        self.resolve_chapter_highlights(cx);
    }

    /// Posiciona os destaques do capítulo atual no texto; se um trecho mudou de lugar
    /// (texto extraído de outro jeito), grava a posição nova.
    fn resolve_chapter_highlights(&mut self, cx: &mut Context<Self>) {
        let text = self.book.text.text();
        let mut moved = Vec::new();
        self.visible = self
            .highlights
            .iter()
            .filter(|h| h.chapter == self.book.current)
            .filter_map(|h| {
                let range = anchor::resolve(text, h.start, h.end, &h.quote)?;
                if range != (h.start..h.end) {
                    moved.push((h.id, range.clone()));
                }
                Some(VisibleHighlight {
                    id: h.id,
                    range,
                    color: h.color,
                    has_note: h.note.is_some(),
                })
            })
            .collect();

        if let Some(link) = &self.link {
            let library = link.library.read(cx);
            for (id, range) in moved {
                library.move_highlight(id, range.start, range.end).ok();
                if let Some(h) = self.highlights.iter_mut().find(|h| h.id == id) {
                    (h.start, h.end) = (range.start, range.end);
                }
            }
        }
    }

    /// Executa uma alteração nos destaques e recarrega; erros vão para a barra de status.
    fn with_library<T>(
        &mut self,
        cx: &mut Context<Self>,
        action: impl FnOnce(&Library, BookId) -> r_ereader_library::Result<T>,
    ) -> Option<T> {
        let link = self.link.as_ref()?;
        let result = action(link.library.read(cx), link.book_id);
        match result {
            Ok(value) => {
                self.status = None;
                self.load_highlights(cx);
                self.sync_obsidian(cx);
                Some(value)
            }
            Err(e) => {
                self.status = Some(e.to_string().into());
                None
            }
        }
    }

    /// Reescreve a nota do livro no Obsidian, se a exportação estiver ativa.
    fn sync_obsidian(&mut self, cx: &mut Context<Self>) {
        let Some(link) = &self.link else { return };
        if let Err(e) = obsidian::sync_book(link.library.read(cx), link.book_id) {
            self.status = Some(format!("Obsidian: {e}").into());
        }
    }

    fn create_highlight(&mut self, color: HighlightColor, cx: &mut Context<Self>) {
        let Some(range) = self.selection.map(Selection::range) else {
            return;
        };
        let Some(range) = trim_range(self.book.text.text(), range) else {
            return;
        };
        let new = NewHighlight {
            chapter: self.book.current,
            chapter_title: self.book.chapter_title().map(Into::into),
            start: range.start,
            end: range.end,
            quote: self.book.text.text()[range].to_owned(),
            color,
        };
        self.with_library(cx, |library, book| library.add_highlight(book, &new));
        self.clear_transient();
        cx.notify();
    }

    fn set_color(&mut self, id: i64, color: HighlightColor, cx: &mut Context<Self>) {
        self.with_library(cx, |library, _| library.set_highlight_color(id, color));
        self.menu = None;
        cx.notify();
    }

    fn delete_highlight(&mut self, id: i64, cx: &mut Context<Self>) {
        self.with_library(cx, |library, _| library.delete_highlight(id));
        self.clear_transient();
        cx.notify();
    }

    fn start_note(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let current = self
            .highlights
            .iter()
            .find(|h| h.id == id)
            .and_then(|h| h.note.clone());
        let input = cx.new(|cx| {
            let mut input = TextInput::new("Escreva uma nota…", cx);
            input.set_text(current.unwrap_or_default(), cx);
            input
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            move |this, input, event: &InputEvent, window, cx| {
                match event {
                    InputEvent::Submit => {
                        let note = input.read(cx).text().to_owned();
                        this.with_library(cx, |library, _| library.set_highlight_note(id, &note));
                        this.note_editor = None;
                        this.menu = None;
                        window.focus(&this.focus_handle);
                    }
                    InputEvent::Cancel => {
                        this.note_editor = None;
                        window.focus(&this.focus_handle);
                    }
                    InputEvent::Changed => {}
                }
                cx.notify();
            },
        );
        window.focus(&input.focus_handle(cx));
        self.note_editor = Some(NoteEditor {
            highlight: id,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    fn copy(&mut self, _: &CopySelection, _: &mut Window, cx: &mut Context<Self>) {
        let text = match (self.selection, &self.menu) {
            (Some(selection), _) => Some(self.book.text.text()[selection.range()].to_owned()),
            (
                None,
                Some(Menu {
                    target: MenuTarget::Highlight(id),
                    ..
                }),
            ) => self
                .highlights
                .iter()
                .find(|h| h.id == *id)
                .map(|h| h.quote.clone()),
            _ => None,
        };
        if let Some(text) = text.filter(|t| !t.trim().is_empty()) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.status = Some("Trecho copiado.".into());
        }
        self.clear_transient();
        cx.notify();
    }

    fn clear_transient(&mut self) {
        self.selection = None;
        self.selecting = false;
        self.menu = None;
        self.note_editor = None;
    }

    // ── Mouse ──────────────────────────────────────────────────────────────

    fn mouse_down(
        &mut self,
        block: usize,
        index: usize,
        click_count: usize,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.menu = None;
        self.note_editor = None;
        let offset = self.book.text.offset(block, index);
        if click_count >= 2 {
            let word = word_at(self.book.blocks[block].text(), index);
            let range = self.book.text.offset(block, word.start)..self.book.text.offset(block, word.end);
            self.selecting = false;
            self.selection = (!range.is_empty()).then_some(Selection {
                anchor: range.start,
                head: range.end,
            });
            if self.selection.is_some() {
                self.menu = Some(Menu {
                    position,
                    target: MenuTarget::Selection,
                });
            }
        } else {
            self.selection = Some(Selection {
                anchor: offset,
                head: offset,
            });
            self.selecting = true;
        }
        cx.notify();
    }

    fn mouse_drag(&mut self, block: usize, index: usize, cx: &mut Context<Self>) {
        if !self.selecting {
            return;
        }
        let offset = self.book.text.offset(block, index);
        if let Some(selection) = &mut self.selection
            && selection.head != offset
        {
            selection.head = offset;
            cx.notify();
        }
    }

    /// `block` é `None` quando o botão é solto fora do texto: a seleção fica onde estava.
    fn mouse_up(&mut self, block: Option<(usize, usize)>, position: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.selecting {
            return;
        }
        self.selecting = false;
        if let (Some((block, index)), Some(selection)) = (block, &mut self.selection) {
            selection.head = self.book.text.offset(block, index);
        }

        match self.selection {
            Some(selection) if !selection.range().is_empty() => {
                self.menu = Some(Menu {
                    position,
                    target: MenuTarget::Selection,
                });
            }
            Some(selection) => {
                // Clique simples: abre o destaque sob o cursor, se houver.
                self.selection = None;
                let clicked = self
                    .visible
                    .iter()
                    .rev()
                    .find(|h| h.range.contains(&selection.head));
                self.menu = clicked.map(|h| Menu {
                    position,
                    target: MenuTarget::Highlight(h.id),
                });
            }
            None => {}
        }
        cx.notify();
    }

    // ── Renderização ───────────────────────────────────────────────────────

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let book = &self.book;
        let crumb =
            |text: SharedString, color: u32| div().flex_none().truncate().text_color(rgb(color)).child(text);
        let separator = || div().flex_none().text_color(rgb(theme::BORDER)).child("/");
        let chapter = self.chapter_label(book.current);

        bar()
            .gap_3()
            .bg(rgb(theme::PANEL))
            .child(
                button("back", "← Biblioteca").on_click(cx.listener(|this, _, window, cx| {
                    this.clear_transient();
                    this.clear_search(cx);
                    this.close(&CloseReader, window, cx)
                })),
            )
            .child(div().w(px(1.)).h(px(18.)).bg(rgb(theme::BORDER)))
            // Trilha: autor / livro / capítulo.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_sm()
                    .when(!book.authors.is_empty(), |this| {
                        this.child(crumb(book.authors.clone(), theme::TEXT_PLACEHOLDER).max_w(px(220.)))
                            .child(separator())
                    })
                    .child(crumb(book.title.clone(), theme::TEXT_MUTED).max_w(px(320.)))
                    .child(separator())
                    .child(crumb(chapter, theme::TEXT).flex_shrink()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(button("previous", "‹").on_click(
                        cx.listener(|this, _, window, cx| {
                            this.previous_chapter(&PreviousChapter, window, cx)
                        }),
                    ))
                    .child(mono(format!(
                        "{:02} / {:02}",
                        book.current + 1,
                        book.chapter_count()
                    )))
                    .child(button("next", "›").on_click(
                        cx.listener(|this, _, window, cx| this.next_chapter(&NextChapter, window, cx)),
                    )),
            )
    }

    fn render_sidebar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let book = &self.book;
        let tab = |id: &'static str,
                   label: &'static str,
                   count: Option<usize>,
                   target: SidebarTab,
                   cx: &mut Context<Self>| {
            let active = self.sidebar_tab == target;
            div()
                .id(id)
                .flex()
                .items_center()
                .gap_1()
                .px_3()
                .h_full()
                .text_xs()
                .cursor_pointer()
                .border_r_1()
                .border_color(rgb(theme::BORDER))
                .text_color(rgb(if active { theme::TEXT } else { theme::TEXT_MUTED }))
                // A aba ativa não tem pauta embaixo: se funde com o painel, como no Zed.
                .when(active, |this| {
                    this.bg(rgb(theme::PANEL)).font_weight(FontWeight::MEDIUM)
                })
                .when(!active, |this| this.border_b_1())
                .when(!active, |this| {
                    this.hover(|this| this.text_color(rgb(theme::TEXT)))
                })
                .on_click(cx.listener(move |this, _, window, cx| this.open_tab(target, window, cx)))
                .child(label)
                .children(count.map(|n| mono(n.to_string())))
        };

        let focused = self.panel_focus.is_focused(window);
        let body = match self.sidebar_tab {
            SidebarTab::Contents => self.render_contents(focused, cx).into_any_element(),
            SidebarTab::Annotations => self.render_annotations(focused, cx).into_any_element(),
            SidebarTab::Search => self.render_search(focused, cx).into_any_element(),
        };

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
                div()
                    .flex()
                    .gap_3()
                    .p_3()
                    .border_b_1()
                    .border_color(rgb(theme::BORDER))
                    .when_some(book.cover.clone(), |this, cover| {
                        this.child(
                            img(cover)
                                .flex_none()
                                .w(px(54.))
                                .h(px(81.))
                                .border_1()
                                .border_color(rgb(theme::BORDER))
                                .object_fit(ObjectFit::Cover),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .min_w_0()
                            .child(
                                div()
                                    .font_family(theme::READING_FONT)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .line_height(px(21.))
                                    .text_color(rgb(theme::TEXT))
                                    .line_clamp(3)
                                    .child(book.title.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(theme::TEXT_MUTED))
                                    .truncate()
                                    .child(book.authors.clone()),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .h(px(32.))
                    .flex_none()
                    .bg(rgb(theme::SURFACE))
                    .child(tab("tab-contents", "Sumário", None, SidebarTab::Contents, cx))
                    .child(tab(
                        "tab-annotations",
                        "Anotações",
                        Some(self.highlights.len()),
                        SidebarTab::Annotations,
                        cx,
                    ))
                    .child(tab("tab-search", "Busca", None, SidebarTab::Search, cx))
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .border_b_1()
                            .border_color(rgb(theme::BORDER)),
                    ),
            )
            .child(body)
    }

    /// Lista navegável da lateral: foco próprio e as ações de `nav`.
    fn panel_list(&self, id: &'static str, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .track_focus(&self.panel_focus)
            .key_context(nav::READER_PANEL)
            .on_action(cx.listener(Self::panel_up))
            .on_action(cx.listener(Self::panel_down))
            .on_action(cx.listener(Self::panel_left))
            .on_action(cx.listener(Self::panel_right))
            .on_action(cx.listener(Self::panel_page_up))
            .on_action(cx.listener(Self::panel_page_down))
            .on_action(cx.listener(Self::panel_first))
            .on_action(cx.listener(Self::panel_last))
            .on_action(cx.listener(Self::panel_confirm))
            .on_action(cx.listener(Self::focus_text))
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
    }

    fn render_contents(&self, focused: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let book = &self.book;
        let active = book.active_toc_row();
        let rows = book.toc.iter().enumerate().map(|(index, row)| {
            let chapter = row.chapter;
            let is_active = Some(index) == active;
            let is_cursor = focused && index == self.panel_cursor;
            div()
                .id(("toc", index))
                .relative()
                .flex()
                .flex_none()
                .items_center()
                .h(px(28.))
                .pl(px(14. + row.depth as f32 * 14.))
                .pr_3()
                .border_1()
                .border_color(focus_ring(is_cursor))
                .text_sm()
                .text_color(rgb(if is_active { theme::TEXT } else { theme::TEXT_MUTED }))
                .when(is_active, |this| {
                    this.bg(rgb(theme::ELEMENT_SELECTED))
                        .font_weight(FontWeight::MEDIUM)
                        .child(active_marker())
                })
                .when_some(chapter, |this, chapter| {
                    this.cursor_pointer()
                        .hover(|this| this.bg(rgb(theme::ELEMENT_HOVER)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.panel_cursor = index;
                            window.focus(&this.panel_focus);
                            this.go_to(chapter, cx)
                        }))
                })
                .when(chapter.is_none(), |this| {
                    this.text_color(rgb(theme::TEXT_PLACEHOLDER))
                })
                .child(div().truncate().child(row.title.clone()))
        });
        let rows: Vec<_> = rows.collect();
        self.panel_list("toc-list", cx)
            .py_1()
            .overflow_y_scroll()
            .track_scroll(&self.panel_scroll)
            .children(rows)
    }

    fn render_annotations(&self, focused: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let hint = match &self.link {
            None => Some("Abra o livro pela biblioteca para criar destaques."),
            Some(_) if self.highlights.is_empty() => {
                Some("Selecione um trecho com o mouse (ou dê duplo clique numa palavra) para destacar.")
            }
            Some(_) => None,
        };
        let items = self.highlights.iter().enumerate().map(|(index, highlight)| {
            let id = highlight.id;
            let found = highlight.chapter != self.book.current || self.visible.iter().any(|v| v.id == id);
            let is_cursor = focused && index == self.panel_cursor;
            div()
                .id(("annotation", id as u64))
                .flex()
                .flex_none()
                .gap_3()
                .px_3()
                .py_2()
                .border_b_1()
                .border_color(rgb(theme::BORDER_SUBTLE))
                .cursor_pointer()
                .hover(|this| this.bg(rgb(theme::ELEMENT_HOVER)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.panel_cursor = index;
                    window.focus(&this.panel_focus);
                    this.go_to_highlight(id, cx)
                }))
                .child(
                    div()
                        .flex_none()
                        .w(px(3.))
                        .bg(rgb(theme::highlight(highlight.color))),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .flex_1()
                        .min_w_0()
                        .p(px(2.))
                        .border_1()
                        .border_color(focus_ring(is_cursor))
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(theme::TEXT_PLACEHOLDER))
                                .truncate()
                                .child(
                                    highlight
                                        .chapter_title
                                        .clone()
                                        .map(SharedString::from)
                                        .unwrap_or_else(|| self.chapter_label(highlight.chapter)),
                                ),
                        )
                        .child(
                            div()
                                .text_sm()
                                .font_family(theme::READING_FONT)
                                .text_color(rgb(theme::TEXT))
                                .line_clamp(4)
                                .child(highlight.quote.replace('\n', " ")),
                        )
                        .when_some(highlight.note.clone(), |this, note| {
                            this.child(
                                div()
                                    .text_xs()
                                    .italic()
                                    .text_color(rgb(theme::TEXT_MUTED))
                                    .child(format!("✎ {note}")),
                            )
                        })
                        .when(!found, |this| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(theme::ERROR))
                                    .child("Trecho não encontrado no capítulo"),
                            )
                        }),
                )
        });
        let items: Vec<_> = items.collect();
        self.panel_list("annotations", cx)
            .overflow_y_scroll()
            .track_scroll(&self.panel_scroll)
            .when_some(hint, |this, hint| {
                this.child(
                    div()
                        .px_3()
                        .py_3()
                        .text_xs()
                        .text_color(rgb(theme::TEXT_PLACEHOLDER))
                        .child(hint),
                )
            })
            .children(items)
    }

    fn render_search(&self, focused: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let Some(panel) = &self.search_panel else {
            return div().into_any_element();
        };
        let query = panel.input.read(cx).text().to_owned();
        let hits = &self.results.hits;
        let message: SharedString = match &self.search_index {
            SearchIndex::Missing | SearchIndex::Building => "Indexando o livro…".into(),
            SearchIndex::Failed(error) => format!("Não foi possível indexar o livro: {error}").into(),
            SearchIndex::Ready(_) if query.trim().is_empty() => {
                "Sem diferenciar acentos nem maiúsculas.".into()
            }
            SearchIndex::Ready(_) if search::normalize_query(&query).is_empty() => {
                "Digite ao menos 2 letras.".into()
            }
            SearchIndex::Ready(_) if hits.is_empty() => "Nada encontrado.".into(),
            SearchIndex::Ready(_) => {
                let mut chapters: Vec<usize> = hits.iter().map(|hit| hit.chapter).collect();
                chapters.sort_unstable();
                chapters.dedup();
                let total = if self.results.truncated {
                    format!("mais de {}", search::MAX_HITS)
                } else {
                    hits.len().to_string()
                };
                let position = self
                    .active_hit
                    .map_or(String::new(), |i| format!("{} de ", i + 1));
                let plural = |n: usize, one: &str, many: &str| if n == 1 { one } else { many }.to_owned();
                format!(
                    "{position}{total} {} em {} {}",
                    plural(hits.len(), "resultado", "resultados"),
                    chapters.len(),
                    plural(chapters.len(), "capítulo", "capítulos"),
                )
                .into()
            }
        };

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            // O campo fica fora da lista: lá Espaço e setas são atalhos.
            .child(div().px_3().pt_3().pb_2().child(panel.input.clone()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_3()
                    .pb_2()
                    .border_b_1()
                    .border_color(rgb(theme::BORDER))
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(rgb(theme::TEXT_MUTED))
                            .child(message),
                    )
                    .when(!hits.is_empty(), |this| {
                        this.child(
                            button("previous-match", "‹")
                                .on_click(cx.listener(|this, _, _, cx| this.step_hit(-1, cx))),
                        )
                        .child(kbd("Enter"))
                        .child(
                            button("next-match", "›")
                                .on_click(cx.listener(|this, _, _, cx| this.step_hit(1, cx))),
                        )
                    }),
            )
            .child(
                self.panel_list("search-panel", cx).child(
                    uniform_list(
                        "search-results",
                        hits.len(),
                        cx.processor(move |this, range: Range<usize>, _, cx| {
                            range.map(|index| this.render_hit(index, focused, cx)).collect()
                        }),
                    )
                    .track_scroll(panel.scroll.clone())
                    .flex_1(),
                ),
            )
            .into_any_element()
    }

    fn render_hit(&self, index: usize, focused: bool, cx: &mut Context<Self>) -> AnyElement {
        let hit = &self.results.hits[index];
        let active = Some(index) == self.active_hit;
        let is_cursor = focused && index == self.panel_cursor;
        let emphasis = HighlightStyle {
            background_color: Some(rgb(theme::MATCH).into()),
            font_weight: Some(FontWeight::SEMIBOLD),
            ..Default::default()
        };
        let snippet = StyledText::new(SharedString::from(hit.snippet.clone()))
            .with_highlights(hit.snippet_ranges.iter().map(|range| (range.clone(), emphasis)));

        div()
            .id(("hit", index))
            .relative()
            .flex()
            .flex_col()
            .gap(px(2.))
            .h(px(SEARCH_ROW_HEIGHT))
            .px_3()
            .py(px(7.))
            .border_b_1()
            .border_color(rgb(theme::BORDER_SUBTLE))
            .cursor_pointer()
            .when(active, |this| {
                this.bg(rgb(theme::ELEMENT_SELECTED)).child(active_marker())
            })
            .hover(|this| this.bg(rgb(theme::ELEMENT_HOVER)))
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&this.panel_focus);
                this.go_to_hit(index, cx)
            }))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .text_xs()
                    .text_color(rgb(theme::TEXT_PLACEHOLDER))
                    .child(div().flex_1().truncate().child(self.chapter_label(hit.chapter)))
                    .when(!hit.exact, |this| {
                        this.child(div().flex_none().child("palavras separadas"))
                    }),
            )
            .child(
                div()
                    .text_sm()
                    .font_family(theme::READING_FONT)
                    .text_color(rgb(theme::TEXT))
                    .line_clamp(2)
                    .child(snippet),
            )
            .when(is_cursor, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .border_1()
                        .border_color(rgb(theme::ACCENT)),
                )
            })
            .into_any_element()
    }

    fn render_item(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let block = &self.book.blocks[index];
        let mut layers: Vec<(Range<usize>, Layer)> = self
            .visible
            .iter()
            .filter_map(|h| {
                let local = self.book.text.local_range(index, &h.range)?;
                Some((local, Layer::Highlight(h.color, h.has_note)))
            })
            .collect();
        let matches = self
            .results
            .hits
            .iter()
            .enumerate()
            .filter(|(_, hit)| hit.chapter == self.book.current)
            .flat_map(|(i, hit)| hit.ranges.iter().map(move |range| (i, range)))
            .filter_map(|(i, range)| {
                let local = self.book.text.local_range(index, range)?;
                Some((local, Layer::Match(Some(i) == self.active_hit)))
            });
        layers.extend(matches);
        if let Some(local) = self
            .selection
            .and_then(|s| self.book.text.local_range(index, &s.range()))
        {
            layers.push((local, Layer::Selection));
        }
        let styles = flatten_layers(&layers)
            .into_iter()
            .map(|(range, layer)| (range, layer.style()));
        let (content, layout) = render_block(block, self.font_size / DEFAULT_FONT_SIZE, styles);

        let last = self.book.blocks.len().saturating_sub(1);
        let mut column = div()
            .flex_1()
            .max_w(px(READING_WIDTH))
            .font_family(theme::READING_FONT)
            .text_size(px(self.font_size))
            .line_height(relative(1.65))
            .text_color(rgb(theme::TEXT))
            .child(content);

        // O mouse vira posição no texto pelo layout do próprio bloco.
        if let Some(layout) = layout {
            let (down, drag, up) = (layout.clone(), layout.clone(), layout);
            column = column
                .cursor_text()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        let i = index_at(&down, event.position);
                        this.mouse_down(index, i, event.click_count, event.position, cx);
                    }),
                )
                .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                    if event.pressed_button == Some(MouseButton::Left) {
                        this.mouse_drag(index, index_at(&drag, event.position), cx);
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseUpEvent, _, cx| {
                        this.mouse_up(Some((index, index_at(&up, event.position))), event.position, cx);
                    }),
                );
        }

        let item = div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .px_8()
            .pb(px(if index == last { 96. } else { 16. }));
        // Cabeçalho do capítulo: posição no livro em mono, sobre uma pauta.
        let item = if index == 0 {
            item.pt_8().child(
                div()
                    .flex()
                    .justify_between()
                    .gap_4()
                    .w_full()
                    .max_w(px(READING_WIDTH))
                    .pb_2()
                    .mb_6()
                    .border_b_1()
                    .border_color(rgb(theme::BORDER))
                    .child(mono(format!(
                        "{:02} / {:02}",
                        self.book.current + 1,
                        self.book.chapter_count()
                    )))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme::TEXT_PLACEHOLDER))
                            .truncate()
                            .child(self.chapter_label(self.book.current)),
                    ),
            )
        } else {
            item
        };
        item.child(div().w_full().flex().justify_center().child(column))
            .into_any_element()
    }

    fn render_content(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let area = div()
            .id("reader-text")
            .track_focus(&self.focus_handle)
            .key_context(TEXT_CONTEXT)
            .on_action(cx.listener(Self::scroll_line_up))
            .on_action(cx.listener(Self::scroll_line_down))
            .on_action(cx.listener(Self::scroll_page_up))
            .on_action(cx.listener(Self::scroll_page_down))
            .on_action(cx.listener(Self::scroll_to_start))
            .on_action(cx.listener(Self::scroll_to_end))
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .bg(rgb(theme::BACKGROUND))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, _| window.focus(&this.focus_handle)),
            );
        if let Some(error) = &self.book.chapter_error {
            return area
                .p_12()
                .text_color(rgb(theme::ERROR))
                .child(error.clone())
                .into_any_element();
        }
        area
            // Soltar o botão fora do texto (margens, outra área) também encerra a seleção.
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _, cx| this.mouse_up(None, event.position, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _, cx| this.mouse_up(None, event.position, cx)),
            )
            .child(
                list(
                    self.blocks.clone(),
                    cx.processor(|this, index: usize, _, cx| this.render_item(index, cx)),
                )
                .flex_1()
                .h_full(),
            )
            .into_any_element()
    }

    fn render_menu(&self, menu: &Menu, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let current = match menu.target {
            MenuTarget::Highlight(id) => self.highlights.iter().find(|h| h.id == id),
            MenuTarget::Selection => None,
        };
        let target = menu.target;

        let panel = div()
            .occlude()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .rounded_sm()
            .border_1()
            .border_color(rgb(theme::BORDER))
            .bg(rgb(theme::INPUT))
            .shadow_lg()
            .font_family(theme::UI_FONT)
            .text_color(rgb(theme::TEXT));

        if let Some(editor) = self
            .note_editor
            .as_ref()
            .filter(|e| Some(e.highlight) == current.map(|h| h.id))
        {
            return panel
                .w(px(340.))
                .child(section_label("NOTA"))
                .child(editor.input.clone())
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme::TEXT_PLACEHOLDER))
                        .child("Enter salva · Esc cancela · deixe vazio para apagar"),
                )
                .into_any_element();
        }

        let swatches = HighlightColor::ALL.into_iter().map(|color| {
            let selected = current.is_some_and(|h| h.color == color);
            div()
                .id(theme::highlight_name(color))
                .size(px(22.))
                .rounded_full()
                .cursor_pointer()
                .bg(rgb(theme::highlight(color)))
                .border_2()
                .border_color(if selected {
                    rgb(theme::ACCENT)
                } else {
                    rgb(theme::INPUT)
                })
                .hover(|this| this.border_color(rgb(theme::TEXT_MUTED)))
                .on_click(cx.listener(move |this, _, _, cx| match target {
                    MenuTarget::Selection => this.create_highlight(color, cx),
                    MenuTarget::Highlight(id) => this.set_color(id, color, cx),
                }))
        });
        let has_note = current.is_some_and(|h| h.note.is_some());

        let row = div()
            .flex()
            .items_center()
            .gap_1()
            .when(self.link.is_some(), |this| {
                this.children(swatches)
                    .child(div().w(px(1.)).h(px(18.)).mx_1().bg(rgb(theme::BORDER)))
            })
            .when_some(current.map(|h| h.id), |this, id| {
                this.child(
                    button("note", if has_note { "Editar nota" } else { "Nota" })
                        .on_click(cx.listener(move |this, _, window, cx| this.start_note(id, window, cx))),
                )
            })
            .child(
                button("copy", "Copiar")
                    .on_click(cx.listener(|this, _, window, cx| this.copy(&CopySelection, window, cx))),
            )
            .when_some(current.map(|h| h.id), |this, id| {
                this.child(
                    button("remove", "Remover")
                        .text_color(rgb(theme::ERROR))
                        .on_click(cx.listener(move |this, _, _, cx| this.delete_highlight(id, cx))),
                )
            });

        panel
            .child(row)
            .when_some(current.and_then(|h| h.note.clone()), |this, note| {
                this.child(
                    div()
                        .max_w(px(320.))
                        .px_1()
                        .text_sm()
                        .italic()
                        .text_color(rgb(theme::TEXT_MUTED))
                        .child(note),
                )
            })
            .when(self.link.is_none(), |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme::TEXT_PLACEHOLDER))
                        .child("Abra o livro pela biblioteca para destacar."),
                )
            })
            .into_any_element()
    }

    fn render_status_bar(&self) -> impl IntoElement + use<> {
        let book = &self.book;
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
                div().flex_1().min_w_0().px_3().child(match &self.status {
                    Some(status) => div()
                        .truncate()
                        .text_color(rgb(theme::ACCENT))
                        .child(status.clone()),
                    None => mono(SharedString::from(book.chapter_path().to_owned())).truncate(),
                }),
            )
            .child(status_segment(book.version.clone()))
            .child(status_segment(format!(
                "Capítulo {} de {}",
                book.current + 1,
                book.chapter_count()
            )))
            .children(book.language.clone().map(status_segment))
            .child(status_segment(mono(format!("{:.0} px", self.font_size))))
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

impl Render for ReaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(&format!("{} — r-ereader", self.book.title));
        let menu = self.menu.as_ref().map(|menu| {
            // Logo abaixo do ponto onde o botão foi solto.
            let position = menu.position + point(px(-12.), px(14.));
            deferred(
                anchored()
                    .position(position)
                    .snap_to_window_with_margin(px(8.))
                    .child(self.render_menu(menu, cx)),
            )
            .with_priority(1)
        });

        div()
            .id("reader")
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::next_chapter))
            .on_action(cx.listener(Self::previous_chapter))
            .on_action(cx.listener(Self::close))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::find_in_book))
            .on_action(cx.listener(Self::next_match))
            .on_action(cx.listener(Self::previous_match))
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::show_contents))
            .on_action(cx.listener(Self::show_annotations))
            .on_action(cx.listener(Self::increase_font_size))
            .on_action(cx.listener(Self::decrease_font_size))
            .on_action(cx.listener(Self::reset_font_size))
            .flex()
            .flex_col()
            .size_full()
            .child(self.render_toolbar(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .when(self.sidebar_visible, |this| {
                        this.child(self.render_sidebar(window, cx))
                    })
                    .child(self.render_content(cx)),
            )
            .child(self.render_status_bar())
            .children(menu)
    }
}

/// Moldura do cursor de teclado; transparente fora dele para o layout não pular.
fn focus_ring(visible: bool) -> gpui::Hsla {
    if visible {
        rgb(theme::ACCENT).into()
    } else {
        gpui::transparent_black()
    }
}

/// Filete azul à esquerda do item ativo.
fn active_marker() -> gpui::Div {
    div()
        .absolute()
        .left_0()
        .top_0()
        .bottom_0()
        .w(px(2.))
        .bg(rgb(theme::ACCENT))
}

/// Índice (em bytes) do caractere sob a posição; fora do texto, o mais próximo.
fn index_at(layout: &TextLayout, position: Point<Pixels>) -> usize {
    let index = layout
        .index_for_position(position)
        .unwrap_or_else(|closest| closest);
    let text = layout.text();
    let mut index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// Remove espaços nas pontas: arrastar até a margem costuma pegar a quebra de linha.
fn trim_range(text: &str, range: Range<usize>) -> Option<Range<usize>> {
    let slice = &text[range.clone()];
    let start = range.start + (slice.len() - slice.trim_start().len());
    let end = range.end - (slice.len() - slice.trim_end().len());
    (start < end).then_some(start..end)
}

/// Desenha o bloco e devolve o layout do texto (para mapear o mouse), se houver texto.
fn render_block(
    block: &ChapterBlock,
    scale: f32,
    styles: impl IntoIterator<Item = (Range<usize>, HighlightStyle)>,
) -> (AnyElement, Option<TextLayout>) {
    let styled = |text: &str| {
        let styled = StyledText::new(SharedString::from(text.to_owned())).with_highlights(styles);
        let layout = styled.layout().clone();
        (styled, layout)
    };

    match block {
        ChapterBlock::Image(image) => (
            div()
                .flex()
                .justify_center()
                .child(
                    img(image.clone())
                        .max_w_full()
                        .max_h(px(720.))
                        .object_fit(ObjectFit::Contain),
                )
                .into_any_element(),
            None,
        ),
        ChapterBlock::Text(Block::Image(_)) => (div().into_any_element(), None),
        ChapterBlock::Text(Block::Heading { level, text }) => {
            let (text, layout) = styled(text);
            let element = div()
                .mt(px(if *level <= 2 { 24. } else { 12. }))
                .text_size(px(scale
                    * match level {
                        1 => 30.,
                        2 => 26.,
                        3 => 22.,
                        _ => 19.,
                    }))
                .line_height(relative(1.3))
                .font_weight(FontWeight::SEMIBOLD)
                .child(text);
            (element.into_any_element(), Some(layout))
        }
        ChapterBlock::Text(Block::Paragraph(text)) => {
            let (text, layout) = styled(text);
            (div().w_full().child(text).into_any_element(), Some(layout))
        }
        ChapterBlock::Text(Block::Quote(text)) => {
            let (text, layout) = styled(text);
            let element = div()
                .pl_4()
                .border_l_2()
                .border_color(rgb(theme::BORDER))
                .italic()
                .text_color(rgb(theme::TEXT_MUTED))
                .child(text);
            (element.into_any_element(), Some(layout))
        }
        ChapterBlock::Text(Block::ListItem(text)) => {
            let (text, layout) = styled(text);
            let element = div()
                .flex()
                .gap_3()
                .child(div().text_color(rgb(theme::ACCENT)).child("•"))
                .child(div().flex_1().child(text));
            (element.into_any_element(), Some(layout))
        }
        ChapterBlock::Text(Block::Preformatted(text)) => {
            let (text, layout) = styled(text);
            let element = div()
                .p_3()
                .rounded_md()
                .bg(rgb(theme::PANEL))
                .font_family(theme::MONO_FONT)
                .text_size(px(14. * scale))
                .child(text);
            (element.into_any_element(), Some(layout))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui::{TestAppContext, VisualTestContext};
    use r_ereader_library::Format;
    use r_ereader_library::testing::write_epub;
    use tempfile::TempDir;

    use super::*;

    struct Setup {
        _dir: TempDir,
        library: Library,
        book_id: BookId,
    }

    fn setup() -> Setup {
        let dir = TempDir::new().unwrap();
        let sources = dir.path().join("origem");
        std::fs::create_dir_all(&sources).unwrap();
        let mut library = Library::open(dir.path().join("biblioteca")).unwrap();
        // Capítulo 0: blocos "Um" (título) e "Era uma vez" (parágrafo).
        let book_id = library
            .import(&write_epub(&sources, "a.epub", "Livro", "Autor", &[]))
            .unwrap()
            .id();
        Setup {
            _dir: dir,
            library,
            book_id,
        }
    }

    fn open_reader(
        setup: Setup,
        linked: bool,
        cx: &mut TestAppContext,
    ) -> (Entity<ReaderView>, &mut VisualTestContext) {
        let path = setup
            .library
            .file_path(setup.book_id, Format::Epub)
            .unwrap()
            .unwrap();
        let book = OpenBook::open(&path).unwrap();
        let book_id = setup.book_id;
        let library = setup.library;
        cx.update(crate::bind_keys);
        // O TempDir precisa viver até o fim do teste.
        std::mem::forget(setup._dir);
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let link = linked.then(|| LibraryLink {
                library: cx.new(|_| library),
                book_id,
            });
            ReaderView::new(book, link, cx)
        });
        cx.update(|window, cx| window.focus(&view.focus_handle(cx)));
        (view, cx)
    }

    fn select(
        view: &Entity<ReaderView>,
        cx: &mut VisualTestContext,
        from: (usize, usize),
        to: (usize, usize),
    ) {
        view.update(cx, |view, cx| {
            view.mouse_down(from.0, from.1, 1, point(px(10.), px(10.)), cx);
            view.mouse_drag(to.0, to.1, cx);
            view.mouse_up(Some(to), point(px(50.), px(50.)), cx);
        });
    }

    fn highlights(view: &Entity<ReaderView>, cx: &mut VisualTestContext) -> Vec<Highlight> {
        view.read_with(cx, |view, _| view.highlights.clone())
    }

    #[gpui::test]
    fn dragging_across_blocks_creates_a_highlight(cx: &mut TestAppContext) {
        let (view, cx) = open_reader(setup(), true, cx);
        // Do meio de "Um" até "Era uma" no parágrafo seguinte.
        select(&view, cx, (0, 1), (1, 7));
        assert_eq!(
            view.read_with(cx, |v, _| v.menu.as_ref().map(|m| m.target)),
            Some(MenuTarget::Selection)
        );

        view.update(cx, |view, cx| view.create_highlight(HighlightColor::Green, cx));
        let saved = highlights(&view, cx);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].quote, "m\nEra uma");
        assert_eq!((saved[0].chapter, saved[0].color), (0, HighlightColor::Green));
        view.read_with(cx, |view, _| {
            assert!(view.menu.is_none() && view.selection.is_none());
            assert_eq!(view.visible.len(), 1);
        });
    }

    #[gpui::test]
    fn selection_is_trimmed_and_backwards_drag_works(cx: &mut TestAppContext) {
        let (view, cx) = open_reader(setup(), true, cx);
        // Arrasta da direita para a esquerda, pegando o espaço antes de "vez".
        select(&view, cx, (1, 11), (1, 7));
        view.update(cx, |view, cx| view.create_highlight(HighlightColor::Yellow, cx));
        assert_eq!(highlights(&view, cx)[0].quote, "vez");
    }

    #[gpui::test]
    fn clicking_a_highlight_opens_its_menu(cx: &mut TestAppContext) {
        let (view, cx) = open_reader(setup(), true, cx);
        select(&view, cx, (1, 4), (1, 7));
        view.update(cx, |view, cx| view.create_highlight(HighlightColor::Yellow, cx));
        let id = highlights(&view, cx)[0].id;

        // Clique simples (sem arrastar) dentro de "uma".
        select(&view, cx, (1, 5), (1, 5));
        assert_eq!(
            view.read_with(cx, |v, _| v.menu.as_ref().map(|m| m.target)),
            Some(MenuTarget::Highlight(id))
        );

        view.update(cx, |view, cx| view.set_color(id, HighlightColor::Blue, cx));
        assert_eq!(highlights(&view, cx)[0].color, HighlightColor::Blue);

        view.update(cx, |view, cx| view.delete_highlight(id, cx));
        assert!(highlights(&view, cx).is_empty());
        assert!(view.read_with(cx, |v, _| v.visible.is_empty()));
    }

    #[gpui::test]
    fn adding_and_clearing_a_note(cx: &mut TestAppContext) {
        let (view, cx) = open_reader(setup(), true, cx);
        select(&view, cx, (1, 0), (1, 3));
        view.update(cx, |view, cx| view.create_highlight(HighlightColor::Pink, cx));
        let id = highlights(&view, cx)[0].id;
        // A nota é editada dentro do menu do destaque: abre o menu clicando nele.
        let open_note = |cx: &mut VisualTestContext| {
            select(&view, cx, (1, 1), (1, 1));
            cx.update(|window, cx| view.update(cx, |view, cx| view.start_note(id, window, cx)));
            cx.run_until_parked();
        };

        open_note(cx);
        cx.simulate_input("abertura clássica");
        cx.simulate_keystrokes("enter");
        assert_eq!(
            highlights(&view, cx)[0].note.as_deref(),
            Some("abertura clássica")
        );
        assert!(view.read_with(cx, |v, _| v.visible[0].has_note));

        // Esc no editor descarta sem salvar.
        open_note(cx);
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input("rascunho");
        cx.simulate_keystrokes("escape");
        assert_eq!(
            highlights(&view, cx)[0].note.as_deref(),
            Some("abertura clássica")
        );
        assert!(view.read_with(cx, |v, _| v.note_editor.is_none()));

        // Nota vazia apaga.
        open_note(cx);
        cx.simulate_keystrokes("ctrl-a backspace enter");
        assert_eq!(highlights(&view, cx)[0].note, None);
    }

    #[gpui::test]
    fn double_click_selects_word_and_escape_unwinds(cx: &mut TestAppContext) {
        let (view, cx) = open_reader(setup(), true, cx);
        let closed = Rc::new(Cell::new(false));
        let flag = closed.clone();
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, _: &ReaderEvent, _| flag.set(true))
                .detach();
        });

        view.update(cx, |view, cx| view.mouse_down(1, 5, 2, point(px(0.), px(0.)), cx));
        let selected = view.read_with(cx, |v, _| {
            v.selection.map(|s| v.book.text.text()[s.range()].to_owned())
        });
        assert_eq!(selected.as_deref(), Some("uma"));

        cx.simulate_keystrokes("ctrl-c");
        assert_eq!(
            cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
            Some("uma")
        );
        assert!(view.read_with(cx, |v, _| v.selection.is_none() && v.menu.is_none()));

        // Com seleção, o primeiro Esc só limpa; o seguinte fecha o leitor.
        view.update(cx, |view, cx| view.mouse_down(1, 5, 2, point(px(0.), px(0.)), cx));
        cx.simulate_keystrokes("escape");
        assert!(!closed.get());
        assert!(view.read_with(cx, |v, _| v.selection.is_none()));
        cx.simulate_keystrokes("escape");
        assert!(closed.get());
    }

    #[gpui::test]
    fn highlights_follow_text_that_moved(cx: &mut TestAppContext) {
        let setup = setup();
        let id = setup
            .library
            .add_highlight(
                setup.book_id,
                &NewHighlight {
                    chapter: 0,
                    chapter_title: None,
                    // Posição desatualizada: o trecho "uma" está em 7..10.
                    start: 0,
                    end: 3,
                    quote: "uma".into(),
                    color: HighlightColor::Yellow,
                },
            )
            .unwrap();
        let (view, cx) = open_reader(setup, true, cx);

        view.read_with(cx, |view, _| {
            assert_eq!(view.visible[0].range, 7..10);
            assert_eq!((view.highlights[0].start, view.highlights[0].end), (7, 10));
            assert_eq!(view.highlights[0].id, id);
        });
    }

    #[gpui::test]
    fn annotation_navigates_to_other_chapter(cx: &mut TestAppContext) {
        let setup = setup();
        setup
            .library
            .add_highlight(
                setup.book_id,
                // Capítulo 2 é "Três\nFim": "ê" ocupa dois bytes, então "Fim" está em 6..9.
                &NewHighlight {
                    chapter: 2,
                    chapter_title: None,
                    start: 6,
                    end: 9,
                    quote: "Fim".into(),
                    color: HighlightColor::Blue,
                },
            )
            .unwrap();
        let (view, cx) = open_reader(setup, true, cx);
        let id = highlights(&view, cx)[0].id;
        assert!(view.read_with(cx, |v, _| v.visible.is_empty()));

        view.update(cx, |view, cx| view.go_to_highlight(id, cx));
        view.read_with(cx, |view, _| {
            assert_eq!(view.book.current, 2);
            assert_eq!(view.visible[0].range, 6..9);
        });
    }

    #[gpui::test]
    fn standalone_reader_does_not_highlight(cx: &mut TestAppContext) {
        let (view, cx) = open_reader(setup(), false, cx);
        select(&view, cx, (1, 0), (1, 3));
        view.update(cx, |view, cx| view.create_highlight(HighlightColor::Yellow, cx));
        assert!(highlights(&view, cx).is_empty());
    }

    fn hits(view: &Entity<ReaderView>, cx: &mut VisualTestContext) -> Vec<(usize, String)> {
        view.read_with(cx, |view, _| {
            let index = match &view.search_index {
                SearchIndex::Ready(index) => index.clone(),
                _ => panic!("índice não montado"),
            };
            view.results
                .hits
                .iter()
                .map(|hit| {
                    (
                        hit.chapter,
                        index.chapters[hit.chapter].text[hit.ranges[0].clone()].to_owned(),
                    )
                })
                .collect()
        })
    }

    #[gpui::test]
    fn searching_the_whole_book(cx: &mut TestAppContext) {
        let (view, cx) = open_reader(setup(), true, cx);
        cx.simulate_keystrokes("ctrl-f");
        cx.run_until_parked();
        assert!(view.read_with(cx, |v, _| v.sidebar_tab == SidebarTab::Search));

        // Sem acento e em minúsculas, acha "Três" no terceiro capítulo.
        cx.simulate_input("tres");
        assert_eq!(hits(&view, cx), [(2, "Três".to_owned())]);

        // Enter vai ao resultado e marca a ocorrência no texto.
        cx.simulate_keystrokes("enter");
        view.read_with(cx, |v, _| {
            assert_eq!(v.book.current, 2);
            assert_eq!(v.active_hit, Some(0));
        });
        cx.run_until_parked();

        // Setas continuam editando o campo, não trocam de capítulo.
        cx.simulate_keystrokes("left left");
        assert_eq!(view.read_with(cx, |v, _| v.book.current), 2);

        // Esc no campo devolve o foco ao livro; o seguinte limpa a busca; o outro fecha.
        let closed = Rc::new(Cell::new(false));
        let flag = closed.clone();
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, _: &ReaderEvent, _| flag.set(true))
                .detach();
        });
        cx.simulate_keystrokes("escape");
        assert!(view.read_with(cx, |v, _| !v.results.hits.is_empty()));
        cx.simulate_keystrokes("escape");
        assert!(view.read_with(cx, |v, cx| v.results.hits.is_empty()
            && v.search_query(cx).is_empty()));
        assert!(!closed.get());
        cx.simulate_keystrokes("escape");
        assert!(closed.get());
    }

    #[gpui::test]
    fn stepping_through_results_wraps_around(cx: &mut TestAppContext) {
        let (view, cx) = open_reader(setup(), true, cx);
        cx.simulate_keystrokes("ctrl-f");
        cx.run_until_parked();
        // "Um" (título do capítulo 1) e "uma" (parágrafo do capítulo 1).
        cx.simulate_input("um");
        assert_eq!(hits(&view, cx), [(0, "Um".to_owned()), (0, "um".to_owned())]);

        cx.simulate_keystrokes("f3 f3");
        assert_eq!(view.read_with(cx, |v, _| v.active_hit), Some(1));
        cx.simulate_keystrokes("f3");
        assert_eq!(view.read_with(cx, |v, _| v.active_hit), Some(0));
        cx.simulate_keystrokes("shift-f3");
        assert_eq!(view.read_with(cx, |v, _| v.active_hit), Some(1));

        // Digitar de novo zera a posição.
        cx.simulate_input("a");
        assert_eq!(hits(&view, cx), [(0, "uma".to_owned())]);
        assert_eq!(view.read_with(cx, |v, _| v.active_hit), None);
        cx.run_until_parked();
    }

    #[gpui::test]
    fn first_result_follows_the_reading_position(cx: &mut TestAppContext) {
        let (view, cx) = open_reader(setup(), true, cx);
        view.update(cx, |view, cx| view.go_to(1, cx));
        cx.simulate_keystrokes("ctrl-f");
        cx.run_until_parked();
        // Só "Dois", no capítulo atual (o segundo), tem "oi".
        cx.simulate_input("o");
        cx.simulate_input("i");
        assert_eq!(hits(&view, cx), [(1, "oi".to_owned())]);
        cx.simulate_keystrokes("enter");
        assert_eq!(
            view.read_with(cx, |v, _| (v.book.current, v.active_hit)),
            (1, Some(0))
        );

        // Sem nada do capítulo atual em diante, volta ao começo do livro.
        view.update(cx, |view, cx| {
            view.go_to(2, cx);
            view.clear_search(cx);
        });
        cx.simulate_input("um");
        assert_eq!(hits(&view, cx).len(), 2);
        cx.simulate_keystrokes("enter");
        assert_eq!(
            view.read_with(cx, |v, _| (v.book.current, v.active_hit)),
            (0, Some(0))
        );
    }

    #[gpui::test]
    fn selection_becomes_the_query(cx: &mut TestAppContext) {
        let (view, cx) = open_reader(setup(), true, cx);
        view.update(cx, |view, cx| view.mouse_down(1, 5, 2, point(px(0.), px(0.)), cx));
        cx.simulate_keystrokes("ctrl-f");
        cx.run_until_parked();
        view.read_with(cx, |v, cx| {
            assert_eq!(v.search_query(cx), "uma");
            assert!(v.selection.is_none() && v.menu.is_none());
        });
        assert_eq!(hits(&view, cx), [(0, "uma".to_owned())]);
    }

    #[gpui::test]
    fn font_size_changes_and_is_remembered(cx: &mut TestAppContext) {
        let (view, cx) = open_reader(setup(), true, cx);
        cx.simulate_keystrokes("ctrl-= ctrl-=");
        assert_eq!(view.read_with(cx, |v, _| v.font_size), DEFAULT_FONT_SIZE + 2.);
        cx.simulate_keystrokes("ctrl--");
        assert_eq!(view.read_with(cx, |v, _| v.font_size), DEFAULT_FONT_SIZE + 1.);
        let saved = view.read_with(cx, |v, cx| {
            v.link
                .as_ref()
                .unwrap()
                .library
                .read(cx)
                .setting(FONT_SIZE_SETTING)
                .unwrap()
        });
        assert_eq!(saved.as_deref(), Some("19"));
        cx.simulate_keystrokes("ctrl-0");
        assert_eq!(view.read_with(cx, |v, _| v.font_size), DEFAULT_FONT_SIZE);
    }

    #[gpui::test]
    fn sidebar_lists_work_with_the_keyboard(cx: &mut TestAppContext) {
        let (view, cx) = open_reader(setup(), true, cx);
        // Sem sumário, o sumário lista os três arquivos; o cursor começa no atual.
        cx.simulate_keystrokes("ctrl-1");
        cx.update(|window, cx| assert!(view.read(cx).panel_focus.is_focused(window)));
        cx.simulate_keystrokes("down down enter");
        assert_eq!(view.read_with(cx, |v, _| v.book.current), 2);

        // Esc devolve o foco ao texto (sem fechar o livro); Ctrl+B esconde a lateral.
        cx.simulate_keystrokes("escape");
        cx.update(|window, cx| assert!(view.read(cx).focus_handle.is_focused(window)));
        cx.simulate_keystrokes("ctrl-b");
        assert!(view.read_with(cx, |v, _| !v.sidebar_visible));
        cx.simulate_keystrokes("ctrl-2");
        assert!(view.read_with(cx, |v, _| v.sidebar_visible
            && v.sidebar_tab == SidebarTab::Annotations));
    }

    #[gpui::test]
    fn space_types_in_the_search_field(cx: &mut TestAppContext) {
        let (view, cx) = open_reader(setup(), true, cx);
        cx.simulate_keystrokes("ctrl-f");
        cx.run_until_parked();
        cx.simulate_keystrokes("e r a space u");
        assert_eq!(view.read_with(cx, |v, cx| v.search_query(cx)), "era u");
        assert_eq!(hits(&view, cx), [(0, "Era u".to_owned())]);
        // ↓ no campo não rola o texto nem troca de capítulo.
        cx.simulate_keystrokes("down");
        assert_eq!(view.read_with(cx, |v, _| v.book.current), 0);
    }

    #[gpui::test]
    fn highlights_are_mirrored_to_obsidian(cx: &mut TestAppContext) {
        let setup = setup();
        let folder = setup._dir.path().join("vault/Leituras");
        setup
            .library
            .set_setting(obsidian::FOLDER_SETTING, &folder.to_string_lossy())
            .unwrap();
        let (view, cx) = open_reader(setup, true, cx);

        select(&view, cx, (1, 4), (1, 7));
        view.update(cx, |view, cx| view.create_highlight(HighlightColor::Green, cx));
        let note = folder.join("Livro — Autor.md");
        let content = std::fs::read_to_string(&note).unwrap();
        assert!(content.contains("> [!quote|verde] Destaque\n> uma\n"));
        assert!(
            content.contains("## Um\n"),
            "o título do capítulo vem do sumário: {content}"
        );

        let id = highlights(&view, cx)[0].id;
        view.update(cx, |view, cx| view.delete_highlight(id, cx));
        assert!(
            std::fs::read_to_string(&note)
                .unwrap()
                .contains("Nenhum destaque ainda")
        );
    }

    #[test]
    fn trims_whitespace_at_selection_edges() {
        assert_eq!(trim_range("a  bc \n", 1..7), Some(3..5));
        assert_eq!(trim_range("   ", 0..3), None);
    }
}

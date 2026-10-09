//! Janela principal: alterna entre a biblioteca e o leitor.

use std::path::{Path, PathBuf};

use gpui::{
    Context, Entity, FocusHandle, Focusable, FontWeight, MouseButton, Subscription, Window, actions, div,
    hsla, prelude::*, px, rgb,
};
use r_ereader_library::{BookId, Format, Library};

use crate::book::OpenBook;
use crate::components::{bar, kbd, section_label};
use crate::library_view::{LibraryEvent, LibraryView};
use crate::reader::{LibraryLink, ReaderEvent, ReaderView};
use crate::shortcuts::{self, Group};
use crate::theme;

actions!(workspace, [FocusNext, FocusPrevious, ToggleHelp, CloseHelp]);

pub const HELP_CONTEXT: &str = "Help";

/// Folha de atalhos aberta: guarda o foco anterior para devolvê-lo ao fechar.
struct Help {
    focus: FocusHandle,
    previous: Option<FocusHandle>,
}

pub struct Workspace {
    library: Entity<Library>,
    library_view: Entity<LibraryView>,
    reader: Option<(Entity<ReaderView>, Subscription)>,
    help: Option<Help>,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    pub fn new(
        library: Library,
        initial_file: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Primeira execução: aponta a exportação para o vault aberto do Obsidian, se houver.
        r_ereader_library::obsidian::configure_default(&library).ok();
        let library = cx.new(|_| library);
        let library_view = cx.new(|cx| LibraryView::new(library.clone(), window, cx));
        let subscription = cx.subscribe_in(
            &library_view,
            window,
            |this, _, event: &LibraryEvent, window, cx| match event {
                LibraryEvent::Read(id) => this.open_from_library(*id, window, cx),
            },
        );
        window.focus(&library_view.focus_handle(cx));

        let mut workspace = Workspace {
            library,
            library_view,
            reader: None,
            help: None,
            _subscriptions: vec![subscription],
        };
        if let Some(path) = initial_file {
            workspace.open_file(&path, window, cx);
        }
        workspace
    }

    fn open_from_library(&mut self, id: BookId, window: &mut Window, cx: &mut Context<Self>) {
        let path = match self.library.read(cx).file_path(id, Format::Epub) {
            Ok(Some(path)) => path,
            Ok(None) => return self.report("Este livro não tem um arquivo EPUB.".into(), cx),
            Err(e) => return self.report(e.to_string(), cx),
        };
        match OpenBook::open(&path) {
            Ok(book) => {
                let link = LibraryLink {
                    library: self.library.clone(),
                    book_id: id,
                };
                self.show_reader(book, Some(link), window, cx);
            }
            Err(e) => self.report(format!("Não foi possível abrir o livro: {e}"), cx),
        }
    }

    /// Arquivo passado pela linha de comando: abre direto no leitor, sem importar. Se o
    /// mesmo arquivo já está na biblioteca, usa o progresso e os destaques dela.
    fn open_file(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let known = self.library.read(cx).find_by_content(path).ok().flatten();
        match OpenBook::open(path) {
            Ok(book) => {
                let link = known.map(|book_id| LibraryLink {
                    library: self.library.clone(),
                    book_id,
                });
                self.show_reader(book, link, window, cx)
            }
            Err(e) => self.report(format!("Não foi possível abrir {}: {e}", path.display()), cx),
        }
    }

    fn show_reader(
        &mut self,
        book: OpenBook,
        link: Option<LibraryLink>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let reader = cx.new(|cx| ReaderView::new(book, link, cx));
        let subscription =
            cx.subscribe_in(
                &reader,
                window,
                |this, _, event: &ReaderEvent, window, cx| match event {
                    ReaderEvent::Close => this.close_reader(window, cx),
                },
            );
        window.focus(&reader.focus_handle(cx));
        self.reader = Some((reader, subscription));
        cx.notify();
    }

    fn close_reader(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reader = None;
        self.library_view.update(cx, |view, cx| view.reload(cx));
        window.focus(&self.library_view.focus_handle(cx));
        cx.notify();
    }

    fn toggle_help(&mut self, _: &ToggleHelp, window: &mut Window, cx: &mut Context<Self>) {
        if self.help.is_some() {
            return self.close_help(&CloseHelp, window, cx);
        }
        let focus = cx.focus_handle();
        let previous = window.focused(cx);
        window.focus(&focus);
        self.help = Some(Help { focus, previous });
        cx.notify();
    }

    fn close_help(&mut self, _: &CloseHelp, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(help) = self.help.take() {
            match help.previous {
                Some(previous) => window.focus(&previous),
                None => window.focus(&self.library_view.focus_handle(cx)),
            }
        }
        cx.notify();
    }

    fn render_help(&self, help: &Help, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let groups: Vec<&Group> = match self.reader {
            Some(_) => shortcuts::READER.iter().collect(),
            None => shortcuts::LIBRARY.iter().collect(),
        };
        let group = |group: &Group| {
            div()
                .flex()
                .flex_col()
                .child(div().pb_1().child(section_label(group.title)))
                .children(group.entries.iter().map(|(keys, description)| {
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .py(px(5.))
                        .border_t_1()
                        .border_color(rgb(theme::BORDER_SUBTLE))
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_1()
                                .w(px(190.))
                                .flex_none()
                                .children(keys.split("  ").map(kbd)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_sm()
                                .text_color(rgb(theme::TEXT))
                                .child(*description),
                        )
                }))
        };

        // Véu sobre a janela; clicar fora da folha fecha.
        div()
            .id("help-overlay")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(0.1, 0.2, 0.1, 0.25))
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close_help(&CloseHelp, window, cx)),
            )
            .child(
                div()
                    .id("help")
                    .track_focus(&help.focus)
                    .key_context(HELP_CONTEXT)
                    .on_action(cx.listener(Self::close_help))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .flex()
                    .flex_col()
                    .w(px(620.))
                    .max_h(px(720.))
                    .bg(rgb(theme::PANEL))
                    .border_1()
                    .border_color(rgb(theme::BORDER))
                    .shadow_lg()
                    .child(
                        bar()
                            .child(
                                div()
                                    .font_family(theme::READING_FONT)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(theme::TEXT))
                                    .child("Atalhos de teclado"),
                            )
                            .child(div().flex_1())
                            .child(kbd("Esc"))
                            .child(div().text_xs().text_color(rgb(theme::TEXT_MUTED)).child("fecha")),
                    )
                    .child(
                        div()
                            .id("help-body")
                            .flex()
                            .flex_col()
                            .gap_5()
                            .p_4()
                            .overflow_y_scroll()
                            .children(groups.into_iter().map(group))
                            .child(group(&shortcuts::GLOBAL)),
                    ),
            )
    }

    fn report(&mut self, message: String, cx: &mut Context<Self>) {
        self.library_view
            .update(cx, |view, cx| view.show_error(message, cx));
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.reader {
            Some((reader, _)) => reader.clone().into_any_element(),
            None => {
                window.set_window_title("r-ereader");
                self.library_view.clone().into_any_element()
            }
        };

        let help = self.help.as_ref().map(|help| self.render_help(help, cx));
        div()
            .on_action(|_: &FocusNext, window, _| window.focus_next())
            .on_action(|_: &FocusPrevious, window, _| window.focus_prev())
            .on_action(cx.listener(Self::toggle_help))
            .relative()
            .size_full()
            .bg(rgb(theme::BACKGROUND))
            .font_family(theme::UI_FONT)
            .text_color(rgb(theme::TEXT))
            .child(content)
            .children(help)
    }
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, VisualTestContext};
    use r_ereader_library::testing::write_epub;
    use tempfile::TempDir;

    use super::*;

    fn open_workspace<'a>(
        dir: &Path,
        cx: &'a mut TestAppContext,
    ) -> (Entity<Workspace>, BookId, &'a mut VisualTestContext) {
        let sources = dir.join("origem");
        std::fs::create_dir_all(&sources).unwrap();
        let mut library = Library::open(dir.join("biblioteca")).unwrap();
        let id = library
            .import(&write_epub(&sources, "a.epub", "Livro", "Autor", &[]))
            .unwrap()
            .id();
        cx.update(crate::bind_keys);
        let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::new(library, None, window, cx));
        (workspace, id, cx)
    }

    fn read(workspace: &Entity<Workspace>, id: BookId, cx: &mut VisualTestContext) {
        let library_view = workspace.read_with(cx, |w, _| w.library_view.clone());
        library_view.update(cx, |_, cx| cx.emit(LibraryEvent::Read(id)));
        cx.run_until_parked();
    }

    #[gpui::test]
    fn reading_saves_progress_and_resumes(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let (workspace, id, cx) = open_workspace(dir.path(), cx);

        read(&workspace, id, cx);
        assert!(workspace.read_with(cx, |w, _| w.reader.is_some()));

        cx.simulate_keystrokes("right right right");
        cx.simulate_keystrokes("escape");
        assert!(workspace.read_with(cx, |w, _| w.reader.is_none()));

        let library = workspace.read_with(cx, |w, _| w.library.clone());
        let progress = library.read_with(cx, |l, _| l.progress(id).unwrap().unwrap());
        // Três capítulos: a terceira seta não passa do último.
        assert_eq!(progress.chapter, 2);
        assert_eq!(progress.fraction, 1.0);

        // Volta um capítulo e reabre: deve continuar de onde parou.
        read(&workspace, id, cx);
        cx.simulate_keystrokes("left escape");
        read(&workspace, id, cx);
        cx.simulate_keystrokes("escape");
        let progress = library.read_with(cx, |l, _| l.progress(id).unwrap().unwrap());
        assert_eq!(progress.chapter, 1);
    }

    #[gpui::test]
    fn library_shows_reading_count_after_closing_reader(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let (workspace, id, cx) = open_workspace(dir.path(), cx);
        read(&workspace, id, cx);
        cx.simulate_keystrokes("right escape");

        let library_view = workspace.read_with(cx, |w, _| w.library_view.clone());
        cx.update(|window, cx| {
            assert!(
                library_view.focus_handle(cx).is_focused(window),
                "a biblioteca deveria recuperar o foco"
            );
        });
        let count = library_view.read_with(cx, |view, _| view.reading_count());
        assert_eq!(count, 1);
    }

    #[gpui::test]
    fn opening_a_library_file_directly_keeps_the_link(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let sources = dir.path().join("origem");
        std::fs::create_dir_all(&sources).unwrap();
        let source = write_epub(&sources, "a.epub", "Livro", "Autor", &[]);
        let mut library = Library::open(dir.path().join("biblioteca")).unwrap();
        let id = library.import(&source).unwrap().id();
        cx.update(crate::bind_keys);
        let (workspace, cx) =
            cx.add_window_view(|window, cx| Workspace::new(library, Some(source), window, cx));

        cx.simulate_keystrokes("right escape");
        let library = workspace.read_with(cx, |w, _| w.library.clone());
        let progress = library.read_with(cx, |l, _| l.progress(id).unwrap());
        assert_eq!(progress.map(|p| p.chapter), Some(1));
    }

    #[gpui::test]
    fn f1_shows_shortcuts_and_escape_only_closes_the_sheet(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let (workspace, id, cx) = open_workspace(dir.path(), cx);
        read(&workspace, id, cx);

        cx.simulate_keystrokes("f1");
        assert!(workspace.read_with(cx, |w, _| w.help.is_some()));
        cx.simulate_keystrokes("escape");
        workspace.read_with(cx, |w, _| {
            assert!(w.help.is_none());
            assert!(w.reader.is_some(), "Esc na folha não fecha o livro");
        });
        // O foco volta ao texto: a seta troca de capítulo.
        cx.simulate_keystrokes("right escape");
        let library = workspace.read_with(cx, |w, _| w.library.clone());
        let progress = library.read_with(cx, |l, _| l.progress(id).unwrap().unwrap());
        assert_eq!(progress.chapter, 1);
    }

    #[gpui::test]
    fn tab_cycles_between_library_panes(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let (workspace, _, cx) = open_workspace(dir.path(), cx);
        let library_view = workspace.read_with(cx, |w, _| w.library_view.clone());
        let focused = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| library_view.read(cx).focused_pane(window, cx))
        };
        assert_eq!(focused(cx), "livros");
        cx.simulate_keystrokes("tab");
        assert_eq!(focused(cx), "navegador");
        cx.simulate_keystrokes("tab");
        assert_eq!(focused(cx), "busca");
        cx.simulate_keystrokes("tab");
        assert_eq!(focused(cx), "livros");
        cx.simulate_keystrokes("shift-tab");
        assert_eq!(focused(cx), "busca");
    }

    #[gpui::test]
    fn missing_epub_reports_error_in_library(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let (workspace, id, cx) = open_workspace(dir.path(), cx);
        let path = workspace.read_with(cx, |w, cx| {
            w.library.read(cx).file_path(id, Format::Epub).unwrap().unwrap()
        });
        std::fs::remove_file(path).unwrap();

        read(&workspace, id, cx);
        assert!(workspace.read_with(cx, |w, _| w.reader.is_none()));
        let has_error = workspace.read_with(cx, |w, cx| w.library_view.read(cx).has_error());
        assert!(has_error);
    }
}

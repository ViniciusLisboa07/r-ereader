mod book;
mod chapter_text;
mod components;
mod input;
mod library_view;
mod nav;
mod preview;
mod reader;
mod search;
mod shortcuts;
mod theme;
mod workspace;

use std::path::PathBuf;

use gpui::{
    App, Application, Bounds, KeyBinding, TitlebarOptions, WindowBounds, WindowOptions, actions, prelude::*,
    px, size,
};
use r_ereader_library::Library;

use workspace::Workspace;

actions!(r_ereader, [Quit]);

fn main() {
    let initial_file = std::env::args_os().nth(1).map(PathBuf::from);
    let library = match Library::open(Library::default_root()) {
        Ok(library) => library,
        Err(e) => {
            eprintln!("r-ereader: não foi possível abrir a biblioteca: {e}");
            std::process::exit(1);
        }
    };

    Application::new().run(move |cx: &mut App| {
        theme::load_fonts(cx);
        bind_keys(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(1320.), px(860.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("r-ereader".into()),
                    ..Default::default()
                }),
                app_id: Some("r-ereader".into()),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| Workspace::new(library, initial_file, window, cx)),
        )
        .expect("falha ao abrir a janela");
        cx.activate(true);
    });
}

fn bind_keys(cx: &mut App) {
    use library_view::{DeleteBook, Dismiss, EditBook, FocusSearch, ImportBooks, ShowGrid, ShowList};
    use reader::{
        CloseReader, CopySelection, DecreaseFontSize, FindInBook, FocusText, IncreaseFontSize, NextChapter,
        NextMatch, PreviousChapter, PreviousMatch, ResetFontSize, ScrollLineDown, ScrollLineUp,
        ScrollPageDown, ScrollPageUp, ScrollToEnd, ScrollToStart, ShowAnnotations, ShowContents,
        ToggleSidebar,
    };
    use workspace::{CloseHelp, FocusNext, FocusPrevious, ToggleHelp};

    input::bind_keys(cx);
    nav::bind_keys(cx);
    cx.bind_keys([
        KeyBinding::new("secondary-q", Quit, None),
        KeyBinding::new("tab", FocusNext, None),
        KeyBinding::new("shift-tab", FocusPrevious, None),
        KeyBinding::new("f1", ToggleHelp, None),
        KeyBinding::new("secondary-/", ToggleHelp, None),
        KeyBinding::new("escape", CloseHelp, Some(workspace::HELP_CONTEXT)),
        KeyBinding::new("secondary-o", ImportBooks, Some(library_view::CONTEXT)),
        KeyBinding::new("secondary-f", FocusSearch, Some(library_view::CONTEXT)),
        KeyBinding::new("escape", Dismiss, Some(library_view::CONTEXT)),
        KeyBinding::new("secondary-1", ShowGrid, Some(library_view::CONTEXT)),
        KeyBinding::new("secondary-2", ShowList, Some(library_view::CONTEXT)),
        KeyBinding::new("f2", EditBook, Some(library_view::CONTEXT)),
        KeyBinding::new("delete", DeleteBook, Some(nav::BOOKS)),
        KeyBinding::new("right", NextChapter, Some(reader::CONTEXT)),
        KeyBinding::new("left", PreviousChapter, Some(reader::CONTEXT)),
        KeyBinding::new("escape", CloseReader, Some(reader::CONTEXT)),
        KeyBinding::new("secondary-c", CopySelection, Some(reader::CONTEXT)),
        KeyBinding::new("secondary-f", FindInBook, Some(reader::CONTEXT)),
        KeyBinding::new("secondary-g", NextMatch, Some(reader::CONTEXT)),
        KeyBinding::new("f3", NextMatch, Some(reader::CONTEXT)),
        KeyBinding::new("secondary-shift-g", PreviousMatch, Some(reader::CONTEXT)),
        KeyBinding::new("shift-f3", PreviousMatch, Some(reader::CONTEXT)),
        KeyBinding::new("secondary-b", ToggleSidebar, Some(reader::CONTEXT)),
        KeyBinding::new("secondary-1", ShowContents, Some(reader::CONTEXT)),
        KeyBinding::new("secondary-2", ShowAnnotations, Some(reader::CONTEXT)),
        KeyBinding::new("secondary-3", FindInBook, Some(reader::CONTEXT)),
        KeyBinding::new("secondary-=", IncreaseFontSize, Some(reader::CONTEXT)),
        KeyBinding::new("secondary-+", IncreaseFontSize, Some(reader::CONTEXT)),
        KeyBinding::new("secondary-shift-=", IncreaseFontSize, Some(reader::CONTEXT)),
        KeyBinding::new("secondary--", DecreaseFontSize, Some(reader::CONTEXT)),
        KeyBinding::new("secondary-0", ResetFontSize, Some(reader::CONTEXT)),
        KeyBinding::new("up", ScrollLineUp, Some(reader::TEXT_CONTEXT)),
        KeyBinding::new("down", ScrollLineDown, Some(reader::TEXT_CONTEXT)),
        KeyBinding::new("pageup", ScrollPageUp, Some(reader::TEXT_CONTEXT)),
        KeyBinding::new("pagedown", ScrollPageDown, Some(reader::TEXT_CONTEXT)),
        KeyBinding::new("shift-space", ScrollPageUp, Some(reader::TEXT_CONTEXT)),
        KeyBinding::new("space", ScrollPageDown, Some(reader::TEXT_CONTEXT)),
        KeyBinding::new("home", ScrollToStart, Some(reader::TEXT_CONTEXT)),
        KeyBinding::new("end", ScrollToEnd, Some(reader::TEXT_CONTEXT)),
        KeyBinding::new("escape", FocusText, Some(nav::READER_PANEL)),
    ]);
}

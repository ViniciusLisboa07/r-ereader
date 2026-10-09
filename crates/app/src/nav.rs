//! Navegação por teclado nas listas: grade de livros, navegador lateral, sumário,
//! anotações e resultados de busca. As ações são as mesmas; cada lista define o que
//! "subir" ou "confirmar" significa no seu contexto.

use gpui::{App, KeyBinding, actions};

actions!(
    nav,
    [
        MoveUp, MoveDown, MoveLeft, MoveRight, PageUp, PageDown, MoveFirst, MoveLast, Confirm
    ]
);

/// Contextos de teclado das listas navegáveis.
pub const BOOKS: &str = "Books";
pub const SIDEBAR: &str = "LibrarySidebar";
pub const READER_PANEL: &str = "ReaderPanel";

pub fn bind_keys(cx: &mut App) {
    for context in [BOOKS, SIDEBAR, READER_PANEL] {
        cx.bind_keys([
            KeyBinding::new("up", MoveUp, Some(context)),
            KeyBinding::new("down", MoveDown, Some(context)),
            KeyBinding::new("left", MoveLeft, Some(context)),
            KeyBinding::new("right", MoveRight, Some(context)),
            KeyBinding::new("pageup", PageUp, Some(context)),
            KeyBinding::new("pagedown", PageDown, Some(context)),
            KeyBinding::new("home", MoveFirst, Some(context)),
            KeyBinding::new("end", MoveLast, Some(context)),
            KeyBinding::new("enter", Confirm, Some(context)),
            KeyBinding::new("space", Confirm, Some(context)),
        ]);
    }
}

/// Novo índice depois de andar `delta` numa lista de `len` itens, sem dar a volta.
/// Sem item atual, qualquer movimento para a frente cai no primeiro e para trás no último.
pub fn step(current: Option<usize>, len: usize, delta: isize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let last = len as isize - 1;
    let next = match current {
        Some(index) => (index as isize + delta).clamp(0, last),
        None if delta >= 0 => 0,
        None => last,
    };
    Some(next as usize)
}

#[cfg(test)]
mod tests {
    use super::step;

    #[test]
    fn steps_stay_inside_the_list() {
        assert_eq!(step(None, 0, 1), None);
        assert_eq!(step(None, 5, 1), Some(0));
        assert_eq!(step(None, 5, -1), Some(4));
        assert_eq!(step(Some(1), 5, 3), Some(4));
        assert_eq!(step(Some(4), 5, 3), Some(4));
        assert_eq!(step(Some(2), 5, -10), Some(0));
        assert_eq!(step(Some(9), 5, 0), Some(4));
    }
}

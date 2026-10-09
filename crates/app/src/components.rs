//! Peças visuais reutilizadas pelas telas.
//!
//! Linguagem comum: cantos quase retos, pautas de 1px separando regiões e números em
//! fonte mono, como num fichário.

use std::path::PathBuf;

use gpui::{
    Div, ElementId, FontWeight, ObjectFit, Pixels, SharedString, Stateful, div, img, prelude::*, px, rgb,
};

use crate::theme;

/// Altura das barras (ferramentas, cabeçalhos de painel, abas).
pub const BAR_HEIGHT: f32 = 38.;
pub const STATUS_HEIGHT: f32 = 26.;

/// Botão discreto, sem fundo até receber o mouse.
pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    base_button(id, label, theme::TEXT_MUTED, theme::TEXT)
}

/// Botão com pauta em volta, para ações secundárias em painéis.
pub fn outline_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    base_button(id, label, theme::TEXT, theme::TEXT)
        .border_1()
        .border_color(rgb(theme::BORDER))
        .bg(rgb(theme::INPUT))
        .py(px(5.))
}

pub fn danger_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    base_button(id, label, theme::ERROR, theme::ERROR)
        .border_1()
        .border_color(rgb(theme::BORDER))
        .bg(rgb(theme::INPUT))
        .py(px(5.))
}

// O GPUI só aceita um `hover` por elemento, por isso as variações partem daqui.
fn base_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    color: u32,
    hover_color: u32,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .gap_2()
        .px_2()
        .py(px(3.))
        .rounded_sm()
        .text_sm()
        .text_color(rgb(color))
        .cursor_pointer()
        .hover(move |this| this.bg(rgb(theme::ELEMENT_HOVER)).text_color(rgb(hover_color)))
        .active(|this| this.bg(rgb(theme::ELEMENT_SELECTED)))
        .child(label.into())
}

pub fn primary_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .gap_2()
        .px_3()
        .py(px(6.))
        .rounded_sm()
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .bg(rgb(theme::ACCENT))
        .text_color(rgb(theme::ON_ACCENT))
        .cursor_pointer()
        .hover(|this| this.bg(rgb(theme::ACCENT_HOVER)))
        .child(label.into())
}

pub fn disabled_button(label: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .px_3()
        .py(px(6.))
        .rounded_sm()
        .text_sm()
        .border_1()
        .border_color(rgb(theme::BORDER_SUBTLE))
        .text_color(rgb(theme::TEXT_PLACEHOLDER))
        .child(label.into())
}

/// Tecla de atalho, desenhada como uma etiqueta mono.
pub fn kbd(keys: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .px(px(4.))
        .rounded_sm()
        .border_1()
        .border_color(rgb(theme::BORDER))
        .bg(rgb(theme::INPUT))
        .font_family(theme::MONO_FONT)
        .text_size(px(10.5))
        .line_height(px(15.))
        .text_color(rgb(theme::TEXT_MUTED))
        .child(keys.into())
}

/// Texto mono pequeno: números de catálogo, contagens, caminhos.
pub fn mono(text: impl Into<SharedString>) -> Div {
    div()
        .font_family(theme::MONO_FONT)
        .text_xs()
        .text_color(rgb(theme::TEXT_PLACEHOLDER))
        .child(text.into())
}

/// Número de catálogo de um livro (o id na biblioteca).
pub fn catalog_number(id: i64) -> String {
    format!("Nº {id:04}")
}

pub fn chip(label: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .px(px(6.))
        .py(px(1.))
        .rounded_sm()
        .border_1()
        .border_color(rgb(theme::BORDER))
        .text_xs()
        .text_color(rgb(theme::TEXT_MUTED))
        .child(label.into())
}

pub fn section_label(label: impl Into<SharedString>) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(theme::TEXT_MUTED))
        .child(label.into())
}

/// Barra de cabeçalho de painel: altura fixa, pauta embaixo.
pub fn bar() -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .h(px(BAR_HEIGHT))
        .px_3()
        .border_b_1()
        .border_color(rgb(theme::BORDER))
}

/// Segmento da barra de status, separado do vizinho por uma pauta vertical.
pub fn status_segment(content: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .h_full()
        .px_3()
        .border_l_1()
        .border_color(rgb(theme::BORDER_SUBTLE))
        .child(content)
}

/// Capa do livro, ou uma capa gerada com o título quando não há imagem.
pub fn cover(image: Option<PathBuf>, title: &str, format: &str, width: Pixels, height: Pixels) -> Div {
    let frame = div()
        .flex_none()
        .w(width)
        .h(height)
        .overflow_hidden()
        .border_1()
        .border_color(rgb(theme::BORDER))
        .shadow_sm();

    match image {
        Some(path) => frame.child(img(path).size_full().object_fit(ObjectFit::Cover)),
        None => {
            // FNV-1a: títulos parecidos ainda caem em cores diferentes.
            let hash = title
                .bytes()
                .fold(0x811c9dc5u32, |h, b| (h ^ b as u32).wrapping_mul(0x01000193));
            let color =
                theme::PLACEHOLDER_COVERS[(hash ^ (hash >> 16)) as usize % theme::PLACEHOLDER_COVERS.len()];
            let large = width > px(100.);
            frame
                .flex()
                .flex_col()
                .justify_between()
                .p(if large { px(12.) } else { px(4.) })
                .bg(rgb(color))
                .text_color(rgb(theme::ON_ACCENT))
                .when(large, |this| {
                    this.child(
                        div()
                            .font_family(theme::MONO_FONT)
                            .text_xs()
                            .opacity(0.75)
                            .child(SharedString::from(format.to_owned())),
                    )
                    .child(
                        div()
                            .font_family(theme::READING_FONT)
                            .text_size(if width > px(200.) { px(21.) } else { px(15.) })
                            .line_height(px(if width > px(200.) { 27. } else { 19. }))
                            .line_clamp(6)
                            .child(SharedString::from(title.to_owned())),
                    )
                })
        }
    }
}

pub fn progress_bar(fraction: f32) -> Div {
    div().h(px(2.)).w_full().bg(rgb(theme::BORDER_SUBTLE)).child(
        div()
            .h_full()
            .bg(rgb(theme::ACCENT))
            .w(gpui::relative(fraction.clamp(0., 1.))),
    )
}

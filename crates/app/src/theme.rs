//! Paleta "fichário": papel sépia, pautas finas separando os painéis e tinta azul de
//! caneta para o que está ativo, como anotação na margem.

use std::borrow::Cow;

use gpui::App;

/// Papel: área de leitura e da grade de livros.
pub const BACKGROUND: u32 = 0xf4eddc;
/// Papel de fichário: painéis laterais e barras.
pub const PANEL: u32 = 0xebe3cf;
pub const SURFACE: u32 = 0xe2d8c0;
pub const INPUT: u32 = 0xfaf6ec;
/// Pauta: as linhas que separam painéis, células e linhas de tabela.
pub const BORDER: u32 = 0xcfc1a0;
pub const BORDER_SUBTLE: u32 = 0xe0d5bb;
pub const ELEMENT_HOVER: u32 = 0xe5dcc6;
pub const ELEMENT_SELECTED: u32 = 0xddd3ba;

pub const TEXT: u32 = 0x26211a;
pub const TEXT_MUTED: u32 = 0x675e4f;
pub const TEXT_PLACEHOLDER: u32 = 0x978c76;
/// Azul de caneta-tinteiro: foco, item ativo, ação principal.
pub const ACCENT: u32 = 0x2b5288;
pub const ACCENT_HOVER: u32 = 0x1f3f6c;
pub const ON_ACCENT: u32 = 0xfaf6ec;
pub const SELECTION: u32 = 0xc8d4e1;
/// Ocorrências da busca (a selecionada usa `ACCENT`).
pub const MATCH: u32 = 0xf0b37a;
/// Vermelho de carimbo.
pub const ERROR: u32 = 0xa83a2b;
pub const SUCCESS: u32 = 0x4b6a33;

/// Fundos das capas geradas para livros sem capa (PDFs, por exemplo): tons de tecido
/// de encadernação.
pub const PLACEHOLDER_COVERS: [u32; 6] = [0x7d4b3a, 0x52643f, 0x3d5a73, 0x8a6a2e, 0x6a4a63, 0x47584f];

pub const UI_FONT: &str = "IBM Plex Sans";
pub const MONO_FONT: &str = "IBM Plex Mono";
pub const READING_FONT: &str = "Literata";

/// Fontes embutidas no binário (licença OFL, em `assets/fonts`), para o app ter a mesma
/// cara em qualquer máquina.
pub fn load_fonts(cx: &App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexSans-Italic.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexSans-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexMono-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Literata-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Literata-Italic.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Literata-SemiBold.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Literata-SemiBoldItalic.ttf")),
    ];
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        eprintln!("r-ereader: não foi possível carregar as fontes embutidas: {e}");
    }
}

/// Cores dos destaques: tons pastel que funcionam sobre o papel sépia.
pub fn highlight(color: r_ereader_library::HighlightColor) -> u32 {
    use r_ereader_library::HighlightColor::*;
    match color {
        Yellow => 0xf3d36e,
        Green => 0xbcd68f,
        Blue => 0xaecbe3,
        Pink => 0xedb4bd,
    }
}

pub fn highlight_name(color: r_ereader_library::HighlightColor) -> &'static str {
    use r_ereader_library::HighlightColor::*;
    match color {
        Yellow => "Amarelo",
        Green => "Verde",
        Blue => "Azul",
        Pink => "Rosa",
    }
}

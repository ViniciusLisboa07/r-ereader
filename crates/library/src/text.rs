//! Utilitários de texto: chaves de ordenação, nomes de arquivo e limpeza de HTML.

use r_ereader_epub::dom;

/// Remove acentos dos caracteres latinos mais comuns, para ordenar "Ética" junto do "E".
pub fn fold_diacritics(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'Á' | 'À' | 'Â' | 'Ã' | 'Ä' | 'Å' => 'A',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'É' | 'È' | 'Ê' | 'Ë' => 'E',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'Í' | 'Ì' | 'Î' | 'Ï' => 'I',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'O',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'Ú' | 'Ù' | 'Û' | 'Ü' => 'U',
            'ç' => 'c',
            'Ç' => 'C',
            'ñ' => 'n',
            'Ñ' => 'N',
            other => other,
        })
        .collect()
}

const LEADING_ARTICLES: &[&str] = &["the ", "a ", "an ", "o ", "os ", "as ", "um ", "uma "];

/// Chave de ordenação do título: sem acento, minúsculo e sem artigo inicial.
pub fn sort_title(title: &str) -> String {
    let folded = fold_diacritics(title.trim()).to_lowercase();
    LEADING_ARTICLES
        .iter()
        .find_map(|article| folded.strip_prefix(article))
        .filter(|rest| !rest.trim().is_empty())
        .map_or(folded.clone(), |rest| rest.trim_start().to_owned())
}

const NAME_PARTICLES: &[&str] = &["de", "da", "do", "das", "dos", "e", "van", "von", "der"];
const NAME_SUFFIXES: &[&str] = &[
    "phd", "ph.d.", "jr", "jr.", "sr", "sr.", "st.", "filho", "neto", "ii", "iii",
];

/// Padroniza iniciais: "C.S. Lewis" → "C. S. Lewis", para não virar outro autor.
pub fn normalize_author(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '.' && chars.peek().is_some_and(|next| next.is_uppercase()) {
            out.push(' ');
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "Machado de Assis" → "Assis, Machado de". Nomes já invertidos ficam como estão.
pub fn author_sort_name(name: &str) -> String {
    let name = name.trim();
    if name.contains(',') {
        return name.to_owned();
    }
    let mut words: Vec<&str> = name.split_whitespace().collect();
    let suffix = match words.last() {
        Some(last) if words.len() >= 2 && NAME_SUFFIXES.contains(&last.to_lowercase().as_str()) => {
            words.pop()
        }
        _ => None,
    };
    let inverted = match words.split_last() {
        Some((last, rest)) if !rest.is_empty() && !NAME_PARTICLES.contains(&last.to_lowercase().as_str()) => {
            format!("{last}, {}", rest.join(" "))
        }
        _ => words.join(" "),
    };
    match suffix {
        Some(suffix) => format!("{inverted} {suffix}"),
        None => inverted,
    }
}

/// Componente de caminho seguro em qualquer sistema de arquivos.
pub fn sanitize_file_name(name: &str, max_chars: usize) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let truncated: String = collapsed.chars().take(max_chars).collect();
    let trimmed = truncated.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() {
        "_".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Descrições de EPUB costumam vir em HTML; guardamos só o texto, com parágrafos.
pub fn html_to_text(html: &str) -> String {
    if !html.contains('<') {
        return html.trim().to_owned();
    }
    match dom::parse(&format!("<div>{html}</div>")) {
        Ok(root) => {
            let mut paragraphs = Vec::new();
            collect_paragraphs(&root, &mut paragraphs);
            paragraphs.join("\n\n")
        }
        Err(_) => strip_tags(html),
    }
}

fn collect_paragraphs(element: &dom::Element, out: &mut Vec<String>) {
    let has_block_children = element.elements().any(|e| {
        matches!(
            e.name.as_str(),
            "p" | "div" | "li" | "br" | "h1" | "h2" | "h3" | "blockquote"
        )
    });
    if !has_block_children {
        let text = collapse(&element.text());
        if !text.is_empty() {
            out.push(text);
        }
        return;
    }
    let mut loose = String::new();
    for node in &element.children {
        match node {
            dom::Node::Text(t) => loose.push_str(t),
            dom::Node::Element(e) if e.name == "br" => flush(&mut loose, out),
            dom::Node::Element(e)
                if matches!(e.name.as_str(), "span" | "em" | "i" | "b" | "strong" | "a") =>
            {
                loose.push_str(&e.text())
            }
            dom::Node::Element(e) => {
                flush(&mut loose, out);
                collect_paragraphs(e, out);
            }
        }
    }
    flush(&mut loose, out);
}

fn flush(loose: &mut String, out: &mut Vec<String>) {
    let text = collapse(&std::mem::take(loose));
    if !text.is_empty() {
        out.push(text);
    }
}

fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    collapse(&out)
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Título de reserva a partir do nome do arquivo: `livro_x.pdf (1).pdf` → `livro x`.
pub fn title_from_file_stem(stem: &str) -> String {
    let mut title = stem.trim().to_owned();
    loop {
        let before = title.clone();
        if let Some(open) = title.rfind(" (")
            && title.ends_with(')')
            && is_copy_marker(&title[open + 2..title.len() - 1])
        {
            title.truncate(open);
        }
        for extension in [".pdf", ".epub"] {
            let cut = title.len().saturating_sub(extension.len());
            if title.is_char_boundary(cut) && title[cut..].eq_ignore_ascii_case(extension) {
                title.truncate(cut);
            }
        }
        if title == before {
            break;
        }
    }
    title
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// "(1)", "(2)"…: sufixo que navegadores põem em downloads repetidos. Anos como
/// "(2023)" fazem parte do título.
fn is_copy_marker(inside: &str) -> bool {
    (1..=2).contains(&inside.len()) && inside.chars().all(|c| c.is_ascii_digit())
}

/// Divide listas digitadas pelo usuário ("a, b; c") em itens limpos e sem repetição.
pub fn split_list(input: &str) -> Vec<String> {
    let mut items: Vec<String> = Vec::new();
    for item in input.split([',', ';']).map(collapse).filter(|i| !i.is_empty()) {
        if !items.iter().any(|existing| existing.eq_ignore_ascii_case(&item)) {
            items.push(item);
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_title_folds_accents_and_drops_articles() {
        assert_eq!(sort_title("Ética a Nicômaco"), "etica a nicomaco");
        assert_eq!(sort_title("O Senhor dos Anéis"), "senhor dos aneis");
        assert_eq!(sort_title("The Pragmatic Programmer"), "pragmatic programmer");
        assert_eq!(sort_title("A"), "a");
    }

    #[test]
    fn author_sort_name_inverts_last_name() {
        assert_eq!(author_sort_name("Machado de Assis"), "Assis, Machado de");
        assert_eq!(author_sort_name("Lewis, C. S."), "Lewis, C. S.");
        assert_eq!(author_sort_name("Platão"), "Platão");
        assert_eq!(author_sort_name("Nicole Forsgren PhD"), "Forsgren, Nicole PhD");
        assert_eq!(author_sort_name("Forsgren PhD"), "Forsgren PhD");
        assert_eq!(
            author_sort_name("Martin Luther King Jr."),
            "King, Martin Luther Jr."
        );
    }

    #[test]
    fn normalizes_author_initials() {
        assert_eq!(normalize_author("C.S.  Lewis"), "C. S. Lewis");
        assert_eq!(normalize_author("J.R.R. Tolkien"), "J. R. R. Tolkien");
        assert_eq!(normalize_author("Machado de Assis"), "Machado de Assis");
    }

    #[test]
    fn sanitizes_file_names() {
        assert_eq!(
            sanitize_file_name("Absalão: uma/história?", 80),
            "Absalão_ uma_história_"
        );
        assert_eq!(sanitize_file_name("...", 80), "_");
        assert_eq!(sanitize_file_name("abcdef", 3), "abc");
    }

    #[test]
    fn converts_html_descriptions() {
        assert_eq!(
            html_to_text("<p>Primeiro <em>parágrafo</em>.</p><p>Segundo&nbsp;aqui</p>"),
            "Primeiro parágrafo.\n\nSegundo aqui"
        );
        assert_eq!(html_to_text("texto puro"), "texto puro");
        assert_eq!(html_to_text("<p>quebrado <b>sem fechar"), "quebrado sem fechar");
    }

    #[test]
    fn derives_title_from_file_name() {
        assert_eq!(
            title_from_file_stem("instrucao-geral_0472723.pdf (1)"),
            "instrucao-geral 0472723"
        );
        assert_eq!(title_from_file_stem("Livro (2023)"), "Livro (2023)");
    }

    #[test]
    fn splits_user_lists() {
        assert_eq!(
            split_list("ficção,  Teologia; ficção ,, "),
            ["ficção", "Teologia"]
        );
    }
}

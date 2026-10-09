//! Busca no livro inteiro.
//!
//! Os textos são comparados "dobrados": minúsculas, sem acentos, com aspas, apóstrofos e
//! travessões tipográficos trocados pelos simples e espaços repetidos reduzidos a um. Assim
//! "nao" acha "Não" e "d'avila" acha "d’Ávila". Cada byte dobrado guarda a posição do
//! caractere original, e os resultados voltam em offsets do texto do capítulo, os mesmos
//! que o leitor usa para desenhar destaques.
//!
//! Com mais de uma palavra, além da frase exata entram os parágrafos que têm todas as
//! palavras em qualquer ordem, listados depois.

use std::ops::Range;
use std::path::Path;

use r_ereader_epub::Epub;

use crate::chapter_text::ChapterText;
use crate::preview::{self, Block};

/// Buscas mais curtas que isso casariam com quase tudo.
pub const MIN_QUERY_CHARS: usize = 2;
pub const MAX_HITS: usize = 1000;

const CONTEXT_BEFORE: usize = 60;
const SNIPPET_LENGTH: usize = 200;
const ELLIPSIS: &str = "…";

/// Texto de um bloco dobrado, com o caminho de volta para o original.
struct Folded {
    text: String,
    /// Para cada byte de `text`, o offset no capítulo do caractere que o gerou; a entrada
    /// extra no fim marca o fim do bloco.
    origin: Vec<u32>,
    /// Intervalo do bloco no texto do capítulo.
    range: Range<usize>,
}

impl Folded {
    fn new(text: &str, base: usize) -> Self {
        let mut folded = String::with_capacity(text.len());
        let mut origin = Vec::with_capacity(text.len() + 1);
        for (index, c) in text.char_indices() {
            if c.is_whitespace() && (folded.is_empty() || folded.ends_with(' ')) {
                continue;
            }
            let at = (base + index) as u32;
            let mut push = |c: char| {
                folded.push(c);
                origin.extend(std::iter::repeat_n(at, c.len_utf8()));
            };
            if c.is_whitespace() {
                push(' ');
                continue;
            }
            match c {
                '‘' | '’' | '‚' | 'ʼ' | '´' | '`' => push('\''),
                '“' | '”' | '„' | '«' | '»' => push('"'),
                '‐' | '‑' | '‒' | '–' | '—' | '―' => push('-'),
                _ => c.to_lowercase().for_each(|lower| push(strip_diacritic(lower))),
            }
        }
        origin.push((base + text.len()) as u32);
        Folded {
            text: folded,
            origin,
            range: base..base + text.len(),
        }
    }

    /// Ocorrências de `needle` (já dobrada) em offsets do capítulo.
    fn find<'a>(&'a self, needle: &'a str) -> impl Iterator<Item = Range<usize>> + 'a {
        self.text
            .match_indices(needle)
            .map(move |(at, _)| self.origin[at] as usize..self.origin[at + needle.len()] as usize)
    }
}

fn strip_diacritic(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        'ç' => 'c',
        'ñ' => 'n',
        other => other,
    }
}

/// A busca como ela é comparada; vazia se curta demais.
pub fn normalize_query(query: &str) -> String {
    let folded = Folded::new(query, 0).text;
    let folded = folded.trim();
    if folded.chars().count() < MIN_QUERY_CHARS {
        return String::new();
    }
    folded.to_owned()
}

pub struct IndexedChapter {
    /// Primeiro título (h1–h3) do capítulo, para rotular resultados em livros sem sumário.
    pub heading: Option<String>,
    /// Texto do capítulo, idêntico ao que o leitor monta.
    pub text: String,
    blocks: Vec<Folded>,
}

impl IndexedChapter {
    pub fn new(blocks: &[Block]) -> Self {
        let text = ChapterText::new(blocks.iter().map(Block::text));
        let folded = (0..blocks.len())
            .map(|block| {
                let range = text.block_range(block);
                Folded::new(&text.text()[range.clone()], range.start)
            })
            .collect();
        let heading = blocks.iter().find_map(|block| match block {
            Block::Heading { level, text } if *level <= 3 => Some(text.clone()),
            _ => None,
        });
        IndexedChapter {
            heading,
            text: text.text().to_owned(),
            blocks: folded,
        }
    }
}

#[derive(Default)]
pub struct BookIndex {
    pub chapters: Vec<IndexedChapter>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub chapter: usize,
    /// Trechos a marcar, em offsets do texto do capítulo (ordenados, sem sobreposição).
    pub ranges: Vec<Range<usize>>,
    /// `false` quando o parágrafo só tem todas as palavras, fora de ordem.
    pub exact: bool,
    pub snippet: String,
    /// Trechos a marcar dentro de `snippet`.
    pub snippet_ranges: Vec<Range<usize>>,
}

#[derive(Default)]
pub struct Results {
    pub hits: Vec<Hit>,
    /// Havia mais ocorrências que `MAX_HITS`.
    pub truncated: bool,
}

impl BookIndex {
    /// Lê todos os capítulos do arquivo. Feito em segundo plano: o leitor só carrega o atual.
    pub fn build(path: &Path) -> r_ereader_epub::Result<Self> {
        let mut epub = Epub::open(path)?;
        let chapters = (0..epub.chapters().len())
            .map(|index| {
                // Capítulos ilegíveis ficam vazios, como no leitor.
                let blocks = match epub.chapter_document(index) {
                    Ok(document) => preview::extract(&document, &epub.chapters()[index].path),
                    Err(_) => Vec::new(),
                };
                IndexedChapter::new(&blocks)
            })
            .collect();
        Ok(BookIndex { chapters })
    }

    pub fn search(&self, query: &str) -> Results {
        let query = normalize_query(query);
        let mut results = Results::default();
        if query.is_empty() {
            return results;
        }
        let mut words: Vec<&str> = query.split(' ').filter(|w| !w.is_empty()).collect();
        words.sort_unstable();
        words.dedup();

        let mut loose = Vec::new();
        for (chapter_index, chapter) in self.chapters.iter().enumerate() {
            for block in &chapter.blocks {
                let mut exact = false;
                for range in block.find(&query) {
                    exact = true;
                    results
                        .hits
                        .push(hit(chapter_index, chapter, block, vec![range], true));
                }
                if !exact
                    && words.len() > 1
                    && let Some(ranges) = all_words(block, &words)
                {
                    loose.push(hit(chapter_index, chapter, block, ranges, false));
                }
                if results.hits.len() + loose.len() > MAX_HITS {
                    results.truncated = true;
                    break;
                }
            }
            if results.truncated {
                break;
            }
        }
        results.hits.extend(loose);
        results.hits.truncate(MAX_HITS);
        results
    }
}

/// Todas as ocorrências de cada palavra no bloco, se nenhuma faltar.
fn all_words(block: &Folded, words: &[&str]) -> Option<Vec<Range<usize>>> {
    let mut ranges = Vec::new();
    for word in words {
        let before = ranges.len();
        ranges.extend(block.find(word));
        if ranges.len() == before {
            return None;
        }
    }
    ranges.sort_by_key(|r| r.start);
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    Some(merged)
}

fn hit(
    chapter_index: usize,
    chapter: &IndexedChapter,
    block: &Folded,
    ranges: Vec<Range<usize>>,
    exact: bool,
) -> Hit {
    let (snippet, snippet_ranges) = snippet(&chapter.text, block.range.clone(), &ranges);
    Hit {
        chapter: chapter_index,
        ranges,
        exact,
        snippet,
        snippet_ranges,
    }
}

/// Recorte do bloco em volta da primeira ocorrência, cortado em limites de palavra.
fn snippet(text: &str, block: Range<usize>, ranges: &[Range<usize>]) -> (String, Vec<Range<usize>>) {
    let first = ranges.first().cloned().unwrap_or(block.start..block.start);

    let mut from = first.start.saturating_sub(CONTEXT_BEFORE).max(block.start);
    while !text.is_char_boundary(from) {
        from += 1;
    }
    if from > block.start {
        // Começa na palavra seguinte ao corte.
        if let Some(space) = text[from..first.start].find(' ') {
            from += space + 1;
        }
    }

    let mut to = (from + SNIPPET_LENGTH).max(first.end).min(block.end);
    while !text.is_char_boundary(to) {
        to -= 1;
    }
    if to < block.end
        && let Some(space) = text[first.end..to].rfind(' ')
    {
        to = first.end + space;
    }

    let prefix = if from > block.start { ELLIPSIS } else { "" };
    let suffix = if to < block.end { ELLIPSIS } else { "" };
    let snippet = format!("{prefix}{}{suffix}", text[from..to].replace('\n', " "));
    let shift = |offset: usize| offset - from + prefix.len();
    let snippet_ranges = ranges
        .iter()
        .filter(|r| r.start < to && r.end > from)
        .map(|r| shift(r.start.max(from))..shift(r.end.min(to)))
        .collect();
    (snippet, snippet_ranges)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(chapters: &[&[&str]]) -> BookIndex {
        BookIndex {
            chapters: chapters
                .iter()
                .map(|blocks| {
                    let blocks: Vec<Block> =
                        blocks.iter().map(|t| Block::Paragraph((*t).to_owned())).collect();
                    IndexedChapter::new(&blocks)
                })
                .collect(),
        }
    }

    fn found<'a>(index: &'a BookIndex, hit: &Hit) -> Vec<&'a str> {
        let text = &index.chapters[hit.chapter].text;
        hit.ranges.iter().map(|r| &text[r.clone()]).collect()
    }

    #[test]
    fn ignores_case_accents_and_typography() {
        let index = index(&[
            &["Não sei, disse d’Ávila — “talvez”."],
            &["NAO  sabia\u{a0}nada."],
        ]);
        let results = index.search("nao");
        assert_eq!(results.hits.len(), 2);
        assert_eq!(found(&index, &results.hits[0]), ["Não"]);
        assert_eq!(found(&index, &results.hits[1]), ["NAO"]);

        let hit = &index.search("D'avila - \"talvez\"").hits[0];
        assert_eq!(found(&index, hit), ["d’Ávila — “talvez”"]);
        // Espaços repetidos e não separáveis contam como um só.
        let hit = &index.search("nao sabia nada").hits[0];
        assert_eq!(found(&index, hit), ["NAO  sabia\u{a0}nada"]);
    }

    #[test]
    fn offsets_point_into_the_chapter_text() {
        let index = index(&[&["Um", "Era uma vez, era outra vez."]]);
        let hits = index.search("vez").hits;
        assert_eq!(hits.len(), 2);
        let start = "Um\nEra uma ".len();
        assert_eq!(hits[0].ranges, vec![start..start + 3]);
        assert_eq!(found(&index, &hits[1]), ["vez"]);
    }

    #[test]
    fn exact_phrases_come_before_scattered_words() {
        let index = index(&[&["o gato preto dormia", "preto era o gato"], &["um gato preto"]]);
        let hits = index.search("gato preto").hits;
        assert_eq!(
            hits.iter().map(|h| (h.chapter, h.exact)).collect::<Vec<_>>(),
            [(0, true), (1, true), (0, false)]
        );
        assert_eq!(found(&index, &hits[2]), ["preto", "gato"]);
        // Sem todas as palavras no mesmo parágrafo, nada.
        assert!(index.search("gato azul").hits.is_empty());
    }

    #[test]
    fn short_queries_find_nothing() {
        let index = index(&[&["a b c"]]);
        assert!(index.search(" a ").hits.is_empty());
        assert_eq!(normalize_query("  É  "), "");
        assert_eq!(normalize_query(" Éle "), "ele");
    }

    #[test]
    fn caps_the_number_of_hits() {
        let paragraph = "ab ".repeat(MAX_HITS + 10);
        let index = index(&[&[&paragraph]]);
        let results = index.search("ab");
        assert_eq!(results.hits.len(), MAX_HITS);
        assert!(results.truncated);
    }

    #[test]
    fn snippets_cut_at_words_and_mark_the_match() {
        let long = format!("{} agulha {}", "palha ".repeat(30), "feno ".repeat(60));
        let index = index(&[&[&long]]);
        let hit = &index.search("AGULHA").hits[0];
        assert!(hit.snippet.starts_with("…palha"), "{}", hit.snippet);
        assert!(hit.snippet.ends_with("feno…"), "{}", hit.snippet);
        assert_eq!(&hit.snippet[hit.snippet_ranges[0].clone()], "agulha");

        let short = index_short();
        let hit = &short.search("fim").hits[0];
        assert_eq!(hit.snippet, "Era o fim.");
        assert_eq!(hit.snippet_ranges, vec![6..9]);
    }

    fn index_short() -> BookIndex {
        index(&[&["Era o fim."]])
    }
}

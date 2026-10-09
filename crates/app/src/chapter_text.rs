//! Texto corrido do capítulo e a ponte entre posições nele e nos blocos exibidos.
//!
//! Destaques e seleções vivem em offsets (bytes) do texto do capítulo: os blocos
//! concatenados, separados por `\n`. Assim um destaque pode atravessar parágrafos e
//! continua estável enquanto a lista de blocos não mudar.

use std::ops::Range;

const SEPARATOR: &str = "\n";

#[derive(Debug, Default)]
pub struct ChapterText {
    text: String,
    /// Offset inicial de cada bloco (imagens contam como blocos vazios).
    starts: Vec<usize>,
}

impl ChapterText {
    pub fn new<'a>(blocks: impl IntoIterator<Item = &'a str>) -> Self {
        let mut text = String::new();
        let mut starts = Vec::new();
        for (index, block) in blocks.into_iter().enumerate() {
            if index > 0 {
                text.push_str(SEPARATOR);
            }
            starts.push(text.len());
            text.push_str(block);
        }
        ChapterText { text, starts }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Offset no capítulo de uma posição dentro de um bloco.
    pub fn offset(&self, block: usize, index_in_block: usize) -> usize {
        let start = self.starts.get(block).copied().unwrap_or(self.text.len());
        (start + index_in_block).min(self.block_end(block))
    }

    /// Bloco que contém o offset (o separador pertence ao bloco anterior).
    pub fn block_at(&self, offset: usize) -> usize {
        self.starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1)
    }

    /// Parte de `range` que cai no bloco, em coordenadas locais do bloco.
    pub fn local_range(&self, block: usize, range: &Range<usize>) -> Option<Range<usize>> {
        let start = *self.starts.get(block)?;
        let end = self.block_end(block);
        let from = range.start.max(start);
        let to = range.end.min(end);
        (from < to).then(|| from - start..to - start)
    }

    /// Intervalo do bloco no texto do capítulo (sem o separador).
    pub fn block_range(&self, block: usize) -> Range<usize> {
        let start = self.starts.get(block).copied().unwrap_or(self.text.len());
        start..self.block_end(block)
    }

    fn block_end(&self, block: usize) -> usize {
        match self.starts.get(block + 1) {
            Some(next) => next - SEPARATOR.len(),
            None => self.text.len(),
        }
    }
}

/// Achata camadas sobrepostas em trechos disjuntos e ordenados; camadas posteriores
/// ganham das anteriores (a seleção vem por último e fica por cima dos destaques).
pub fn flatten_layers<S: Copy + PartialEq>(layers: &[(Range<usize>, S)]) -> Vec<(Range<usize>, S)> {
    let mut points: Vec<usize> = layers.iter().flat_map(|(r, _)| [r.start, r.end]).collect();
    points.sort_unstable();
    points.dedup();

    let mut out: Vec<(Range<usize>, S)> = Vec::new();
    for window in points.windows(2) {
        let (from, to) = (window[0], window[1]);
        let Some((_, style)) = layers.iter().rev().find(|(r, _)| r.start <= from && to <= r.end) else {
            continue;
        };
        match out.last_mut() {
            // Trechos vizinhos com o mesmo estilo viram um só.
            Some((previous, last)) if previous.end == from && last == style => previous.end = to,
            _ => out.push((from..to, *style)),
        }
    }
    out
}

/// Palavra sob o índice (para duplo clique); vazio se o índice estiver em pontuação/espaço.
pub fn word_at(text: &str, index: usize) -> Range<usize> {
    let is_word = |c: char| c.is_alphanumeric() || c == '-' || c == '\'' || c == '’';
    let index = index.min(text.len());
    let start = text[..index]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word(*c))
        .last()
        .map_or(index, |(i, _)| i);
    let end = text[index..]
        .char_indices()
        .find(|(_, c)| !is_word(*c))
        .map_or(text.len(), |(i, _)| index + i);
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ChapterText {
        // "Título\nEra uma vez\n\nFim" — o terceiro bloco é uma imagem (vazio).
        ChapterText::new(["Título", "Era uma vez", "", "Fim"])
    }

    #[test]
    fn maps_block_positions_to_chapter_offsets() {
        let text = sample();
        assert_eq!(text.text(), "Título\nEra uma vez\n\nFim");
        assert_eq!(text.offset(0, 0), 0);
        assert_eq!(text.offset(1, 4), "Título\n".len() + 4);
        // Índices além do fim do bloco ficam presos ao fim dele.
        assert_eq!(text.offset(1, 999), "Título\nEra uma vez".len());
        assert_eq!(&text.text()[text.offset(3, 0)..], "Fim");
    }

    #[test]
    fn finds_block_of_offset() {
        let text = sample();
        assert_eq!(text.block_at(0), 0);
        assert_eq!(text.block_at("Título".len()), 0);
        assert_eq!(text.block_at("Título\n".len()), 1);
        assert_eq!(text.block_at(text.text().len()), 3);
    }

    #[test]
    fn splits_cross_block_ranges() {
        let text = sample();
        let era = text.offset(1, 4);
        let fim = text.offset(3, 2);
        let range = era..fim;
        assert_eq!(text.local_range(0, &range), None);
        assert_eq!(text.local_range(1, &range), Some(4.."Era uma vez".len()));
        assert_eq!(text.local_range(2, &range), None);
        assert_eq!(text.local_range(3, &range), Some(0..2));
    }

    #[test]
    fn later_layers_win() {
        let layers = [(0..10, 'a'), (5..15, 'b'), (8..9, 's')];
        assert_eq!(
            flatten_layers(&layers),
            [(0..5, 'a'), (5..8, 'b'), (8..9, 's'), (9..15, 'b')]
        );
        assert!(flatten_layers::<char>(&[]).is_empty());
    }

    #[test]
    fn selects_words() {
        let text = "Era uma vez, d’Ávila";
        assert_eq!(&text[word_at(text, 5)], "uma");
        assert_eq!(&text[word_at(text, 4)], "uma");
        assert_eq!(&text[word_at(text, 0)], "Era");
        assert_eq!(&text[word_at(text, text.len())], "d’Ávila");
        assert_eq!(&text[word_at(text, 11)], "vez");
        assert_eq!(word_at(text, 12), 12..12);
    }
}

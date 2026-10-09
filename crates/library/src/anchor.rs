//! Reencontra destaques no texto de um capítulo.
//!
//! O destaque guarda o intervalo e o trecho citado. Se a extração do texto mudar
//! (nova versão do renderizador, livro editado), o intervalo antigo pode não apontar
//! mais para o mesmo trecho; aí procuramos a citação, preferindo a ocorrência mais
//! próxima da posição original.

use std::ops::Range;

/// Intervalo atual do trecho `quote`, ou `None` se ele não existe mais no texto.
pub fn resolve(text: &str, start: usize, end: usize, quote: &str) -> Option<Range<usize>> {
    if quote.is_empty() {
        return None;
    }
    if text.get(start..end) == Some(quote) {
        return Some(start..end);
    }
    text.match_indices(quote)
        .map(|(index, _)| index)
        .min_by_key(|index| index.abs_diff(start))
        .map(|index| index..index + quote.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_exact_position() {
        assert_eq!(resolve("um dois um", 8, 10, "um"), Some(8..10));
    }

    #[test]
    fn finds_nearest_occurrence_after_text_shift() {
        // O texto ganhou o prefixo "abc ": o segundo "um", antes em 8, agora está em 12.
        assert_eq!(resolve("abc um dois um", 8, 10, "x"), None);
        assert_eq!(resolve("abc um dois um", 10, 12, "um"), Some(12..14));
        assert_eq!(resolve("abc um dois um", 1, 3, "um"), Some(4..6));
    }

    #[test]
    fn handles_invalid_offsets_and_missing_quotes() {
        assert_eq!(resolve("ação", 1, 2, "çã"), Some(1..5));
        assert_eq!(resolve("texto", 0, 99, "sumiu"), None);
        assert_eq!(resolve("texto", 0, 0, ""), None);
    }
}

//! Exporta os destaques para um vault do Obsidian: uma nota por livro.
//!
//! A nota tem propriedades no topo e um trecho gerenciado entre marcadores
//! `%% r-ereader:inicio %%` e `%% r-ereader:fim %%`. Só esse trecho e as propriedades
//! do r-ereader são reescritos; o resto (anotações do usuário, propriedades extras,
//! tags editadas) é preservado. A nota é reencontrada pela propriedade
//! `r_ereader_id`, então pode ser renomeada ou movida dentro da pasta.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::library::Library;
use crate::model::{BookDetails, BookId, BookQuery, Highlight, HighlightColor, Progress};
use crate::text;

/// Configuração com a pasta das notas. Vazia = exportação desativada.
pub const FOLDER_SETTING: &str = "obsidian.pasta";
pub const DEFAULT_SUBFOLDER: &str = "Leituras";

const ID_KEY: &str = "r_ereader_id";
const START_MARKER: &str = "%% r-ereader:inicio";
const START_LINE: &str = "%% r-ereader:inicio — este trecho é reescrito pelo r-ereader; escreva fora dele %%";
const END_LINE: &str = "%% r-ereader:fim %%";
const SNIPPET_FILE: &str = "r-ereader-destaques.css";
/// Nomes de antes da troca de nome do projeto: notas antigas continuam sendo reconhecidas
/// e passam para os nomes novos na próxima exportação.
const LEGACY_ID_KEY: &str = "rereader_id";
const LEGACY_START_MARKER: &str = "%% rereader:inicio";
const LEGACY_END_LINE: &str = "%% rereader:fim %%";
const LEGACY_SNIPPET_FILE: &str = "rereader-destaques.css";

/// Propriedades que o r-ereader controla; as demais são do usuário.
const MANAGED_KEYS: &[&str] = &[
    ID_KEY,
    LEGACY_ID_KEY,
    "titulo",
    "autores",
    "serie",
    "volume",
    "isbn",
    "editora",
    "publicado",
    "idioma",
    "progresso",
    "destaques",
    "atualizado",
];

// ── Configuração ───────────────────────────────────────────────────────────

/// Pasta de exportação configurada; `None` se desativada.
pub fn folder(library: &Library) -> Result<Option<PathBuf>> {
    Ok(library
        .setting(FOLDER_SETTING)?
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from))
}

/// Na primeira execução, aponta para `<vault aberto>/Leituras` se houver um vault.
/// Depois disso respeita a escolha (inclusive desativado).
pub fn configure_default(library: &Library) -> Result<Option<PathBuf>> {
    configure_default_with(library, detect_vault)
}

/// Como [`configure_default`], com a detecção do vault injetável (testes).
pub fn configure_default_with(
    library: &Library,
    detect: impl FnOnce() -> Option<PathBuf>,
) -> Result<Option<PathBuf>> {
    if library.setting(FOLDER_SETTING)?.is_none() {
        let default = detect().map(|vault| vault.join(DEFAULT_SUBFOLDER));
        let value = default
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        library.set_setting(FOLDER_SETTING, &value)?;
    }
    folder(library)
}

/// Vault aberto mais recentemente segundo a configuração do Obsidian (nativo ou Flatpak).
pub fn detect_vault() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    [
        ".config/obsidian/obsidian.json",
        ".var/app/md.obsidian.Obsidian/config/obsidian/obsidian.json",
    ]
    .iter()
    .filter_map(|relative| fs::read_to_string(home.join(relative)).ok())
    .find_map(|json| vault_from_config(&json))
    .filter(|vault| vault.is_dir())
}

/// Escolhe o vault marcado como aberto; sem nenhum, o usado por último.
pub fn vault_from_config(json: &str) -> Option<PathBuf> {
    let config: serde_json::Value = serde_json::from_str(json).ok()?;
    let vaults = config.get("vaults")?.as_object()?;
    let entries = vaults.values().filter_map(|vault| {
        let path = vault.get("path")?.as_str()?;
        let open = vault.get("open").and_then(|v| v.as_bool()).unwrap_or(false);
        let ts = vault.get("ts").and_then(|v| v.as_i64()).unwrap_or(0);
        Some((open, ts, path))
    });
    entries
        .max_by_key(|(open, ts, _)| (*open, *ts))
        .map(|(_, _, path)| PathBuf::from(path))
}

// ── Exportação ─────────────────────────────────────────────────────────────

/// Atualiza a nota do livro na pasta configurada. `Ok(None)` quando a exportação está
/// desativada, ou quando o livro não tem destaques e ainda não tem nota.
pub fn sync_book(library: &Library, book: BookId) -> Result<Option<PathBuf>> {
    match folder(library)? {
        Some(folder) => export_book(library, book, &folder),
        None => Ok(None),
    }
}

pub fn export_book(library: &Library, book: BookId, folder: &Path) -> Result<Option<PathBuf>> {
    let details = library.book(book)?;
    let highlights = library.highlights(book)?;
    let existing = find_note(folder, book);
    if existing.is_none() && highlights.is_empty() {
        return Ok(None);
    }

    fs::create_dir_all(folder)?;
    install_snippet(folder);

    let path = existing.unwrap_or_else(|| new_note_path(folder, &details));
    let previous = fs::read_to_string(&path).unwrap_or_default();
    let progress = library.progress(book)?;
    let note = merge(
        &previous,
        &frontmatter(&details, &highlights, progress, &library.today()?),
        &body(&details, &highlights),
    );
    write_atomically(&path, &note)?;
    Ok(Some(path))
}

/// Exporta todos os livros com destaques (ou que já têm nota). Devolve quantas notas
/// foram escritas.
pub fn export_all(library: &Library, folder: &Path) -> Result<usize> {
    let mut written = 0;
    for book in library.books(&BookQuery::default())? {
        if export_book(library, book.id, folder)?.is_some() {
            written += 1;
        }
    }
    Ok(written)
}

// ── Conteúdo da nota ───────────────────────────────────────────────────────

fn frontmatter(
    details: &BookDetails,
    highlights: &[Highlight],
    progress: Option<Progress>,
    today: &str,
) -> Vec<(String, String)> {
    let summary = &details.summary;
    let mut entries = vec![
        (ID_KEY.to_owned(), summary.id.to_string()),
        ("titulo".to_owned(), yaml_string(&summary.title)),
    ];
    if !summary.authors.is_empty() {
        let list: String = summary
            .authors
            .iter()
            .map(|a| format!("\n  - {}", yaml_string(&format!("[[{a}]]"))))
            .collect();
        entries.push(("autores".to_owned(), list));
    }
    if let Some(series) = &summary.series {
        entries.push(("serie".to_owned(), yaml_string(&series.name)));
        if let Some(index) = series.index {
            entries.push(("volume".to_owned(), index.to_string()));
        }
    }
    let optional = [
        ("isbn", &details.isbn),
        ("editora", &details.publisher),
        ("publicado", &details.published),
        ("idioma", &details.language),
    ];
    for (key, value) in optional {
        if let Some(value) = value {
            entries.push((key.to_owned(), yaml_string(value)));
        }
    }
    if let Some(progress) = progress {
        entries.push(("progresso".to_owned(), format!("{:.0}", progress.fraction * 100.)));
    }
    entries.push(("destaques".to_owned(), highlights.len().to_string()));
    entries.push(("atualizado".to_owned(), today.to_owned()));
    // Só usadas ao criar a nota: depois as tags são do usuário.
    let tags: String = std::iter::once("livro".to_owned())
        .chain(details.tags.iter().map(|t| tag_slug(t)))
        .filter(|t| !t.is_empty())
        .map(|t| format!("\n  - {t}"))
        .collect();
    entries.push(("tags".to_owned(), tags));
    entries
}

fn body(details: &BookDetails, highlights: &[Highlight]) -> String {
    let summary = &details.summary;
    let mut out = String::new();
    out.push_str(START_LINE);
    out.push_str("\n# ");
    out.push_str(&summary.title);
    out.push('\n');

    let mut byline: Vec<String> = summary.authors.iter().map(|a| format!("[[{a}]]")).collect();
    if let Some(publisher) = &details.publisher {
        byline.push(publisher.clone());
    }
    if let Some(year) = details.published.as_deref().and_then(|d| d.get(..4)) {
        byline.push(year.to_owned());
    }
    if !byline.is_empty() {
        out.push('\n');
        out.push_str(&byline.join(" · "));
        out.push('\n');
    }

    if highlights.is_empty() {
        out.push_str("\n*Nenhum destaque ainda.*\n");
    }
    let mut current_chapter = None;
    for highlight in highlights {
        if current_chapter != Some(highlight.chapter) {
            current_chapter = Some(highlight.chapter);
            let title = highlight
                .chapter_title
                .clone()
                .unwrap_or_else(|| format!("Capítulo {}", highlight.chapter + 1));
            out.push_str(&format!("\n## {}\n", title.trim()));
        }
        out.push_str(&format!(
            "\n> [!quote|{}] Destaque\n",
            color_name(highlight.color)
        ));
        for (index, paragraph) in highlight
            .quote
            .split('\n')
            .filter(|p| !p.trim().is_empty())
            .enumerate()
        {
            if index > 0 {
                out.push_str(">\n");
            }
            out.push_str("> ");
            out.push_str(&escape_markdown(paragraph.trim()));
            out.push('\n');
        }
        if let Some(note) = &highlight.note {
            out.push_str(">\n> **Nota:** ");
            out.push_str(&escape_markdown(note));
            out.push('\n');
        }
        // Âncora para citar o destaque de outras notas: [[Livro#^rr-12]].
        out.push_str(&format!("\n^rr-{}\n", highlight.id));
    }
    out.push('\n');
    out.push_str(END_LINE);
    out
}

fn color_name(color: HighlightColor) -> &'static str {
    match color {
        HighlightColor::Yellow => "amarelo",
        HighlightColor::Green => "verde",
        HighlightColor::Blue => "azul",
        HighlightColor::Pink => "rosa",
    }
}

/// Junta a nota nova com a existente: propriedades do r-ereader atualizadas, as do
/// usuário mantidas; trecho gerenciado substituído, o resto intocado.
fn merge(existing: &str, managed: &[(String, String)], body: &str) -> String {
    let (old_entries, rest) = split_frontmatter(existing);

    let mut entries: Vec<(String, String)> = managed
        .iter()
        .filter(|(key, _)| key != "tags" || !old_entries.iter().any(|(k, _)| k == "tags"))
        .cloned()
        .collect();
    for (key, value) in &old_entries {
        let ours = MANAGED_KEYS.contains(&key.as_str());
        if !ours && !entries.iter().any(|(k, _)| k == key) {
            entries.push((key.clone(), value.clone()));
        }
    }

    let mut out = String::from("---\n");
    for (key, value) in &entries {
        if value.starts_with('\n') {
            out.push_str(&format!("{key}:{value}\n"));
        } else {
            out.push_str(&format!("{key}: {value}\n"));
        }
    }
    out.push_str("---\n");

    let markers = [(START_MARKER, END_LINE), (LEGACY_START_MARKER, LEGACY_END_LINE)];
    let (start, end) = markers
        .iter()
        .find_map(|(start_marker, end_line)| {
            let start = rest.find(start_marker)?;
            let end = rest[start..].find(end_line)? + start + end_line.len();
            Some((Some(start), Some(end)))
        })
        .unwrap_or((None, None));
    match (start, end) {
        (Some(start), Some(end)) => {
            out.push_str(&rest[..start]);
            out.push_str(body);
            out.push_str(&rest[end..]);
        }
        // Sem marcadores (nota nova ou marcadores apagados): o trecho gerado vai no
        // topo e o texto do usuário continua embaixo.
        _ => {
            out.push_str(body);
            out.push('\n');
            let rest = rest.trim();
            if !rest.is_empty() {
                out.push('\n');
                out.push_str(rest);
                out.push('\n');
            }
        }
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Separa as propriedades em entradas `(chave, valor bruto)`; valores de várias linhas
/// (listas) começam com `\n` e mantêm a indentação original.
fn split_frontmatter(note: &str) -> (Vec<(String, String)>, &str) {
    let Some(after_open) = note.strip_prefix("---\n") else {
        return (Vec::new(), note);
    };
    let Some(close) = after_open.find("\n---") else {
        return (Vec::new(), note);
    };
    let block = &after_open[..close];
    let rest = after_open[close + 4..]
        .strip_prefix('\n')
        .unwrap_or(&after_open[close + 4..]);

    let mut entries: Vec<(String, String)> = Vec::new();
    for line in block.lines() {
        let continuation = line.starts_with(' ') || line.starts_with('\t') || line.starts_with('-');
        match (continuation, entries.last_mut(), line.split_once(':')) {
            (true, Some((_, value)), _) => {
                value.push('\n');
                value.push_str(line);
            }
            (false, _, Some((key, value))) => {
                let value = value.trim();
                let value = if value.is_empty() {
                    String::new()
                } else {
                    value.to_owned()
                };
                entries.push((key.trim().to_owned(), value));
            }
            _ => {}
        }
    }
    (entries, rest)
}

// ── Arquivos ───────────────────────────────────────────────────────────────

/// Procura (recursivamente, até 4 níveis) a nota com `r_ereader_id: <book>`.
pub fn find_note(folder: &Path, book: BookId) -> Option<PathBuf> {
    fn walk(dir: &Path, depth: usize, needle: &str) -> Option<PathBuf> {
        let mut entries: Vec<PathBuf> = fs::read_dir(dir)
            .ok()?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        entries.sort();
        for path in &entries {
            if path.extension().is_some_and(|e| e == "md") && has_id(path, needle) {
                return Some(path.clone());
            }
        }
        if depth == 0 {
            return None;
        }
        entries
            .iter()
            .filter(|p| {
                p.is_dir()
                    && !p
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with('.'))
            })
            .find_map(|p| walk(p, depth - 1, needle))
    }
    fn has_id(path: &Path, needle: &str) -> bool {
        use std::io::Read;
        let mut head = vec![0; 4096];
        let Ok(read) = fs::File::open(path).and_then(|mut f| f.read(&mut head)) else {
            return false;
        };
        let head = String::from_utf8_lossy(&head[..read]);
        let (entries, _) = split_frontmatter(&head);
        entries
            .iter()
            .any(|(k, v)| (k == ID_KEY || k == LEGACY_ID_KEY) && v == needle)
    }
    walk(folder, 4, &book.to_string())
}

fn new_note_path(folder: &Path, details: &BookDetails) -> PathBuf {
    let summary = &details.summary;
    let name = match summary.authors.first() {
        Some(author) => format!("{} — {author}", summary.title),
        None => summary.title.clone(),
    };
    let base = text::sanitize_file_name(&name, 120);
    let path = folder.join(format!("{base}.md"));
    if path.exists() {
        // Já existe uma nota com esse nome que não é deste livro: não sobrescreve.
        folder.join(format!("{base} ({}).md", summary.id))
    } else {
        path
    }
}

fn write_atomically(path: &Path, content: &str) -> Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temporary = path.with_file_name(format!(".{name}.r-ereader-tmp"));
    fs::write(&temporary, content)?;
    fs::rename(&temporary, path)?;
    Ok(())
}

/// Grava o snippet de cores no vault (uma vez). O usuário decide se ativa.
fn install_snippet(folder: &Path) {
    let Some(vault) = folder.ancestors().find(|dir| dir.join(".obsidian").is_dir()) else {
        return;
    };
    let snippets = vault.join(".obsidian").join("snippets");
    let path = snippets.join(SNIPPET_FILE);
    if path.exists() || snippets.join(LEGACY_SNIPPET_FILE).exists() {
        return;
    }
    let css = "/* Cores dos destaques exportados pelo r-ereader.\n   Ative em Configurações → Aparência → Trechos de CSS. */\n\
.callout[data-callout=\"quote\"][data-callout-metadata~=\"amarelo\"] { --callout-color: 214, 169, 31; }\n\
.callout[data-callout=\"quote\"][data-callout-metadata~=\"verde\"] { --callout-color: 104, 155, 63; }\n\
.callout[data-callout=\"quote\"][data-callout-metadata~=\"azul\"] { --callout-color: 73, 130, 184; }\n\
.callout[data-callout=\"quote\"][data-callout-metadata~=\"rosa\"] { --callout-color: 205, 94, 120; }\n";
    // Falhar aqui só deixa os destaques sem cor; não impede a exportação.
    let _ = fs::create_dir_all(&snippets).and_then(|()| fs::write(path, css));
}

// ── Utilitários ────────────────────────────────────────────────────────────

fn yaml_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Tags do Obsidian não aceitam espaços nem pontuação: "Ficção Científica" → "ficção-científica".
fn tag_slug(tag: &str) -> String {
    tag.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_' || c == '/'))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Evita que o texto citado vire formatação: início de linha que parece título ou
/// lista, comentários `%%` (que engoliriam o resto da nota) e wikilinks.
fn escape_markdown(line: &str) -> String {
    let mut out = line.replace("%%", "\\%\\%").replace("[[", "\\[\\[");
    if out.starts_with(['#', '-', '+', '*', '>']) {
        out.insert(0, '\\');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_open_vault_then_most_recent() {
        let json = r#"{"vaults":{
            "a":{"path":"/vaults/antigo","ts":1},
            "b":{"path":"/vaults/aberto","ts":0,"open":true},
            "c":{"path":"/vaults/recente","ts":9}}}"#;
        assert_eq!(vault_from_config(json), Some(PathBuf::from("/vaults/aberto")));
        let json = r#"{"vaults":{"a":{"path":"/v/1","ts":1},"c":{"path":"/v/2","ts":9}}}"#;
        assert_eq!(vault_from_config(json), Some(PathBuf::from("/v/2")));
        assert_eq!(vault_from_config("{}"), None);
        assert_eq!(vault_from_config("lixo"), None);
    }

    #[test]
    fn slugs_tags() {
        assert_eq!(tag_slug("Ficção Científica"), "ficção-científica");
        assert_eq!(tag_slug("Religion, General"), "religion-general");
        assert_eq!(tag_slug("  "), "");
    }

    #[test]
    fn escapes_markdown_in_quotes() {
        assert_eq!(escape_markdown("# não é título"), "\\# não é título");
        assert_eq!(escape_markdown("100%% certo [[x]]"), "100\\%\\% certo \\[\\[x]]");
        assert_eq!(escape_markdown("texto normal"), "texto normal");
    }

    #[test]
    fn quotes_yaml_strings() {
        assert_eq!(yaml_string(r#"Um "livro"\ raro"#), r#""Um \"livro\"\\ raro""#);
    }

    #[test]
    fn merge_keeps_user_properties_text_and_tags() {
        let existing = "---\nr_ereader_id: 3\ntitulo: \"Velho\"\nnota: 5\ntags:\n  - livro\n  - favorito\n---\n\
Minha introdução.\n\n%% r-ereader:inicio antigo %%\nconteúdo velho\n%% r-ereader:fim %%\n\nMinhas reflexões.\n";
        let managed = vec![
            ("r_ereader_id".to_owned(), "3".to_owned()),
            ("titulo".to_owned(), "\"Novo\"".to_owned()),
            ("tags".to_owned(), "\n  - livro".to_owned()),
        ];
        let merged = merge(
            existing,
            &managed,
            "%% r-ereader:inicio %%\nconteúdo novo\n%% r-ereader:fim %%",
        );
        assert_eq!(
            merged,
            "---\nr_ereader_id: 3\ntitulo: \"Novo\"\nnota: 5\ntags:\n  - livro\n  - favorito\n---\n\
Minha introdução.\n\n%% r-ereader:inicio %%\nconteúdo novo\n%% r-ereader:fim %%\n\nMinhas reflexões.\n"
        );
    }

    #[test]
    fn merge_migrates_notes_from_before_the_rename() {
        let existing = "---\nrereader_id: 3\nnota: 5\n---\nAntes.\n\n%% rereader:inicio antigo %%\nvelho\n%% rereader:fim %%\n\nDepois.\n";
        let merged = merge(
            existing,
            &[("r_ereader_id".to_owned(), "3".to_owned())],
            "%% r-ereader:inicio %%\nnovo\n%% r-ereader:fim %%",
        );
        assert_eq!(
            merged,
            "---\nr_ereader_id: 3\nnota: 5\n---\nAntes.\n\n%% r-ereader:inicio %%\nnovo\n%% r-ereader:fim %%\n\nDepois.\n"
        );
    }

    #[test]
    fn merge_without_markers_puts_generated_part_on_top() {
        let merged = merge(
            "Texto solto do usuário.\n",
            &[("r_ereader_id".into(), "1".into())],
            "GERADO",
        );
        assert_eq!(
            merged,
            "---\nr_ereader_id: 1\n---\nGERADO\n\nTexto solto do usuário.\n"
        );
    }
}

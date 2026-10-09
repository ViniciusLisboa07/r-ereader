use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params, params_from_iter};

use crate::cover::{self, THUMBNAIL_FILE};
use crate::error::{Error, Result};
use crate::extract::{self, Extracted};
use crate::model::*;
use crate::schema;
use crate::text;

const DATABASE_FILE: &str = "metadata.db";
const UNKNOWN_AUTHOR: &str = "Autor desconhecido";
/// Separador para `group_concat`: não aparece em nomes digitados.
const SEP: char = '\u{1f}';

/// Biblioteca gerenciada: um banco SQLite e uma pasta com os arquivos organizados em
/// `Autor/Título (id)/`.
pub struct Library {
    root: PathBuf,
    conn: Connection,
}

impl Library {
    /// `$R_EREADER_LIBRARY` ou `~/Livros/r-ereader`.
    pub fn default_root() -> PathBuf {
        if let Some(path) = std::env::var_os("R_EREADER_LIBRARY") {
            return PathBuf::from(path);
        }
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
        let books = home.join("Livros");
        let root = books.join("r-ereader");
        // Antes da troca de nome a biblioteca ficava em `~/Livros/rereader`: muda a pasta
        // de lugar uma vez (os caminhos no banco são relativos à raiz).
        let legacy = books.join("rereader");
        if !root.exists() && legacy.join(DATABASE_FILE).is_file() && fs::rename(&legacy, &root).is_err() {
            return legacy;
        }
        root
    }

    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        let mut conn = Connection::open(root.join(DATABASE_FILE))?;
        schema::migrate(&mut conn)?;
        Ok(Library { root, conn })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    // ── Importação ─────────────────────────────────────────────────────────

    pub fn import(&mut self, source: &Path) -> Result<ImportOutcome> {
        let format = Format::from_path(source).ok_or_else(|| Error::Unsupported(source.to_path_buf()))?;
        let (hash, size) = hash_file(source)?;
        if let Some(id) = self
            .conn
            .query_row("SELECT book_id FROM files WHERE hash = ?1", [&hash], |r| r.get(0))
            .optional()?
        {
            return Ok(ImportOutcome::Duplicate(id));
        }

        let extracted = extract::extract(source, format)?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO books (title, sort_title, description, publisher, language, published, isbn, folder)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '')",
            params![
                extracted.title,
                text::sort_title(&extracted.title),
                extracted.description,
                extracted.publisher,
                extracted.language,
                extracted.published,
                extracted.isbn,
            ],
        )?;
        let id = tx.last_insert_rowid();
        set_authors(&tx, id, &extracted.authors)?;
        set_tags(&tx, id, &extracted.tags)?;
        set_series(&tx, id, extracted.series.as_ref())?;
        for (scheme, value) in &extracted.identifiers {
            tx.execute(
                "INSERT OR IGNORE INTO identifiers (book_id, scheme, value) VALUES (?1, ?2, ?3)",
                params![id, scheme, value],
            )?;
        }

        let folder = book_folder(&extracted.authors, &extracted.title, id);
        let file_name = book_file_name(&extracted.title, &extracted.authors, format);
        let dir = self.root.join(&folder);

        let result = copy_into_library(source, &dir, &file_name, &extracted).and_then(|has_cover| {
            tx.execute(
                "UPDATE books SET folder = ?1, has_cover = ?2 WHERE id = ?3",
                params![folder.to_string_lossy(), has_cover, id],
            )?;
            tx.execute(
                "INSERT INTO files (book_id, format, file_name, size, hash) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, format.as_str(), file_name, size as i64, hash],
            )?;
            reindex(&tx, id)?;
            tx.commit()?;
            Ok(())
        });

        if let Err(error) = result {
            let _ = fs::remove_dir_all(&dir);
            remove_if_empty(dir.parent());
            return Err(error);
        }
        Ok(ImportOutcome::Added(id))
    }

    // ── Consultas ──────────────────────────────────────────────────────────

    pub fn books(&self, query: &BookQuery) -> Result<Vec<BookSummary>> {
        let mut sql = format!("{SUMMARY_SELECT} WHERE 1 = 1");
        let mut values: Vec<Value> = Vec::new();

        if let Some(fts) = fts_query(&query.text) {
            sql.push_str(" AND b.id IN (SELECT rowid FROM search WHERE search MATCH ?)");
            values.push(Value::Text(fts));
        }
        match query.filter {
            Filter::All => {}
            Filter::Reading => sql.push_str(" AND p.fraction IS NOT NULL AND p.fraction < 0.99"),
            Filter::Author(author) => {
                sql.push_str(" AND b.id IN (SELECT book_id FROM book_authors WHERE author_id = ?)");
                values.push(Value::Integer(author));
            }
            Filter::Series(series) => {
                sql.push_str(" AND b.series_id = ?");
                values.push(Value::Integer(series));
            }
            Filter::Tag(tag) => {
                sql.push_str(" AND b.id IN (SELECT book_id FROM book_tags WHERE tag_id = ?)");
                values.push(Value::Integer(tag));
            }
            Filter::Collection(collection) => {
                sql.push_str(" AND b.id IN (SELECT book_id FROM collection_books WHERE collection_id = ?)");
                values.push(Value::Integer(collection));
            }
            Filter::Format(format) => {
                sql.push_str(" AND b.id IN (SELECT book_id FROM files WHERE format = ?)");
                values.push(Value::Text(format.as_str().to_owned()));
            }
        }
        sql.push_str(match query.sort {
            SortOrder::RecentlyAdded => " ORDER BY b.added_at DESC, b.id DESC",
            SortOrder::Title => " ORDER BY b.sort_title, b.id",
            SortOrder::Author => {
                " ORDER BY (SELECT a.sort_name FROM book_authors ba JOIN authors a ON a.id = ba.author_id
                            WHERE ba.book_id = b.id ORDER BY ba.position LIMIT 1) COLLATE NOCASE NULLS LAST,
                           b.sort_title"
            }
            SortOrder::RecentlyRead => " ORDER BY p.updated_at DESC NULLS LAST, b.added_at DESC",
        });

        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| self.summary_from_row(row))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn book(&self, id: BookId) -> Result<BookDetails> {
        let summary = self
            .conn
            .query_row(&format!("{SUMMARY_SELECT} WHERE b.id = ?1"), [id], |row| {
                self.summary_from_row(row)
            })
            .optional()?
            .ok_or(Error::NotFound(id))?;

        let (description, publisher, language, published, isbn, folder, has_cover) = self.conn.query_row(
            "SELECT description, publisher, language, published, isbn, folder, has_cover FROM books WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, bool>(6)?,
                ))
            },
        )?;
        let folder = self.root.join(folder);

        let tags = self.strings(
            "SELECT t.name FROM book_tags bt JOIN tags t ON t.id = bt.tag_id
             WHERE bt.book_id = ?1 ORDER BY t.name COLLATE NOCASE",
            id,
        )?;
        let collections = self
            .conn
            .prepare(
                "SELECT c.id, c.name FROM collection_books cb JOIN collections c ON c.id = cb.collection_id
                 WHERE cb.book_id = ?1 ORDER BY c.name COLLATE NOCASE",
            )?
            .query_map([id], |r| {
                Ok(CollectionRef {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let files = self
            .conn
            .prepare("SELECT format, file_name, size FROM files WHERE book_id = ?1 ORDER BY format")?
            .query_map([id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?
            .filter_map(|row| {
                let (format, name, size) = row.ok()?;
                Some(BookFile {
                    format: Format::parse(&format)?,
                    path: folder.join(name),
                    size: size as u64,
                })
            })
            .collect();

        Ok(BookDetails {
            summary,
            description,
            publisher,
            language,
            published,
            isbn,
            tags,
            collections,
            cover: has_cover.then(|| folder.join(cover::COVER_FILE)),
            files,
            folder,
        })
    }

    /// Caminho do arquivo do livro no formato pedido.
    /// Livro da biblioteca com exatamente este conteúdo (mesmo hash), se houver.
    pub fn find_by_content(&self, path: &Path) -> Result<Option<BookId>> {
        let (hash, _) = hash_file(path)?;
        Ok(self
            .conn
            .query_row("SELECT book_id FROM files WHERE hash = ?1", [&hash], |r| r.get(0))
            .optional()?)
    }

    pub fn file_path(&self, id: BookId, format: Format) -> Result<Option<PathBuf>> {
        Ok(self
            .conn
            .query_row(
                "SELECT b.folder, f.file_name FROM files f JOIN books b ON b.id = f.book_id
                 WHERE f.book_id = ?1 AND f.format = ?2",
                params![id, format.as_str()],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
            .map(|(folder, name)| self.root.join(folder).join(name)))
    }

    pub fn book_count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT count(*) FROM books", [], |r| r.get(0))?)
    }

    pub fn reading_count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT count(*) FROM progress WHERE fraction < 0.99", [], |r| {
                r.get(0)
            })?)
    }

    pub fn authors(&self) -> Result<Vec<Facet>> {
        self.facets(
            "SELECT a.id, a.name, count(*) FROM authors a JOIN book_authors ba ON ba.author_id = a.id
             GROUP BY a.id ORDER BY a.sort_name COLLATE NOCASE",
        )
    }

    pub fn series(&self) -> Result<Vec<Facet>> {
        self.facets(
            "SELECT s.id, s.name, count(*) FROM series s JOIN books b ON b.series_id = s.id
             GROUP BY s.id ORDER BY s.name COLLATE NOCASE",
        )
    }

    pub fn tags(&self) -> Result<Vec<Facet>> {
        self.facets(
            "SELECT t.id, t.name, count(*) FROM tags t JOIN book_tags bt ON bt.tag_id = t.id
             GROUP BY t.id ORDER BY t.name COLLATE NOCASE",
        )
    }

    /// Todas as coleções, inclusive as vazias.
    pub fn collections(&self) -> Result<Vec<Facet>> {
        self.facets(
            "SELECT c.id, c.name, count(cb.book_id) FROM collections c
             LEFT JOIN collection_books cb ON cb.collection_id = c.id
             GROUP BY c.id ORDER BY c.name COLLATE NOCASE",
        )
    }

    pub fn formats(&self) -> Result<Vec<(Format, i64)>> {
        let mut statement = self
            .conn
            .prepare("SELECT format, count(*) FROM files GROUP BY format ORDER BY format")?;
        let rows = statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        Ok(rows
            .filter_map(|row| {
                let (format, count) = row.ok()?;
                Some((Format::parse(&format)?, count))
            })
            .collect())
    }

    // ── Edição ─────────────────────────────────────────────────────────────

    /// Atualiza os metadados e, se título ou autor principal mudarem, move a pasta
    /// e renomeia os arquivos (como o Calibre faz).
    pub fn update_metadata(&mut self, id: BookId, edit: &MetadataEdit) -> Result<()> {
        let title = edit.title.trim();
        if title.is_empty() {
            return Err(Error::InvalidName("o título não pode ficar vazio".into()));
        }
        let authors: Vec<String> = edit
            .authors
            .iter()
            .map(|a| text::normalize_author(a))
            .filter(|a| !a.is_empty())
            .collect();
        let old_folder: String = self
            .conn
            .query_row("SELECT folder FROM books WHERE id = ?1", [id], |r| r.get(0))
            .optional()?
            .ok_or(Error::NotFound(id))?;

        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE books SET title = ?1, sort_title = ?2, description = ?3, publisher = ?4, language = ?5,
                              published = ?6, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?7",
            params![
                title,
                text::sort_title(title),
                non_empty(&edit.description),
                non_empty(&edit.publisher),
                non_empty(&edit.language),
                non_empty(&edit.published),
                id
            ],
        )?;
        set_authors(&tx, id, &authors)?;
        set_tags(&tx, id, &edit.tags)?;
        let series = non_empty(&edit.series).map(|name| SeriesRef {
            name,
            index: edit.series_index,
        });
        set_series(&tx, id, series.as_ref())?;
        remove_orphans(&tx)?;
        reindex(&tx, id)?;

        let new_folder = book_folder(&authors, title, id);
        let old_dir = self.root.join(&old_folder);
        let new_dir = self.root.join(&new_folder);
        let mut renames: Vec<(PathBuf, PathBuf)> = Vec::new();

        let result = (|| -> Result<()> {
            if old_dir != new_dir {
                if let Some(parent) = new_dir.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::rename(&old_dir, &new_dir)?;
                renames.push((new_dir.clone(), old_dir.clone()));
                tx.execute(
                    "UPDATE books SET folder = ?1 WHERE id = ?2",
                    params![new_folder.to_string_lossy(), id],
                )?;
            }
            let files: Vec<(i64, String, String)> = tx
                .prepare("SELECT id, format, file_name FROM files WHERE book_id = ?1")?
                .query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<_>>()?;
            for (file_id, format, old_name) in files {
                let Some(format) = Format::parse(&format) else {
                    continue;
                };
                let new_name = book_file_name(title, &authors, format);
                if new_name != old_name {
                    fs::rename(new_dir.join(&old_name), new_dir.join(&new_name))?;
                    renames.push((new_dir.join(&new_name), new_dir.join(&old_name)));
                    tx.execute(
                        "UPDATE files SET file_name = ?1 WHERE id = ?2",
                        params![new_name, file_id],
                    )?;
                }
            }
            Ok(())
        })()
        .and_then(|()| Ok(tx.commit()?));

        if let Err(error) = result {
            // Desfaz as renomeações na ordem inversa; o banco já foi revertido.
            for (from, to) in renames.into_iter().rev() {
                let _ = fs::rename(from, to);
            }
            remove_if_empty(new_dir.parent());
            return Err(error);
        }
        remove_if_empty(old_dir.parent());
        Ok(())
    }

    pub fn delete(&mut self, id: BookId) -> Result<()> {
        let folder: String = self
            .conn
            .query_row("SELECT folder FROM books WHERE id = ?1", [id], |r| r.get(0))
            .optional()?
            .ok_or(Error::NotFound(id))?;

        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM books WHERE id = ?1", [id])?;
        tx.execute("DELETE FROM search WHERE rowid = ?1", [id])?;
        remove_orphans(&tx)?;
        tx.commit()?;

        let dir = self.root.join(folder);
        if dir.starts_with(&self.root) && dir != self.root {
            fs::remove_dir_all(&dir)?;
            remove_if_empty(dir.parent());
        }
        Ok(())
    }

    // ── Coleções ───────────────────────────────────────────────────────────

    pub fn create_collection(&self, name: &str) -> Result<i64> {
        let name = valid_name(name)?;
        let inserted = self
            .conn
            .execute("INSERT OR IGNORE INTO collections (name) VALUES (?1)", [&name])?;
        if inserted == 0 {
            return Err(Error::InvalidName(format!(
                "já existe uma coleção chamada \"{name}\""
            )));
        }
        Ok(self.conn.last_insert_rowid())
    }

    pub fn rename_collection(&self, id: i64, name: &str) -> Result<()> {
        let name = valid_name(name)?;
        let taken: bool = self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM collections WHERE name = ?1 AND id != ?2)",
            params![name, id],
            |r| r.get(0),
        )?;
        if taken {
            return Err(Error::InvalidName(format!(
                "já existe uma coleção chamada \"{name}\""
            )));
        }
        self.conn.execute(
            "UPDATE collections SET name = ?1 WHERE id = ?2",
            params![name, id],
        )?;
        Ok(())
    }

    pub fn delete_collection(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM collections WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn add_to_collection(&self, collection: i64, book: BookId) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO collection_books (collection_id, book_id) VALUES (?1, ?2)",
            params![collection, book],
        )?;
        Ok(())
    }

    pub fn remove_from_collection(&self, collection: i64, book: BookId) -> Result<()> {
        self.conn.execute(
            "DELETE FROM collection_books WHERE collection_id = ?1 AND book_id = ?2",
            params![collection, book],
        )?;
        Ok(())
    }

    // ── Progresso ──────────────────────────────────────────────────────────

    pub fn save_progress(&self, id: BookId, progress: Progress) -> Result<()> {
        self.conn.execute(
            "INSERT INTO progress (book_id, chapter, fraction) VALUES (?1, ?2, ?3)
             ON CONFLICT (book_id) DO UPDATE SET
                 chapter = excluded.chapter,
                 fraction = excluded.fraction,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![
                id,
                progress.chapter as i64,
                progress.fraction.clamp(0., 1.) as f64
            ],
        )?;
        Ok(())
    }

    pub fn progress(&self, id: BookId) -> Result<Option<Progress>> {
        Ok(self
            .conn
            .query_row(
                "SELECT chapter, fraction FROM progress WHERE book_id = ?1",
                [id],
                |r| {
                    Ok(Progress {
                        chapter: r.get::<_, i64>(0)? as usize,
                        fraction: r.get::<_, f64>(1)? as f32,
                    })
                },
            )
            .optional()?)
    }

    // ── Destaques ──────────────────────────────────────────────────────────

    pub fn add_highlight(&self, book: BookId, highlight: &NewHighlight) -> Result<i64> {
        if highlight.end <= highlight.start || highlight.quote.trim().is_empty() {
            return Err(Error::InvalidName("o destaque precisa de algum texto".into()));
        }
        self.conn.execute(
            "INSERT INTO highlights (book_id, chapter, chapter_title, start, end, quote, color)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                book,
                highlight.chapter as i64,
                highlight.chapter_title,
                highlight.start as i64,
                highlight.end as i64,
                highlight.quote,
                highlight.color.as_str()
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Destaques do livro na ordem de leitura.
    pub fn highlights(&self, book: BookId) -> Result<Vec<Highlight>> {
        let mut statement = self.conn.prepare(
            "SELECT id, book_id, chapter, start, end, quote, color, note, created_at, chapter_title
               FROM highlights WHERE book_id = ?1 ORDER BY chapter, start, id",
        )?;
        let rows = statement.query_map([book], |r| {
            Ok(Highlight {
                id: r.get(0)?,
                book_id: r.get(1)?,
                chapter: r.get::<_, i64>(2)? as usize,
                start: r.get::<_, i64>(3)? as usize,
                end: r.get::<_, i64>(4)? as usize,
                quote: r.get(5)?,
                color: HighlightColor::parse(&r.get::<_, String>(6)?),
                note: r.get(7)?,
                created_at: r.get(8)?,
                chapter_title: r.get(9)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn set_highlight_color(&self, id: i64, color: HighlightColor) -> Result<()> {
        self.touch_highlight(id, "color = ?1", color.as_str())
    }

    /// Nota vazia (ou só espaços) remove a nota.
    pub fn set_highlight_note(&self, id: i64, note: &str) -> Result<()> {
        let note = Some(note.trim().to_owned()).filter(|n| !n.is_empty());
        self.touch_highlight(id, "note = ?1", note)
    }

    /// Atualiza a posição depois que o trecho foi reencontrado em outro lugar do capítulo.
    pub fn move_highlight(&self, id: i64, start: usize, end: usize) -> Result<()> {
        self.conn.execute(
            "UPDATE highlights SET start = ?1, end = ?2 WHERE id = ?3",
            params![start as i64, end as i64, id],
        )?;
        Ok(())
    }

    pub fn delete_highlight(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM highlights WHERE id = ?1", [id])?;
        Ok(())
    }

    fn touch_highlight(&self, id: i64, assignment: &str, value: impl rusqlite::ToSql) -> Result<()> {
        let changed = self.conn.execute(
            &format!(
                "UPDATE highlights SET {assignment}, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?2"
            ),
            params![value, id],
        )?;
        if changed == 0 {
            return Err(Error::NotFound(id));
        }
        Ok(())
    }

    // ── Configurações ──────────────────────────────────────────────────────

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Data local de hoje (AAAA-MM-DD), pelo relógio do SQLite.
    pub(crate) fn today(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row("SELECT date('now', 'localtime')", [], |r| r.get(0))?)
    }

    // ── Internos ───────────────────────────────────────────────────────────

    fn summary_from_row(&self, row: &Row<'_>) -> rusqlite::Result<BookSummary> {
        let folder: String = row.get("folder")?;
        let has_cover: bool = row.get("has_cover")?;
        let series_name: Option<String> = row.get("series_name")?;
        Ok(BookSummary {
            id: row.get("id")?,
            title: row.get("title")?,
            authors: split_concat(row.get("authors")?),
            series: series_name.map(|name| SeriesRef {
                name,
                index: row.get("series_index").ok().flatten(),
            }),
            formats: split_concat(row.get("formats")?)
                .iter()
                .filter_map(|f| Format::parse(f))
                .collect(),
            thumbnail: has_cover.then(|| self.root.join(folder).join(THUMBNAIL_FILE)),
            progress: row.get::<_, Option<f64>>("fraction")?.map(|f| f as f32),
            added_at: row.get("added_at")?,
        })
    }

    fn facets(&self, sql: &str) -> Result<Vec<Facet>> {
        let mut statement = self.conn.prepare(sql)?;
        let rows = statement.query_map([], |r| {
            Ok(Facet {
                id: r.get(0)?,
                name: r.get(1)?,
                count: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn strings(&self, sql: &str, id: BookId) -> Result<Vec<String>> {
        let mut statement = self.conn.prepare(sql)?;
        let rows = statement.query_map([id], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

const SUMMARY_SELECT: &str = "
    SELECT b.id, b.title, b.folder, b.has_cover, b.series_index, b.added_at,
           s.name AS series_name, p.fraction,
           (SELECT group_concat(a.name, char(31) ORDER BY ba.position)
              FROM book_authors ba JOIN authors a ON a.id = ba.author_id
             WHERE ba.book_id = b.id) AS authors,
           (SELECT group_concat(f.format, char(31) ORDER BY f.format)
              FROM files f WHERE f.book_id = b.id) AS formats
      FROM books b
      LEFT JOIN series s ON s.id = b.series_id
      LEFT JOIN progress p ON p.book_id = b.id";

fn split_concat(value: Option<String>) -> Vec<String> {
    value
        .map(|v| v.split(SEP).map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Cada palavra vira um prefixo entre aspas: `absal fau` → `"absal"* "fau"*`.
fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        .map(|term| term.replace('"', ""))
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{term}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

fn set_authors(tx: &Transaction<'_>, id: BookId, names: &[String]) -> Result<()> {
    tx.execute("DELETE FROM book_authors WHERE book_id = ?1", [id])?;
    for (position, name) in names
        .iter()
        .map(|n| n.trim())
        .filter(|n| !n.is_empty())
        .enumerate()
    {
        tx.execute(
            "INSERT INTO authors (name, sort_name) VALUES (?1, ?2) ON CONFLICT (name) DO NOTHING",
            params![name, text::author_sort_name(name)],
        )?;
        let author: i64 = tx.query_row("SELECT id FROM authors WHERE name = ?1", [name], |r| r.get(0))?;
        tx.execute(
            "INSERT OR IGNORE INTO book_authors (book_id, author_id, position) VALUES (?1, ?2, ?3)",
            params![id, author, position as i64],
        )?;
    }
    Ok(())
}

fn set_tags(tx: &Transaction<'_>, id: BookId, names: &[String]) -> Result<()> {
    tx.execute("DELETE FROM book_tags WHERE book_id = ?1", [id])?;
    for name in names.iter().map(|n| n.trim()).filter(|n| !n.is_empty()) {
        tx.execute(
            "INSERT INTO tags (name) VALUES (?1) ON CONFLICT (name) DO NOTHING",
            [name],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO book_tags (book_id, tag_id) SELECT ?1, id FROM tags WHERE name = ?2",
            params![id, name],
        )?;
    }
    Ok(())
}

fn set_series(tx: &Transaction<'_>, id: BookId, series: Option<&SeriesRef>) -> Result<()> {
    let Some(series) = series.filter(|s| !s.name.trim().is_empty()) else {
        tx.execute(
            "UPDATE books SET series_id = NULL, series_index = NULL WHERE id = ?1",
            [id],
        )?;
        return Ok(());
    };
    let name = series.name.trim();
    tx.execute(
        "INSERT INTO series (name) VALUES (?1) ON CONFLICT (name) DO NOTHING",
        [name],
    )?;
    tx.execute(
        "UPDATE books SET series_id = (SELECT id FROM series WHERE name = ?1), series_index = ?2 WHERE id = ?3",
        params![name, series.index, id],
    )?;
    Ok(())
}

fn remove_orphans(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "DELETE FROM authors WHERE id NOT IN (SELECT author_id FROM book_authors);
         DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM book_tags);
         DELETE FROM series WHERE id NOT IN (SELECT series_id FROM books WHERE series_id IS NOT NULL);",
    )?;
    Ok(())
}

fn reindex(tx: &Transaction<'_>, id: BookId) -> Result<()> {
    tx.execute("DELETE FROM search WHERE rowid = ?1", [id])?;
    tx.execute(
        "INSERT INTO search (rowid, title, authors, series, tags, description)
         SELECT b.id, b.title,
                coalesce((SELECT group_concat(a.name, ' ') FROM book_authors ba
                          JOIN authors a ON a.id = ba.author_id WHERE ba.book_id = b.id), ''),
                coalesce(s.name, ''),
                coalesce((SELECT group_concat(t.name, ' ') FROM book_tags bt
                          JOIN tags t ON t.id = bt.tag_id WHERE bt.book_id = b.id), ''),
                coalesce(b.description, '')
           FROM books b LEFT JOIN series s ON s.id = b.series_id
          WHERE b.id = ?1",
        [id],
    )?;
    Ok(())
}

fn copy_into_library(source: &Path, dir: &Path, file_name: &str, extracted: &Extracted) -> Result<bool> {
    fs::create_dir_all(dir)?;
    fs::copy(source, dir.join(file_name))?;
    match &extracted.cover {
        Some(bytes) => cover::write_covers(bytes, dir),
        None => Ok(false),
    }
}

fn book_folder(authors: &[String], title: &str, id: BookId) -> PathBuf {
    let author = authors.first().map_or(UNKNOWN_AUTHOR, String::as_str);
    PathBuf::from(text::sanitize_file_name(author, 60))
        .join(format!("{} ({id})", text::sanitize_file_name(title, 80)))
}

fn book_file_name(title: &str, authors: &[String], format: Format) -> String {
    let author = authors.first().map_or(UNKNOWN_AUTHOR, String::as_str);
    format!(
        "{}.{}",
        text::sanitize_file_name(&format!("{title} - {author}"), 120),
        format.extension()
    )
}

fn hash_file(path: &Path) -> Result<(String, u64)> {
    let mut file = fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0; 64 * 1024];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size += read as u64;
        hasher.update(&buffer[..read]);
    }
    Ok((hasher.finalize().to_hex().to_string(), size))
}

fn remove_if_empty(dir: Option<&Path>) {
    if let Some(dir) = dir {
        // Só remove se estiver vazia; erro aqui é esperado e ignorado.
        let _ = fs::remove_dir(dir);
    }
}

fn non_empty(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

fn valid_name(name: &str) -> Result<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(Error::InvalidName("o nome não pode ficar vazio".into()));
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_prefix_fts_queries() {
        assert_eq!(fts_query("  absal  fau "), Some("\"absal\"* \"fau\"*".into()));
        assert_eq!(fts_query("\"\""), None);
        assert_eq!(fts_query(""), None);
    }

    #[test]
    fn builds_folder_and_file_names() {
        let authors = vec!["William Faulkner".to_owned()];
        assert_eq!(
            book_folder(&authors, "Absalão, Absalão!", 7),
            PathBuf::from("William Faulkner/Absalão, Absalão! (7)")
        );
        assert_eq!(
            book_file_name("Absalão: Absalão!", &[], Format::Pdf),
            "Absalão_ Absalão! - Autor desconhecido.pdf"
        );
    }
}

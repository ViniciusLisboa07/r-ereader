//! Migrações do banco, versionadas por `PRAGMA user_version`.

use rusqlite::Connection;

use crate::error::Result;

const MIGRATIONS: &[&str] = &[
    r#"
CREATE TABLE series (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE
);

CREATE TABLE books (
    id           INTEGER PRIMARY KEY,
    title        TEXT NOT NULL,
    sort_title   TEXT NOT NULL,
    description  TEXT,
    publisher    TEXT,
    language     TEXT,
    published    TEXT,
    isbn         TEXT,
    series_id    INTEGER REFERENCES series(id) ON DELETE SET NULL,
    series_index REAL,
    -- Pasta do livro, relativa à raiz da biblioteca.
    folder       TEXT NOT NULL,
    has_cover    INTEGER NOT NULL DEFAULT 0,
    added_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE authors (
    id        INTEGER PRIMARY KEY,
    name      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    sort_name TEXT NOT NULL
);

CREATE TABLE book_authors (
    book_id   INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    author_id INTEGER NOT NULL REFERENCES authors(id) ON DELETE CASCADE,
    position  INTEGER NOT NULL,
    PRIMARY KEY (book_id, author_id)
);
CREATE INDEX book_authors_author ON book_authors(author_id);

CREATE TABLE tags (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE
);

CREATE TABLE book_tags (
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    tag_id  INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (book_id, tag_id)
);
CREATE INDEX book_tags_tag ON book_tags(tag_id);

CREATE TABLE collections (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE COLLATE NOCASE,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE collection_books (
    collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    book_id       INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    added_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (collection_id, book_id)
);
CREATE INDEX collection_books_book ON collection_books(book_id);

CREATE TABLE files (
    id        INTEGER PRIMARY KEY,
    book_id   INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    format    TEXT NOT NULL,
    file_name TEXT NOT NULL,
    size      INTEGER NOT NULL,
    hash      TEXT NOT NULL UNIQUE,
    UNIQUE (book_id, format)
);

CREATE TABLE identifiers (
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    scheme  TEXT NOT NULL,
    value   TEXT NOT NULL,
    PRIMARY KEY (book_id, scheme)
);

CREATE TABLE progress (
    book_id    INTEGER PRIMARY KEY REFERENCES books(id) ON DELETE CASCADE,
    chapter    INTEGER NOT NULL,
    fraction   REAL NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- rowid = books.id. `remove_diacritics` faz "absalao" achar "Absalão".
CREATE VIRTUAL TABLE search USING fts5(
    title, authors, series, tags, description,
    tokenize = 'unicode61 remove_diacritics 2'
);
"#,
    r#"
-- Destaques: a posição é um intervalo de bytes no texto do capítulo, e o trecho
-- citado permite reencontrá-lo se a extração de texto mudar.
CREATE TABLE highlights (
    id         INTEGER PRIMARY KEY,
    book_id    INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    chapter    INTEGER NOT NULL,
    start      INTEGER NOT NULL,
    end        INTEGER NOT NULL,
    quote      TEXT NOT NULL,
    color      TEXT NOT NULL,
    note       TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX highlights_book ON highlights(book_id, chapter, start);
"#,
    r#"
-- Título do capítulo no momento do destaque, para exportar sem abrir o EPUB.
ALTER TABLE highlights ADD COLUMN chapter_title TEXT;

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#,
];

pub fn migrate(conn: &mut Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;

    let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    for (index, migration) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(migration)?;
        tx.pragma_update(None, "user_version", index as i64 + 1)?;
        tx.commit()?;
    }
    Ok(())
}

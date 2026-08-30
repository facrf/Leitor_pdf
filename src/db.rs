use std::{path::Path, sync::Arc};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use tokio::sync::Mutex;

use crate::{
    error::{AppError, AppResult},
    models::{
        Book, BookSummary, Note, Provider, ReadingProgress, SaveNote, SaveProgress, SaveProvider,
        Share, UpdateBook,
    },
    scanner::ScannedBook,
};

#[derive(Clone)]
pub struct Database {
    connection: Arc<Mutex<Connection>>,
}

impl Database {
    pub async fn open(path: &Path, initial_library_root: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.execute_batch(SCHEMA)?;
        connection.execute(
            "INSERT OR IGNORE INTO settings (key, value) VALUES ('library_root', ?1)",
            [initial_library_root.to_string_lossy().as_ref()],
        )?;
        connection.execute(
            "INSERT OR IGNORE INTO settings (key, value) VALUES ('network_metadata_enabled', 'false')",
            [],
        )?;
        connection.execute(
            "INSERT OR IGNORE INTO metadata_providers (id, name, kind, base_url, enabled, builtin)
             VALUES (1, 'Open Library', 'open_library', 'https://openlibrary.org', 1, 1)",
            [],
        )?;
        connection.execute(
            "INSERT OR IGNORE INTO metadata_providers (id, name, kind, base_url, enabled, builtin)
             VALUES (2, 'Google Books', 'google_books', 'https://www.googleapis.com/books/v1', 1, 1)",
            [],
        )?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub async fn setting(&self, key: &str) -> AppResult<Option<String>> {
        let conn = self.connection.lock().await;
        Ok(conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> AppResult<()> {
        let conn = self.connection.lock().await;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub async fn sync_scan(&self, books: &[ScannedBook]) -> AppResult<(usize, usize)> {
        let mut conn = self.connection.lock().await;
        let tx = conn.transaction()?;
        tx.execute("UPDATE books SET is_available = 0", [])?;
        let mut added = 0;
        let mut updated = 0;
        for book in books {
            if upsert_scanned(&tx, book)? {
                added += 1;
            } else {
                updated += 1;
            }
        }
        tx.commit()?;
        Ok((added, updated))
    }

    pub async fn list_books(&self, search: Option<&str>) -> AppResult<Vec<BookSummary>> {
        let conn = self.connection.lock().await;
        let search = format!("%{}%", search.unwrap_or_default().trim());
        let mut stmt = conn.prepare(
            "SELECT b.id, b.title, b.author, b.filename, b.format, b.size, b.is_available,
                    b.cover_filename IS NOT NULL, COALESCE(p.percent, 0), b.updated_at
             FROM books b LEFT JOIN reading_progress p ON p.book_id = b.id
             WHERE (?1 = '%%' OR b.title LIKE ?1 OR COALESCE(b.author, '') LIKE ?1 OR b.filename LIKE ?1)
             ORDER BY b.title COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([search], |row| {
            Ok(BookSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                author: row.get(2)?,
                filename: row.get(3)?,
                format: row.get(4)?,
                size: row.get(5)?,
                is_available: row.get(6)?,
                has_cover: row.get(7)?,
                progress_percent: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub async fn book(&self, id: i64) -> AppResult<Book> {
        let conn = self.connection.lock().await;
        let mut book = conn
            .query_row(
                "SELECT id, title, author, description, publisher, published_date, isbn, language,
                        subjects, filename, relative_path, format, size, modified_at, page_count,
                        is_available, cover_filename IS NOT NULL, created_at, updated_at
                 FROM books WHERE id = ?1",
                [id],
                row_to_book,
            )
            .optional()?
            .ok_or(AppError::NotFound)?;
        book.progress = conn
            .query_row(
                "SELECT location, percent, updated_at FROM reading_progress WHERE book_id = ?1",
                [id],
                |row| {
                    let location: String = row.get(0)?;
                    Ok(ReadingProgress {
                        location: serde_json::from_str(&location).unwrap_or_default(),
                        percent: row.get(1)?,
                        updated_at: row.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(book)
    }

    pub async fn cover_filename(&self, id: i64) -> AppResult<String> {
        let conn = self.connection.lock().await;
        conn.query_row(
            "SELECT cover_filename FROM books WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(AppError::NotFound)
    }

    pub async fn update_book(&self, id: i64, update: &UpdateBook) -> AppResult<()> {
        if update.title.trim().is_empty() {
            return Err(AppError::BadRequest("o titulo e obrigatorio".into()));
        }
        let conn = self.connection.lock().await;
        let changed = conn.execute(
            "UPDATE books SET title=?2, author=?3, description=?4, publisher=?5,
             published_date=?6, isbn=?7, language=?8, subjects=?9, updated_at=?10 WHERE id=?1",
            params![
                id,
                update.title.trim(),
                clean(&update.author),
                clean(&update.description),
                clean(&update.publisher),
                clean(&update.published_date),
                clean(&update.isbn),
                clean(&update.language),
                serde_json::to_string(&update.subjects).unwrap_or_else(|_| "[]".into()),
                Utc::now().to_rfc3339(),
            ],
        )?;
        if changed == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    pub async fn set_cover(&self, id: i64, filename: &str) -> AppResult<()> {
        let conn = self.connection.lock().await;
        conn.execute(
            "UPDATE books SET cover_filename=?2, updated_at=?3 WHERE id=?1",
            params![id, filename, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub async fn save_progress(&self, book_id: i64, progress: &SaveProgress) -> AppResult<()> {
        if !(0.0..=100.0).contains(&progress.percent) {
            return Err(AppError::BadRequest(
                "o progresso deve estar entre 0 e 100".into(),
            ));
        }
        let conn = self.connection.lock().await;
        conn.execute(
            "INSERT INTO reading_progress (book_id, location, percent, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(book_id) DO UPDATE SET location=excluded.location,
             percent=excluded.percent, updated_at=excluded.updated_at",
            params![book_id, progress.location.to_string(), progress.percent, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub async fn notes(&self, book_id: i64) -> AppResult<Vec<Note>> {
        let conn = self.connection.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, book_id, location, content, created_at, updated_at
             FROM notes WHERE book_id=?1 ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([book_id], row_to_note)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub async fn create_note(&self, book_id: i64, note: &SaveNote) -> AppResult<Note> {
        if note.content.trim().is_empty() {
            return Err(AppError::BadRequest(
                "a anotacao nao pode ficar vazia".into(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let conn = self.connection.lock().await;
        conn.execute(
            "INSERT INTO notes (book_id, location, content, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![book_id, note.location.to_string(), note.content.trim(), now],
        )?;
        let id = conn.last_insert_rowid();
        Ok(Note {
            id,
            book_id,
            location: note.location.clone(),
            content: note.content.trim().into(),
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub async fn delete_note(&self, id: i64) -> AppResult<()> {
        let conn = self.connection.lock().await;
        if conn.execute("DELETE FROM notes WHERE id=?1", [id])? == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    pub async fn providers(&self) -> AppResult<Vec<Provider>> {
        let conn = self.connection.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, base_url, enabled, builtin FROM metadata_providers ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Provider {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                base_url: row.get(3)?,
                enabled: row.get(4)?,
                builtin: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub async fn add_provider(&self, provider: &SaveProvider) -> AppResult<Provider> {
        validate_provider(provider)?;
        let conn = self.connection.lock().await;
        conn.execute(
            "INSERT INTO metadata_providers (name, kind, base_url, enabled, builtin)
             VALUES (?1, ?2, ?3, ?4, 0)",
            params![
                provider.name.trim(),
                provider.kind,
                provider.base_url.trim_end_matches('/'),
                provider.enabled
            ],
        )?;
        Ok(Provider {
            id: conn.last_insert_rowid(),
            name: provider.name.trim().into(),
            kind: provider.kind.clone(),
            base_url: provider.base_url.trim_end_matches('/').into(),
            enabled: provider.enabled,
            builtin: false,
        })
    }

    pub async fn delete_provider(&self, id: i64) -> AppResult<()> {
        let conn = self.connection.lock().await;
        if conn.execute(
            "DELETE FROM metadata_providers WHERE id=?1 AND builtin=0",
            [id],
        )? == 0
        {
            return Err(AppError::BadRequest(
                "fontes nativas nao podem ser removidas".into(),
            ));
        }
        Ok(())
    }

    pub async fn create_share(
        &self,
        book_id: i64,
        token: &str,
        expires_at: Option<&str>,
    ) -> AppResult<Share> {
        let now = Utc::now().to_rfc3339();
        let conn = self.connection.lock().await;
        conn.execute(
            "INSERT INTO shares (book_id, token, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
            params![book_id, token, now, expires_at],
        )?;
        Ok(Share {
            id: conn.last_insert_rowid(),
            book_id,
            token: token.into(),
            created_at: now,
            expires_at: expires_at.map(str::to_string),
            revoked_at: None,
        })
    }

    pub async fn shares(&self, book_id: i64) -> AppResult<Vec<Share>> {
        let conn = self.connection.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, book_id, token, created_at, expires_at, revoked_at FROM shares
             WHERE book_id=?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([book_id], |row| {
            Ok(Share {
                id: row.get(0)?,
                book_id: row.get(1)?,
                token: row.get(2)?,
                created_at: row.get(3)?,
                expires_at: row.get(4)?,
                revoked_at: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub async fn revoke_share(&self, id: i64) -> AppResult<()> {
        let conn = self.connection.lock().await;
        if conn.execute(
            "UPDATE shares SET revoked_at=?2 WHERE id=?1",
            params![id, Utc::now().to_rfc3339()],
        )? == 0
        {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    pub async fn shared_book_path(&self, token: &str) -> AppResult<(i64, String)> {
        let conn = self.connection.lock().await;
        conn.query_row(
            "SELECT b.id, b.relative_path FROM shares s JOIN books b ON b.id=s.book_id
             WHERE s.token=?1 AND s.revoked_at IS NULL
             AND (s.expires_at IS NULL OR s.expires_at > ?2) AND b.is_available=1",
            params![token, Utc::now().to_rfc3339()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)
    }
}

fn upsert_scanned(tx: &Transaction<'_>, book: &ScannedBook) -> rusqlite::Result<bool> {
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM books WHERE relative_path=?1)",
        [&book.relative_path],
        |row| row.get(0),
    )?;
    let now = Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO books (title, author, filename, relative_path, format, size, modified_at,
          page_count, is_available, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?9)
         ON CONFLICT(relative_path) DO UPDATE SET filename=excluded.filename, format=excluded.format,
          size=excluded.size, modified_at=excluded.modified_at, page_count=COALESCE(excluded.page_count, books.page_count),
          is_available=1, updated_at=excluded.updated_at",
        params![book.title, book.author, book.filename, book.relative_path, book.format,
                book.size, book.modified_at, book.page_count, now],
    )?;
    Ok(!exists)
}

fn row_to_book(row: &rusqlite::Row<'_>) -> rusqlite::Result<Book> {
    let subjects: String = row.get(8)?;
    Ok(Book {
        id: row.get(0)?,
        title: row.get(1)?,
        author: row.get(2)?,
        description: row.get(3)?,
        publisher: row.get(4)?,
        published_date: row.get(5)?,
        isbn: row.get(6)?,
        language: row.get(7)?,
        subjects: serde_json::from_str(&subjects).unwrap_or_default(),
        filename: row.get(9)?,
        relative_path: row.get(10)?,
        format: row.get(11)?,
        size: row.get(12)?,
        modified_at: row.get(13)?,
        page_count: row.get(14)?,
        is_available: row.get(15)?,
        has_cover: row.get(16)?,
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
        progress: None,
    })
}

fn row_to_note(row: &rusqlite::Row<'_>) -> rusqlite::Result<Note> {
    let location: String = row.get(2)?;
    Ok(Note {
        id: row.get(0)?,
        book_id: row.get(1)?,
        location: serde_json::from_str(&location).unwrap_or_default(),
        content: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn clean(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn validate_provider(provider: &SaveProvider) -> AppResult<()> {
    if provider.name.trim().is_empty() {
        return Err(AppError::BadRequest("o nome da fonte e obrigatorio".into()));
    }
    if !matches!(provider.kind.as_str(), "open_library" | "google_books") {
        return Err(AppError::BadRequest("tipo de fonte invalido".into()));
    }
    let parsed = url::Url::parse(&provider.base_url)
        .map_err(|_| AppError::BadRequest("URL da fonte invalida".into()))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AppError::BadRequest(
            "a fonte deve usar HTTP ou HTTPS".into(),
        ));
    }
    Ok(())
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS books (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    author TEXT,
    description TEXT,
    publisher TEXT,
    published_date TEXT,
    isbn TEXT,
    language TEXT,
    subjects TEXT NOT NULL DEFAULT '[]',
    filename TEXT NOT NULL,
    relative_path TEXT NOT NULL UNIQUE,
    format TEXT NOT NULL,
    size INTEGER NOT NULL,
    modified_at INTEGER NOT NULL,
    page_count INTEGER,
    cover_filename TEXT,
    is_available INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS reading_progress (
    book_id INTEGER PRIMARY KEY REFERENCES books(id) ON DELETE CASCADE,
    location TEXT NOT NULL,
    percent REAL NOT NULL DEFAULT 0 CHECK(percent >= 0 AND percent <= 100),
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS notes (
    id INTEGER PRIMARY KEY,
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    location TEXT NOT NULL DEFAULT '{}',
    content TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS metadata_providers (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    base_url TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    builtin INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS shares (
    id INTEGER PRIMARY KEY,
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    token TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    expires_at TEXT,
    revoked_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_books_title ON books(title);
CREATE INDEX IF NOT EXISTS idx_notes_book ON notes(book_id);
CREATE INDEX IF NOT EXISTS idx_shares_token ON shares(token);
"#;

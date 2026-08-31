use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use tokio::sync::Mutex;

use crate::{
    error::{AppError, AppResult},
    models::{
        Book, BookSummary, CatalogFacets, CatalogQuery, Collection, DuplicateBook, DuplicateGroup,
        HealthIssue, LibraryHealth, Note, Provider, ReadingProgress, SaveCollection, SaveNote,
        SaveProgress, SaveProvider, Share, UpdateBook,
    },
    scanner::{metadata_looks_corrupt, sanitize_metadata_text, KnownBook, ScannedBook},
};

#[derive(Clone)]
pub struct Database {
    connection: Arc<Mutex<Connection>>,
    path: Arc<PathBuf>,
}

impl Database {
    pub async fn open(path: &Path, initial_library_root: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let connection = open_connection(path)?;
        connection.execute(
            "INSERT OR IGNORE INTO settings (key, value) VALUES ('library_root', ?1)",
            [initial_library_root.to_string_lossy().as_ref()],
        )?;
        connection.execute(
            "INSERT OR IGNORE INTO settings (key, value) VALUES ('network_metadata_enabled', 'false')",
            [],
        )?;
        for (key, value) in [
            ("scan_profile", "economical"),
            ("carousel_enabled", "false"),
            ("carousel_interval_seconds", "12"),
            ("auto_cover_enabled", "false"),
        ] {
            connection.execute(
                "INSERT OR IGNORE INTO settings (key, value) VALUES (?1, ?2)",
                params![key, value],
            )?;
        }
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
        for (name, color) in [
            ("Favoritos", "#d75d36"),
            ("Para estudar", "#52758f"),
            ("Ler depois", "#6e816a"),
        ] {
            connection.execute(
                "INSERT OR IGNORE INTO collections (name, color, created_at) VALUES (?1, ?2, ?3)",
                params![name, color, Utc::now().to_rfc3339()],
            )?;
        }
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            path: Arc::new(path.to_path_buf()),
        })
    }

    pub async fn snapshot_to(&self, destination: &Path) -> AppResult<()> {
        let conn = self.connection.lock().await;
        conn.execute("VACUUM INTO ?1", [destination.to_string_lossy().as_ref()])?;
        Ok(())
    }

    pub async fn replace_from(&self, source: &Path) -> AppResult<()> {
        let candidate = Connection::open(source)?;
        let integrity: String =
            candidate.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err(AppError::BadRequest(format!(
                "o banco do backup falhou na verificacao de integridade: {integrity}"
            )));
        }
        let has_books: bool = candidate.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='books')",
            [],
            |row| row.get(0),
        )?;
        if !has_books {
            return Err(AppError::BadRequest(
                "o pacote nao contem um catalogo Estante Livre valido".into(),
            ));
        }
        drop(candidate);

        let mut conn = self.connection.lock().await;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        let placeholder = Connection::open_in_memory()?;
        let previous = std::mem::replace(&mut *conn, placeholder);
        drop(previous);

        let restore_new = self
            .path
            .with_extension(format!("restore-{}", Utc::now().timestamp_millis()));
        let restore_old = self
            .path
            .with_extension(format!("before-restore-{}", Utc::now().timestamp_millis()));
        std::fs::copy(source, &restore_new)?;
        if self.path.exists() {
            std::fs::rename(self.path.as_ref(), &restore_old)?;
        }
        if let Err(error) = std::fs::rename(&restore_new, self.path.as_ref()) {
            let _ = std::fs::rename(&restore_old, self.path.as_ref());
            *conn = Connection::open(self.path.as_ref())?;
            return Err(error.into());
        }
        match open_connection(self.path.as_ref()) {
            Ok(replacement) => {
                *conn = replacement;
                let _ = std::fs::remove_file(restore_old);
                Ok(())
            }
            Err(error) => {
                let _ = std::fs::remove_file(self.path.as_ref());
                let _ = std::fs::rename(&restore_old, self.path.as_ref());
                *conn = open_connection(self.path.as_ref())?;
                Err(error)
            }
        }
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

    pub async fn delete_setting(&self, key: &str) -> AppResult<()> {
        let conn = self.connection.lock().await;
        conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
        Ok(())
    }

    pub async fn scan_index(&self) -> AppResult<HashMap<String, KnownBook>> {
        let conn = self.connection.lock().await;
        let mut stmt = conn.prepare(
            "SELECT relative_path, title, author, size, modified_at, page_count, content_hash
             FROM books",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                KnownBook {
                    title: row.get(1)?,
                    author: row.get(2)?,
                    size: row.get(3)?,
                    modified_at: row.get(4)?,
                    page_count: row.get(5)?,
                    content_hash: row.get(6)?,
                },
            ))
        })?;
        Ok(rows.collect::<Result<HashMap<_, _>, _>>()?)
    }

    pub async fn sync_scan(&self, books: &[ScannedBook]) -> AppResult<(usize, usize, usize)> {
        let mut conn = self.connection.lock().await;
        let tx = conn.transaction()?;
        tx.execute("UPDATE books SET is_available = 0", [])?;
        let mut added = 0;
        let mut updated = 0;
        let mut unchanged = 0;
        for book in books {
            if upsert_scanned(&tx, book)? {
                added += 1;
            } else if book.unchanged {
                unchanged += 1;
            } else {
                updated += 1;
            }
        }
        tx.commit()?;
        Ok((added, updated, unchanged))
    }

    pub async fn save_scan_issues(&self, warnings: &[String]) -> AppResult<()> {
        let mut conn = self.connection.lock().await;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM scan_issues", [])?;
        let now = Utc::now().to_rfc3339();
        for warning in warnings {
            let (path, message) = warning
                .split_once(": ")
                .unwrap_or(("varredura", warning.as_str()));
            tx.execute(
                "INSERT INTO scan_issues (path, message, created_at) VALUES (?1, ?2, ?3)",
                params![path, message, now],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub async fn list_books(&self, query: &CatalogQuery) -> AppResult<Vec<BookSummary>> {
        let conn = self.connection.lock().await;
        let search = format!("%{}%", query.q.as_deref().unwrap_or_default().trim());
        let format = query.format.as_deref().unwrap_or_default().trim();
        let author = query.author.as_deref().unwrap_or_default().trim();
        let subject = query.subject.as_deref().unwrap_or_default().trim();
        let publisher = query.publisher.as_deref().unwrap_or_default().trim();
        let language = query.language.as_deref().unwrap_or_default().trim();
        let year = format!("{}%", query.year.as_deref().unwrap_or_default().trim());
        let progress = query.progress.as_deref().unwrap_or_default().trim();
        let availability = query.availability.as_deref().unwrap_or_default().trim();
        let order = match query.sort.as_deref() {
            Some("author") => "COALESCE(b.author, '') COLLATE NOCASE, b.title COLLATE NOCASE",
            Some("year") => "COALESCE(b.published_date, '') DESC, b.title COLLATE NOCASE",
            Some("recent") => "b.updated_at DESC",
            Some("size") => "b.size DESC",
            Some("progress") => "COALESCE(p.percent, 0) DESC, b.title COLLATE NOCASE",
            _ => "b.title COLLATE NOCASE",
        };
        let sql = format!(
            "SELECT b.id, b.title, b.author, b.filename, b.format, b.subjects,
                    b.publisher, b.published_date, b.language, b.size, b.is_available,
                    b.cover_filename IS NOT NULL, COALESCE(p.percent, 0), b.updated_at
             FROM books b LEFT JOIN reading_progress p ON p.book_id = b.id
             WHERE (?1 = '%%' OR b.title LIKE ?1 OR COALESCE(b.author, '') LIKE ?1
                    OR b.filename LIKE ?1 OR b.subjects LIKE ?1)
               AND (?2 = '' OR b.format = ?2)
               AND (?3 = '' OR COALESCE(b.author, '') = ?3 COLLATE NOCASE)
               AND (?4 = '' OR EXISTS (
                    SELECT 1 FROM json_each(b.subjects) WHERE value = ?4 COLLATE NOCASE
               ))
               AND (?5 = '' OR COALESCE(b.publisher, '') = ?5 COLLATE NOCASE)
               AND (?6 = '' OR COALESCE(b.language, '') = ?6 COLLATE NOCASE)
               AND (?7 = '%' OR COALESCE(b.published_date, '') LIKE ?7)
               AND (?8 IS NULL OR b.size >= ?8)
               AND (?9 IS NULL OR b.size <= ?9)
               AND (?10 = '' OR (?10 = 'unread' AND COALESCE(p.percent, 0) = 0)
                    OR (?10 = 'reading' AND p.percent > 0 AND p.percent < 100)
                    OR (?10 = 'finished' AND p.percent >= 100))
               AND (?11 = '' OR (?11 = 'available' AND b.is_available = 1)
                    OR (?11 = 'missing' AND b.is_available = 0))
               AND (?12 IS NULL OR EXISTS (
                    SELECT 1 FROM collection_books cb
                    WHERE cb.book_id = b.id AND cb.collection_id = ?12
               ))
             ORDER BY {order}"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![
                search,
                format,
                author,
                subject,
                publisher,
                language,
                year,
                query.min_size,
                query.max_size,
                progress,
                availability,
                query.collection_id,
            ],
            row_to_summary,
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub async fn reading_desk(&self) -> AppResult<Vec<BookSummary>> {
        let conn = self.connection.lock().await;
        let mut stmt = conn.prepare(
            "SELECT b.id, b.title, b.author, b.filename, b.format, b.subjects,
                    b.publisher, b.published_date, b.language, b.size, b.is_available,
                    b.cover_filename IS NOT NULL, p.percent, b.updated_at
             FROM books b JOIN reading_progress p ON p.book_id = b.id
             WHERE b.is_available=1 AND p.percent > 0 AND p.percent < 100
             ORDER BY p.updated_at DESC",
        )?;
        let rows = stmt.query_map([], row_to_summary)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub async fn suggestions(&self, limit: usize) -> AppResult<Vec<BookSummary>> {
        let conn = self.connection.lock().await;
        let mut stmt = conn.prepare(
            "SELECT b.id, b.title, b.author, b.filename, b.format, b.subjects,
                    b.publisher, b.published_date, b.language, b.size, b.is_available,
                    b.cover_filename IS NOT NULL, COALESCE(p.percent, 0), b.updated_at
             FROM books b LEFT JOIN reading_progress p ON p.book_id = b.id
             WHERE b.is_available=1
             ORDER BY RANDOM() LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit.clamp(1, 20) as i64], row_to_summary)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub async fn catalog_facets(&self) -> AppResult<CatalogFacets> {
        let conn = self.connection.lock().await;
        let mut formats = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT format FROM books WHERE is_available=1 ORDER BY format COLLATE NOCASE",
        )?;
        for value in stmt.query_map([], |row| row.get(0))? {
            formats.push(value?);
        }

        let mut authors = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT author FROM books WHERE is_available=1 AND author IS NOT NULL
             AND TRIM(author) <> '' ORDER BY author COLLATE NOCASE",
        )?;
        for value in stmt.query_map([], |row| row.get(0))? {
            authors.push(value?);
        }

        let mut subjects = BTreeSet::new();
        let mut stmt = conn.prepare("SELECT subjects FROM books WHERE is_available=1")?;
        for value in stmt.query_map([], |row| row.get::<_, String>(0))? {
            let values: Vec<String> = serde_json::from_str(&value?).unwrap_or_default();
            subjects.extend(values.into_iter().filter(|value| !value.trim().is_empty()));
        }
        let mut publishers = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT publisher FROM books WHERE is_available=1 AND publisher IS NOT NULL
             AND TRIM(publisher) <> '' ORDER BY publisher COLLATE NOCASE",
        )?;
        for value in stmt.query_map([], |row| row.get(0))? {
            publishers.push(value?);
        }
        let mut languages = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT language FROM books WHERE is_available=1 AND language IS NOT NULL
             AND TRIM(language) <> '' ORDER BY language COLLATE NOCASE",
        )?;
        for value in stmt.query_map([], |row| row.get(0))? {
            languages.push(value?);
        }
        let mut years = BTreeSet::new();
        let mut stmt = conn.prepare(
            "SELECT published_date FROM books WHERE is_available=1 AND published_date IS NOT NULL",
        )?;
        for value in stmt.query_map([], |row| row.get::<_, String>(0))? {
            let value = value?;
            if let Some(year) = value
                .get(..4)
                .filter(|year| year.chars().all(|c| c.is_ascii_digit()))
            {
                years.insert(year.to_string());
            }
        }
        let collections = query_collections(&conn)?;
        Ok(CatalogFacets {
            formats,
            authors,
            subjects: subjects.into_iter().collect(),
            publishers,
            languages,
            years: years.into_iter().rev().collect(),
            collections,
        })
    }

    pub async fn collections(&self) -> AppResult<Vec<Collection>> {
        let conn = self.connection.lock().await;
        query_collections(&conn)
    }

    pub async fn create_collection(&self, input: &SaveCollection) -> AppResult<Collection> {
        let name = input.name.trim();
        validate_collection(name, &input.color)?;
        let now = Utc::now().to_rfc3339();
        let conn = self.connection.lock().await;
        conn.execute(
            "INSERT INTO collections (name, color, created_at) VALUES (?1, ?2, ?3)",
            params![name, input.color, now],
        )?;
        Ok(Collection {
            id: conn.last_insert_rowid(),
            name: name.into(),
            color: input.color.clone(),
            book_count: 0,
            created_at: now,
        })
    }

    pub async fn update_collection(&self, id: i64, input: &SaveCollection) -> AppResult<()> {
        let name = input.name.trim();
        validate_collection(name, &input.color)?;
        let conn = self.connection.lock().await;
        if conn.execute(
            "UPDATE collections SET name=?2, color=?3 WHERE id=?1",
            params![id, name, input.color],
        )? == 0
        {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    pub async fn delete_collection(&self, id: i64) -> AppResult<()> {
        let conn = self.connection.lock().await;
        if conn.execute("DELETE FROM collections WHERE id=?1", [id])? == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    pub async fn add_to_collection(&self, collection_id: i64, book_id: i64) -> AppResult<()> {
        let conn = self.connection.lock().await;
        conn.execute(
            "INSERT OR IGNORE INTO collection_books (collection_id, book_id, added_at)
             VALUES (?1, ?2, ?3)",
            params![collection_id, book_id, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub async fn remove_from_collection(&self, collection_id: i64, book_id: i64) -> AppResult<()> {
        let conn = self.connection.lock().await;
        conn.execute(
            "DELETE FROM collection_books WHERE collection_id=?1 AND book_id=?2",
            params![collection_id, book_id],
        )?;
        Ok(())
    }

    pub async fn book_collection_ids(&self, book_id: i64) -> AppResult<Vec<i64>> {
        let conn = self.connection.lock().await;
        let mut stmt = conn.prepare(
            "SELECT collection_id FROM collection_books WHERE book_id=?1 ORDER BY collection_id",
        )?;
        let rows = stmt.query_map([book_id], |row| row.get(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub async fn duplicates(&self) -> AppResult<Vec<DuplicateGroup>> {
        let conn = self.connection.lock().await;
        let mut stmt = conn.prepare(
            "SELECT b.content_hash, b.size, b.id, b.title, b.filename, b.relative_path
             FROM books b
             WHERE b.is_available=1 AND b.content_hash IS NOT NULL
               AND b.content_hash IN (
                 SELECT content_hash FROM books WHERE is_available=1 AND content_hash IS NOT NULL
                 GROUP BY content_hash, size HAVING COUNT(*) > 1
               )
             ORDER BY b.content_hash, b.relative_path COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                DuplicateBook {
                    id: row.get(2)?,
                    title: row.get(3)?,
                    filename: row.get(4)?,
                    relative_path: row.get(5)?,
                    size: row.get(1)?,
                },
            ))
        })?;
        let mut groups: Vec<DuplicateGroup> = Vec::new();
        for row in rows {
            let (fingerprint, size, book) = row?;
            if let Some(group) = groups
                .last_mut()
                .filter(|group| group.fingerprint == fingerprint)
            {
                group.books.push(book);
            } else {
                groups.push(DuplicateGroup {
                    fingerprint,
                    size,
                    books: vec![book],
                });
            }
        }
        Ok(groups)
    }

    pub async fn library_health(&self) -> AppResult<LibraryHealth> {
        let conn = self.connection.lock().await;
        let counts = conn.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN is_available=1 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN is_available=0 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN author IS NULL OR TRIM(author)='' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN subjects='[]' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN cover_filename IS NULL THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN language IS NULL OR TRIM(language)='' THEN 1 ELSE 0 END), 0)
             FROM books",
            [],
            |row| Ok((row.get::<_, usize>(0)?, row.get::<_, usize>(1)?, row.get::<_, usize>(2)?, row.get::<_, usize>(3)?, row.get::<_, usize>(4)?, row.get::<_, usize>(5)?, row.get::<_, usize>(6)?)),
        )?;
        let duplicate_groups: usize = conn.query_row(
            "SELECT COUNT(*) FROM (
               SELECT content_hash FROM books WHERE is_available=1 AND content_hash IS NOT NULL
               GROUP BY content_hash, size HAVING COUNT(*) > 1
             )",
            [],
            |row| row.get(0),
        )?;
        let mut stmt =
            conn.prepare("SELECT path, message FROM scan_issues ORDER BY id DESC LIMIT 100")?;
        let rows = stmt.query_map([], |row| {
            Ok(HealthIssue {
                path: row.get(0)?,
                message: row.get(1)?,
            })
        })?;
        Ok(LibraryHealth {
            total: counts.0,
            available: counts.1,
            unavailable: counts.2,
            without_author: counts.3,
            without_subject: counts.4,
            without_cover: counts.5,
            without_language: counts.6,
            duplicate_groups,
            scan_issues: rows.collect::<Result<Vec<_>, _>>()?,
        })
    }

    pub async fn mark_unavailable(&self, id: i64) -> AppResult<()> {
        let conn = self.connection.lock().await;
        if conn.execute(
            "UPDATE books SET is_available=0, updated_at=?2 WHERE id=?1",
            params![id, Utc::now().to_rfc3339()],
        )? == 0
        {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    pub async fn missing_pdf_covers(&self, limit: usize) -> AppResult<Vec<(i64, String)>> {
        let conn = self.connection.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, relative_path FROM books
             WHERE is_available=1 AND format='pdf' AND cover_filename IS NULL
             ORDER BY id LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit.clamp(1, 10_000) as i64], |row| {
            Ok((row.get(0)?, row.get(1)?))
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
        let title = sanitize_metadata_text(&update.title).ok_or_else(|| {
            AppError::BadRequest(
                "informe um titulo legivel; caracteres invalidos foram recusados".into(),
            )
        })?;
        let conn = self.connection.lock().await;
        let changed = conn.execute(
            "UPDATE books SET title=?2, author=?3, description=?4, publisher=?5,
             published_date=?6, isbn=?7, language=?8, subjects=?9, updated_at=?10 WHERE id=?1",
            params![
                id,
                title,
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

    pub async fn remove_progress(&self, book_id: i64) -> AppResult<()> {
        let conn = self.connection.lock().await;
        conn.execute("DELETE FROM reading_progress WHERE book_id=?1", [book_id])?;
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
    let existing_title: Option<String> = tx
        .query_row(
            "SELECT title FROM books WHERE relative_path=?1",
            [&book.relative_path],
            |row| row.get(0),
        )
        .optional()?;
    let repair_metadata = existing_title
        .as_deref()
        .map(metadata_looks_corrupt)
        .unwrap_or(false);
    let now = Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO books (title, author, filename, relative_path, format, size, modified_at,
          page_count, content_hash, is_available, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, ?10, ?10)
         ON CONFLICT(relative_path) DO UPDATE SET filename=excluded.filename, format=excluded.format,
          size=excluded.size, modified_at=excluded.modified_at, page_count=COALESCE(excluded.page_count, books.page_count),
          content_hash=COALESCE(excluded.content_hash, books.content_hash),
          title=CASE WHEN ?11 THEN excluded.title ELSE books.title END,
          author=CASE WHEN ?11 THEN COALESCE(excluded.author, books.author) ELSE books.author END,
          is_available=1,
          updated_at=CASE WHEN ?12 THEN books.updated_at ELSE excluded.updated_at END",
        params![book.title, book.author, book.filename, book.relative_path, book.format,
                book.size, book.modified_at, book.page_count, book.content_hash, now,
                repair_metadata, book.unchanged],
    )?;
    Ok(existing_title.is_none())
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

fn row_to_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<BookSummary> {
    let subjects: String = row.get(5)?;
    Ok(BookSummary {
        id: row.get(0)?,
        title: row.get(1)?,
        author: row.get(2)?,
        filename: row.get(3)?,
        format: row.get(4)?,
        subjects: serde_json::from_str(&subjects).unwrap_or_default(),
        publisher: row.get(6)?,
        published_date: row.get(7)?,
        language: row.get(8)?,
        size: row.get(9)?,
        is_available: row.get(10)?,
        has_cover: row.get(11)?,
        progress_percent: row.get(12)?,
        updated_at: row.get(13)?,
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

fn query_collections(conn: &Connection) -> AppResult<Vec<Collection>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.name, c.color, COUNT(cb.book_id), c.created_at
         FROM collections c LEFT JOIN collection_books cb ON cb.collection_id=c.id
         GROUP BY c.id ORDER BY c.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Collection {
            id: row.get(0)?,
            name: row.get(1)?,
            color: row.get(2)?,
            book_count: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn validate_collection(name: &str, color: &str) -> AppResult<()> {
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::BadRequest(
            "o nome da colecao deve ter entre 1 e 80 caracteres".into(),
        ));
    }
    if color.len() != 7
        || !color.starts_with('#')
        || !color[1..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(AppError::BadRequest("cor da colecao invalida".into()));
    }
    Ok(())
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
    content_hash TEXT,
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
CREATE TABLE IF NOT EXISTS collections (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE,
    color TEXT NOT NULL DEFAULT '#6e816a',
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS collection_books (
    collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    added_at TEXT NOT NULL,
    PRIMARY KEY(collection_id, book_id)
);
CREATE TABLE IF NOT EXISTS scan_issues (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL,
    message TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_books_title ON books(title);
CREATE INDEX IF NOT EXISTS idx_notes_book ON notes(book_id);
CREATE INDEX IF NOT EXISTS idx_shares_token ON shares(token);
CREATE INDEX IF NOT EXISTS idx_collection_books_book ON collection_books(book_id);
"#;

fn migrate(connection: &Connection) -> rusqlite::Result<()> {
    let mut stmt = connection.prepare("PRAGMA table_info(books)")?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !columns.iter().any(|column| column == "content_hash") {
        connection.execute("ALTER TABLE books ADD COLUMN content_hash TEXT", [])?;
    }
    connection.execute(
        "CREATE INDEX IF NOT EXISTS idx_books_fingerprint ON books(content_hash, size)",
        [],
    )?;
    Ok(())
}

fn open_connection(path: &Path) -> AppResult<Connection> {
    let connection = Connection::open(path)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.execute_batch(SCHEMA)?;
    migrate(&connection)?;
    Ok(connection)
}

use std::{collections::HashMap, path::Path, sync::Arc};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::{mpsc, oneshot};

use crate::{
    error::{AppError, AppResult},
    models::{
        validate_location, Book, BookSummary, CatalogFacets, CatalogPage, CatalogQuery, Collection,
        DuplicateBook, DuplicateGroup, HealthIssue, LibraryHealth, Note, Provider, ReadingProgress,
        SaveCollection, SaveNote, SaveProgress, SaveProvider, Share, UpdateBook,
    },
    scanner::{metadata_looks_corrupt, sanitize_metadata_text, KnownBook, ScannedBook},
};

type DatabaseJob = Box<dyn FnOnce(&mut Connection) + Send>;

/// Uma thread dedicada serializa SQLite sem bloquear o executor HTTP.
#[derive(Clone)]
pub struct Database {
    sender: Arc<mpsc::Sender<DatabaseJob>>,
}

impl Database {
    pub async fn open(path: &Path, initial_library_root: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let path = path.to_path_buf();
        let initial_library_root = initial_library_root.to_path_buf();
        let connection = tokio::task::spawn_blocking(move || -> AppResult<Connection> {
            let connection = open_connection(&path)?;
            connection.execute("INSERT OR IGNORE INTO settings(key,value) VALUES('catalog_id',?1)", [format!("{:032x}", rand::random::<u128>())])?;
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
            Ok(connection)
        }).await.map_err(|error| AppError::Internal(error.to_string()))??;
        Self::from_connection(connection)
    }

    /// Transfere a conexao para uma unica thread com fila limitada.
    fn from_connection(connection: Connection) -> AppResult<Self> {
        let (sender, mut receiver) = mpsc::channel::<DatabaseJob>(64);
        std::thread::Builder::new()
            .name("sqlite-worker".into())
            .spawn(move || {
                let mut connection = connection;
                while let Some(job) = receiver.blocking_recv() {
                    job(&mut connection);
                }
            })?;
        Ok(Self {
            sender: Arc::new(sender),
        })
    }

    /// Enfileira uma operacao com dados proprios e aguarda seu resultado.
    async fn run<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Connection) -> AppResult<T> + Send + 'static,
    ) -> AppResult<T> {
        let (reply, result) = oneshot::channel();
        self.sender
            .send(Box::new(move |connection| {
                let _ = reply.send(operation(connection));
            }))
            .await
            .map_err(|_| AppError::Internal("worker SQLite encerrado".into()))?;
        result
            .await
            .map_err(|_| AppError::Internal("worker SQLite sem resposta".into()))?
    }

    /// Troca a raiz e a identificacao da fila local em uma unica transacao.
    pub async fn set_library_root(&self, root: &Path) -> AppResult<()> {
        let root = root.to_string_lossy().into_owned();
        self.run(move |conn| {
            let previous: String = conn.query_row(
                "SELECT value FROM settings WHERE key='library_root'",
                [],
                |row| row.get(0),
            )?;
            let tx = conn.transaction()?;
            if previous != root {
                tx.execute(
                    "UPDATE settings SET value=?1 WHERE key='catalog_id'",
                    [format!("{:032x}", rand::random::<u128>())],
                )?;
            }
            tx.execute(
                "UPDATE settings SET value=?1 WHERE key='library_root'",
                [root],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// Referencias de todos os livros, inclusive indisponiveis, protegem suas capas.
    pub async fn referenced_covers(&self) -> AppResult<std::collections::HashSet<String>> {
        self.run(|conn| {
            let mut statement =
                conn.prepare("SELECT cover_filename FROM books WHERE cover_filename IS NOT NULL")?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
        .await
    }

    pub async fn mark_available(&self, id: i64) -> AppResult<()> {
        self.run(move |conn| {
            conn.execute("UPDATE books SET is_available=1 WHERE id=?1", [id])?;
            Ok(())
        })
        .await
    }

    pub async fn snapshot_to(&self, destination: &Path) -> AppResult<()> {
        let destination = destination.to_path_buf();
        self.run(move |conn| {
            conn.execute("VACUUM INTO ?1", [destination.to_string_lossy().as_ref()])?;
            Ok(())
        })
        .await
    }

    pub async fn replace_from(&self, source: &Path) -> AppResult<()> {
        let source = source.to_path_buf();
        self.run(move |conn| {

            let candidate = Connection::open(&source)?;
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
            validate_restore_schema(&candidate)?;
            drop(candidate);

            // O source e a copia extraida do ZIP. Migre antes de copiar para o
            // destino. SQLite faz commit apenas ao concluir; Drop aborta se falhar.
            let candidate = open_connection(&source)?;
            validate_restore_data(&candidate)?;
            // Estes valores descrevem esta instalacao e seu consentimento de rede.
            for key in ["library_root", "network_metadata_enabled"] {
                let value: String =
                    conn.query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
                        row.get(0)
                    })?;
                candidate.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, value])?;
            }
            candidate.execute("INSERT INTO settings(key,value) VALUES('catalog_id',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [format!("{:032x}", rand::random::<u128>())])?;
            let backup = rusqlite::backup::Backup::new(&candidate, conn)?;
            match backup.step(-1)? {
                rusqlite::backup::StepResult::Done => Ok(()),
                _ => Err(AppError::BadRequest(
                    "banco ocupado; tente restaurar novamente".into(),
                )),
            }
        }).await
    }

    pub async fn setting(&self, key: &str) -> AppResult<Option<String>> {
        let key = key.to_owned();
        self.run(move |conn| {
            let key = key.as_str();
            Ok(conn
                .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                    row.get(0)
                })
                .optional()?)
        })
        .await
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> AppResult<()> {
        let key = key.to_owned();
        let value = value.to_owned();
        self.run(move |conn| {
            let key = key.as_str();
            let value = value.as_str();
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn delete_setting(&self, key: &str) -> AppResult<()> {
        let key = key.to_owned();
        self.run(move |conn| {
            let key = key.as_str();
            conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
            Ok(())
        })
        .await
    }

    pub async fn scan_index(&self) -> AppResult<HashMap<String, KnownBook>> {
        self.run(move |conn| {

            let mut stmt = conn.prepare(
                "SELECT relative_path, title, author, size, modified_at, page_count, content_hash, metadata_scanned
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
                        metadata_scanned: row.get(7)?,
                    },
                ))
            })?;
            Ok(rows.collect::<Result<HashMap<_, _>, _>>()?)
        }).await
    }

    pub async fn sync_scan(&self, books: &[ScannedBook]) -> AppResult<(usize, usize, usize)> {
        let books = books.to_vec();
        self.run(move |conn| {
                let books = books.as_slice();
            let tx = conn.transaction()?;
            tx.execute("UPDATE books SET is_available = 0", [])?;
            let mut added = 0;
            let mut updated = 0;
            let mut unchanged = 0;
            let now = Utc::now().to_rfc3339();
            let mut select_stmt = tx.prepare("SELECT title FROM books WHERE relative_path=?1")?;
            let mut upsert_stmt = tx.prepare(
                "INSERT INTO books (title, author, filename, relative_path, format, size, modified_at,
                  page_count, content_hash, is_available, created_at, updated_at, metadata_scanned)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, ?10, ?10, ?13)
                 ON CONFLICT(relative_path) DO UPDATE SET filename=excluded.filename, format=excluded.format,
                  size=excluded.size, modified_at=excluded.modified_at, page_count=COALESCE(excluded.page_count, books.page_count),
                  content_hash=COALESCE(excluded.content_hash, books.content_hash),
                  title=CASE WHEN ?11 OR (books.metadata_edited=0 AND excluded.metadata_scanned=1)
                        THEN excluded.title ELSE books.title END,
                  author=CASE WHEN ?11 OR (books.metadata_edited=0 AND excluded.metadata_scanned=1)
                        THEN COALESCE(excluded.author, books.author) ELSE books.author END,
                  metadata_scanned=excluded.metadata_scanned,
                  is_available=1,
                  updated_at=CASE WHEN ?12 THEN books.updated_at ELSE excluded.updated_at END",
            )?;
            for book in books {
                let existing_title: Option<String> = select_stmt
                    .query_row([&book.relative_path], |row| row.get(0))
                    .optional()?;
                let repair_metadata = existing_title
                    .as_deref()
                    .map(metadata_looks_corrupt)
                    .unwrap_or(false);
                upsert_stmt.execute(params![
                    book.title,
                    book.author,
                    book.filename,
                    book.relative_path,
                    book.format,
                    book.size,
                    book.modified_at,
                    book.page_count,
                    book.content_hash,
                    now,
                    repair_metadata,
                    book.unchanged,
                    book.metadata_scanned,
                ])?;
                if existing_title.is_none() {
                    added += 1;
                } else if book.unchanged {
                    unchanged += 1;
                } else {
                    updated += 1;
                }
            }
            drop(select_stmt);
            drop(upsert_stmt);
            tx.commit()?;
            Ok((added, updated, unchanged))
        }).await
    }

    pub async fn save_scan_issues(&self, warnings: &[String]) -> AppResult<()> {
        let warnings = warnings.to_vec();
        self.run(move |conn| {
            let warnings = warnings.as_slice();
            let tx = conn.transaction()?;
            tx.execute("DELETE FROM scan_issues", [])?;
            let now = Utc::now().to_rfc3339();
            let mut insert_stmt = tx.prepare(
                "INSERT INTO scan_issues (path, message, created_at) VALUES (?1, ?2, ?3)",
            )?;
            for warning in warnings {
                let (path, message) = warning
                    .split_once(": ")
                    .unwrap_or(("varredura", warning.as_str()));
                insert_stmt.execute(params![path, message, now])?;
            }
            drop(insert_stmt);
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// Retorna uma pagina e seu total filtrado, com limite de 1 a 200 livros.
    pub async fn list_books(&self, query: &CatalogQuery) -> AppResult<CatalogPage> {
        let query = query.clone();
        self.run(move |conn| {
            let query = &query;
            let limit = query.limit.unwrap_or(60);
            if !(1..=200).contains(&limit) {
                return Err(AppError::BadRequest(
                    "limit deve ficar entre 1 e 200".into(),
                ));
            }
            let offset = query.offset.unwrap_or(0);
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
            let filter_sql = "FROM books b LEFT JOIN reading_progress p ON p.book_id = b.id
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
                   ))";
            let sql = format!(
                "SELECT b.id, b.title, b.author, b.filename, b.format, b.subjects,
                        b.publisher, b.published_date, b.language, b.size, b.is_available,
                        b.cover_filename IS NOT NULL, COALESCE(p.percent, 0), b.updated_at
                 {filter_sql} ORDER BY {order}, b.id"
            );
            let parameters = params![
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
            ];
            let total = conn.query_row(
                &format!("SELECT COUNT(*) {filter_sql}"),
                parameters,
                |row| row.get(0),
            )?;
            let mut stmt = conn.prepare(&format!("{sql} LIMIT ?13 OFFSET ?14"))?;
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
                    limit,
                    offset,
                ],
                row_to_summary,
            )?;
            Ok(CatalogPage {
                books: rows.collect::<Result<Vec<_>, _>>()?,
                total,
                limit,
                offset,
            })
        })
        .await
    }

    pub async fn reading_desk(&self) -> AppResult<Vec<BookSummary>> {
        self.run(move |conn| {
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
        })
        .await
    }

    pub async fn suggestions(&self, limit: usize) -> AppResult<Vec<BookSummary>> {
        self.run(move |conn| {
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
        })
        .await
    }

    pub async fn catalog_facets(&self) -> AppResult<CatalogFacets> {
        self.run(move |conn| {
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

                let mut subjects = Vec::new();
                let mut stmt = conn.prepare(
                    "SELECT DISTINCT TRIM(value) FROM books, json_each(books.subjects)
                 WHERE is_available=1 AND json_valid(books.subjects) AND TRIM(value) <> ''
                 ORDER BY TRIM(value) COLLATE NOCASE",
                )?;
                for value in stmt.query_map([], |row| row.get(0))? {
                    subjects.push(value?);
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
                let mut years = Vec::new();
                let mut stmt = conn.prepare(
                    "SELECT DISTINCT SUBSTR(published_date, 1, 4) AS yr FROM books
                 WHERE is_available=1 AND published_date IS NOT NULL AND LENGTH(published_date) >= 4
                   AND SUBSTR(published_date, 1, 4) GLOB '[0-9][0-9][0-9][0-9]'
                 ORDER BY yr DESC",
                )?;
                for value in stmt.query_map([], |row| row.get(0))? {
                    years.push(value?);
                }
                let collections = query_collections(conn)?;
                Ok(CatalogFacets {
                    formats,
                    authors,
                    subjects,
                    publishers,
                    languages,
                    years,
                    collections,
                })
            })
            .await
    }

    pub async fn collections(&self) -> AppResult<Vec<Collection>> {
        self.run(move |conn| query_collections(conn)).await
    }

    pub async fn create_collection(&self, input: &SaveCollection) -> AppResult<Collection> {
        let input = input.clone();
        self.run(move |conn| {
            let input = &input;
            let name = input.name.trim();
            validate_collection(name, &input.color)?;
            let now = Utc::now().to_rfc3339();
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
        })
        .await
    }

    pub async fn update_collection(&self, id: i64, input: &SaveCollection) -> AppResult<()> {
        let input = input.clone();
        self.run(move |conn| {
            let input = &input;
            let name = input.name.trim();
            validate_collection(name, &input.color)?;
            if conn.execute(
                "UPDATE collections SET name=?2, color=?3 WHERE id=?1",
                params![id, name, input.color],
            )? == 0
            {
                return Err(AppError::NotFound);
            }
            Ok(())
        })
        .await
    }

    pub async fn delete_collection(&self, id: i64) -> AppResult<()> {
        self.run(move |conn| {
            if conn.execute("DELETE FROM collections WHERE id=?1", [id])? == 0 {
                return Err(AppError::NotFound);
            }
            Ok(())
        })
        .await
    }

    pub async fn add_to_collection(&self, collection_id: i64, book_id: i64) -> AppResult<()> {
        self.run(move |conn| {
            conn.execute(
                "INSERT OR IGNORE INTO collection_books (collection_id, book_id, added_at)
                 VALUES (?1, ?2, ?3)",
                params![collection_id, book_id, Utc::now().to_rfc3339()],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn remove_from_collection(&self, collection_id: i64, book_id: i64) -> AppResult<()> {
        self.run(move |conn| {
            conn.execute(
                "DELETE FROM collection_books WHERE collection_id=?1 AND book_id=?2",
                params![collection_id, book_id],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn book_collection_ids(&self, book_id: i64) -> AppResult<Vec<i64>> {
        self.run(move |conn| {
                let mut stmt = conn.prepare(
                "SELECT collection_id FROM collection_books WHERE book_id=?1 ORDER BY collection_id",
            )?;
                let rows = stmt.query_map([book_id], |row| row.get(0))?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })
            .await
    }

    pub async fn duplicates(&self) -> AppResult<Vec<DuplicateGroup>> {
        self.run(move |conn| {
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
            })
            .await
    }

    pub async fn library_health(&self) -> AppResult<LibraryHealth> {
        self.run(move |conn| {

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
        }).await
    }

    pub async fn mark_unavailable(&self, id: i64) -> AppResult<()> {
        self.run(move |conn| {
            if conn.execute(
                "UPDATE books SET is_available=0, updated_at=?2 WHERE id=?1",
                params![id, Utc::now().to_rfc3339()],
            )? == 0
            {
                return Err(AppError::NotFound);
            }
            Ok(())
        })
        .await
    }

    pub async fn missing_pdf_covers(&self, limit: usize) -> AppResult<Vec<(i64, String)>> {
        self.run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, relative_path FROM books
                 WHERE is_available=1 AND format='pdf' AND cover_filename IS NULL
                 ORDER BY id LIMIT ?1",
            )?;
            let rows = stmt.query_map([limit.clamp(1, 10_000) as i64], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
    }

    pub async fn book(&self, id: i64) -> AppResult<Book> {
        self.run(move |conn| {
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
            })
            .await
    }

    pub async fn cover_filename(&self, id: i64) -> AppResult<String> {
        self.run(move |conn| {
            conn.query_row(
                "SELECT cover_filename FROM books WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(AppError::NotFound)
        })
        .await
    }

    pub async fn update_book(&self, id: i64, update: &UpdateBook) -> AppResult<()> {
        self.update_book_with_cover(id, update, None).await
    }

    /// Grava metadados e a referencia da capa em uma unica instrucao SQLite.
    /// None conserva a capa existente; o arquivo novo deve estar pronto antes.
    pub async fn update_book_with_cover(
        &self,
        id: i64,
        update: &UpdateBook,
        cover: Option<&str>,
    ) -> AppResult<()> {
        let update = update.clone();
        let cover = cover.map(str::to_owned);
        self.run(move |conn| {
            let update = &update;
            let cover = cover.as_deref();
            let title = sanitize_metadata_text(&update.title).ok_or_else(|| {
                AppError::BadRequest(
                    "informe um titulo legivel; caracteres invalidos foram recusados".into(),
                )
            })?;
            let changed = conn.execute(
                "UPDATE books SET title=?2, author=?3, description=?4, publisher=?5,
                 published_date=?6, isbn=?7, language=?8, subjects=?9, updated_at=?10,
                 metadata_edited=1, cover_filename=COALESCE(?11, cover_filename) WHERE id=?1",
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
                    cover,
                ],
            )?;
            if changed == 0 {
                return Err(AppError::NotFound);
            }
            Ok(())
        })
        .await
    }

    pub async fn set_cover(&self, id: i64, filename: &str) -> AppResult<()> {
        let filename = filename.to_owned();
        self.run(move |conn| {
            let filename = filename.as_str();
            conn.execute(
                "UPDATE books SET cover_filename=?2, updated_at=?3 WHERE id=?1",
                params![id, filename, Utc::now().to_rfc3339()],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn save_progress(&self, book_id: i64, progress: &SaveProgress) -> AppResult<()> {
        let progress = progress.clone();
        self.run(move |conn| {
                let progress = &progress;
            if !progress.percent.is_finite() || !(0.0..=100.0).contains(&progress.percent) {
                return Err(AppError::BadRequest(
                    "o progresso deve estar entre 0 e 100".into(),
                ));
            }
            validate_location(&progress.location)?;
            let location = progress.location.to_string();
            conn.execute(
                "INSERT INTO reading_progress (book_id, location, percent, updated_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(book_id) DO UPDATE SET location=excluded.location,
                 percent=excluded.percent, updated_at=excluded.updated_at",
                params![book_id, location, progress.percent, Utc::now().to_rfc3339()],
            )?;
            Ok(())
        }).await
    }

    pub async fn remove_progress(&self, book_id: i64) -> AppResult<()> {
        self.run(move |conn| {
            conn.execute("DELETE FROM reading_progress WHERE book_id=?1", [book_id])?;
            Ok(())
        })
        .await
    }

    pub async fn notes(&self, book_id: i64) -> AppResult<Vec<Note>> {
        self.run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, book_id, location, content, created_at, updated_at
             FROM notes WHERE book_id=?1 ORDER BY updated_at DESC",
            )?;
            let rows = stmt.query_map([book_id], row_to_note)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
    }

    pub async fn create_note(&self, book_id: i64, note: &SaveNote) -> AppResult<Note> {
        let note = note.clone();
        self.run(move |conn| {
            let note = &note;
            validate_location(&note.location)?;
            if note.content.trim().is_empty() {
                return Err(AppError::BadRequest(
                    "a anotacao nao pode ficar vazia".into(),
                ));
            }
            let now = Utc::now().to_rfc3339();
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
        })
        .await
    }

    pub async fn delete_note(&self, id: i64) -> AppResult<()> {
        self.run(move |conn| {
            if conn.execute("DELETE FROM notes WHERE id=?1", [id])? == 0 {
                return Err(AppError::NotFound);
            }
            Ok(())
        })
        .await
    }

    pub async fn providers(&self) -> AppResult<Vec<Provider>> {
        self.run(move |conn| {
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
        })
        .await
    }

    pub async fn add_provider(&self, provider: &SaveProvider) -> AppResult<Provider> {
        let provider = provider.clone();
        self.run(move |conn| {
            let provider = &provider;
            validate_provider(provider)?;
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
        })
        .await
    }

    pub async fn delete_provider(&self, id: i64) -> AppResult<()> {
        self.run(move |conn| {
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
        })
        .await
    }

    pub async fn create_share(
        &self,
        book_id: i64,
        token: &str,
        expires_at: Option<&str>,
    ) -> AppResult<Share> {
        let token = token.to_owned();
        let expires_at = expires_at.map(str::to_owned);
        self.run(move |conn| {
            let token = token.as_str();
            let expires_at = expires_at.as_deref();
            let now = Utc::now().to_rfc3339();
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
        })
        .await
    }

    pub async fn shares(&self, book_id: i64) -> AppResult<Vec<Share>> {
        self.run(move |conn| {
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
        })
        .await
    }

    pub async fn revoke_share(&self, id: i64) -> AppResult<()> {
        self.run(move |conn| {
            if conn.execute(
                "UPDATE shares SET revoked_at=?2 WHERE id=?1",
                params![id, Utc::now().to_rfc3339()],
            )? == 0
            {
                return Err(AppError::NotFound);
            }
            Ok(())
        })
        .await
    }

    pub async fn shared_book_path(&self, token: &str) -> AppResult<(i64, String)> {
        let token = token.to_owned();
        self.run(move |conn| {
            let token = token.as_str();
            conn.query_row(
                "SELECT b.id, b.relative_path FROM shares s JOIN books b ON b.id=s.book_id
             WHERE s.token=?1 AND s.revoked_at IS NULL
             AND (s.expires_at IS NULL OR s.expires_at > ?2) AND b.is_available=1",
                params![token, Utc::now().to_rfc3339()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or(AppError::NotFound)
        })
        .await
    }
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
    crate::metadata::validate_public_http_url(&provider.base_url)?;
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
    metadata_scanned INTEGER NOT NULL DEFAULT 0,
    metadata_edited INTEGER NOT NULL DEFAULT 0,
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
    if !columns.iter().any(|column| column == "metadata_scanned") {
        connection.execute(
            "ALTER TABLE books ADD COLUMN metadata_scanned INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !columns.iter().any(|column| column == "metadata_edited") {
        // Em catalogos antigos nao ha origem dos metadados: preserve todos.
        // O default permanece 0 para livros novos apos a migracao.
        connection.execute(
            "ALTER TABLE books ADD COLUMN metadata_edited INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
        connection.execute("UPDATE books SET metadata_edited=1", [])?;
    }
    connection.execute(
        "CREATE INDEX IF NOT EXISTS idx_books_fingerprint ON books(content_hash, size)",
        [],
    )?;
    Ok(())
}

/// Rejeita objetos executaveis e schemas incompletos antes de migrar o pacote.
fn validate_restore_schema(candidate: &Connection) -> AppResult<()> {
    let reference = Connection::open_in_memory()?;
    reference.execute_batch(SCHEMA)?;
    migrate(&reference)?;
    let mut objects =
        candidate.prepare("SELECT type, name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'")?;
    for row in objects.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })? {
        let (kind, name) = row?;
        let known: bool = reference.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type=?1 AND name=?2)",
            params![kind, name],
            |row| row.get(0),
        )?;
        if !known || !matches!(kind.as_str(), "table" | "index") {
            return Err(AppError::BadRequest(
                "backup contem objetos SQL nao reconhecidos".into(),
            ));
        }
    }
    let mut tables = reference.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
    for table in tables.query_map([], |row| row.get::<_, String>(0))? {
        let table = table?;
        let mut fields = reference.prepare(&format!("PRAGMA table_info({table})"))?;
        let fields = fields
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        let required: Vec<_> = fields
            .iter()
            .filter(|name| {
                !matches!(
                    name.as_str(),
                    "content_hash" | "metadata_scanned" | "metadata_edited"
                )
            })
            .cloned()
            .collect();
        candidate
            .prepare(&format!(
                "SELECT {} FROM {table} LIMIT 0",
                required.join(",")
            ))
            .map_err(|_| AppError::BadRequest(format!("schema incompleto na tabela {table}")))?;
    }
    Ok(())
}

/// Verifica referencias, caminhos e JSON consumidos pelo catalogo e leitores.
fn validate_restore_data(candidate: &Connection) -> AppResult<()> {
    // SaveNote antigo aceita location omitido (JSON null).
    candidate.execute(
        "UPDATE notes SET location='{}' WHERE TRIM(location)='null'",
        [],
    )?;
    if candidate
        .prepare("PRAGMA foreign_key_check")?
        .query([])?
        .next()?
        .is_some()
    {
        return Err(AppError::BadRequest(
            "backup contem referencias invalidas".into(),
        ));
    }
    let mut books =
        candidate.prepare("SELECT relative_path, cover_filename, subjects FROM books")?;
    for row in books.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, String>(2)?,
        ))
    })? {
        let (relative, cover, subjects) = row?;
        crate::scanner::path_to_safe_relative(Path::new(&relative))?;
        if let Some(cover) = cover {
            if Path::new(&cover).file_name().and_then(|name| name.to_str()) != Some(cover.as_str())
                || cover.contains('\\')
            {
                return Err(AppError::BadRequest(
                    "caminho de capa invalido no backup".into(),
                ));
            }
        }
        serde_json::from_str::<Vec<String>>(&subjects)
            .map_err(|_| AppError::BadRequest("assuntos invalidos no backup".into()))?;
    }
    for table in ["reading_progress", "notes"] {
        let mut stmt = candidate.prepare(&format!("SELECT location FROM {table}"))?;
        for value in stmt.query_map([], |row| row.get::<_, String>(0))? {
            let value = value?;
            let location: serde_json::Value = serde_json::from_str(&value)
                .map_err(|_| AppError::BadRequest("posicao invalida no backup".into()))?;
            validate_location(&location)?;
        }
    }
    let invalid: bool = candidate.query_row("SELECT EXISTS(SELECT 1 FROM reading_progress WHERE percent IS NULL OR typeof(percent) NOT IN ('integer','real') OR percent < 0 OR percent > 100)", [], |row| row.get(0))?;
    if invalid {
        return Err(AppError::BadRequest("progresso invalido no backup".into()));
    }
    Ok(())
}

fn open_connection(path: &Path) -> AppResult<Connection> {
    let connection = Connection::open(path)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.execute_batch(SCHEMA)?;
    migrate(&connection)?;
    Ok(connection)
}

#[cfg(test)]
mod restore_tests {
    use super::*;

    #[test]
    fn restore_validation_rejects_triggers_and_unsafe_book_paths() {
        let candidate = Connection::open_in_memory().unwrap();
        candidate.execute_batch(SCHEMA).unwrap();
        migrate(&candidate).unwrap();
        validate_restore_schema(&candidate).unwrap();
        validate_restore_data(&candidate).unwrap();
        candidate
            .execute_batch(
                "CREATE TRIGGER unexpected AFTER INSERT ON settings BEGIN DELETE FROM books; END;",
            )
            .unwrap();
        assert!(validate_restore_schema(&candidate).is_err());
        candidate.execute_batch("DROP TRIGGER unexpected;").unwrap();
        candidate.execute("INSERT INTO books(title,filename,relative_path,format,size,modified_at,created_at,updated_at) VALUES('test','test.txt','../test.txt','txt',1,1,'now','now')", []).unwrap();
        assert!(validate_restore_data(&candidate).is_err());
        candidate
            .execute(
                "UPDATE books SET relative_path='test.txt', subjects='not JSON'",
                [],
            )
            .unwrap();
        assert!(validate_restore_data(&candidate).is_err());
    }

    #[tokio::test]
    async fn richer_scan_enriches_once_and_preserves_manual_edits() {
        use crate::scanner::{scan, ScanProfile};
        use std::io::Write;
        let root = std::path::PathBuf::from(format!(".test-profile-{}", rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        {
            let mut archive =
                zip::ZipWriter::new(std::fs::File::create(root.join("fallback.epub")).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            archive
                .start_file("META-INF/container.xml", options)
                .unwrap();
            archive
                .write_all(b"<container><rootfile full-path='book.opf'/></container>")
                .unwrap();
            archive.start_file("book.opf", options).unwrap();
            archive.write_all(b"<package><metadata><title>Embedded title</title><creator>Embedded author</creator></metadata></package>").unwrap();
            archive.finish().unwrap();
        }
        let db = Database::open(&root.join("catalog.db"), &root)
            .await
            .unwrap();
        let (books, _, _) = scan(&root, ScanProfile::Economical, &HashMap::new(), |_| {}).unwrap();
        assert!(!books[0].metadata_scanned);
        db.sync_scan(&books).await.unwrap();
        let (books, _, _) = scan(
            &root,
            ScanProfile::Complete,
            &db.scan_index().await.unwrap(),
            |_| {},
        )
        .unwrap();
        assert!(!books[0].unchanged);
        assert!(books[0].metadata_scanned);
        db.sync_scan(&books).await.unwrap();
        let book = db
            .list_books(&CatalogQuery::default())
            .await
            .unwrap()
            .books
            .remove(0);
        assert_eq!(book.title, "Embedded title");
        assert_eq!(book.author.as_deref(), Some("Embedded author"));
        let edit: UpdateBook = serde_json::from_value(
            serde_json::json!({"title": "Manual title", "author": "Manual author"}),
        )
        .unwrap();
        db.update_book(book.id, &edit).await.unwrap();
        let mut known = db.scan_index().await.unwrap();
        known.get_mut("fallback.epub").unwrap().metadata_scanned = false;
        let (books, _, _) = scan(&root, ScanProfile::Balanced, &known, |_| {}).unwrap();
        db.sync_scan(&books).await.unwrap();
        assert_eq!(db.book(book.id).await.unwrap().title, "Manual title");
        assert_eq!(
            db.book(book.id).await.unwrap().author.as_deref(),
            Some("Manual author")
        );
        let (books, _, _) = scan(
            &root,
            ScanProfile::Complete,
            &db.scan_index().await.unwrap(),
            |_| {},
        )
        .unwrap();
        assert!(books[0].unchanged);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn restore_keeps_connection_usable_after_success_and_invalid_schema() {
        let root = std::path::PathBuf::from(format!(".test-db-restore-{}", rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        let db = Database::open(&root.join("live.db"), &root).await.unwrap();
        db.set_setting("test-marker", "snapshot").await.unwrap();
        let snapshot = root.join("snapshot.db");
        db.snapshot_to(&snapshot).await.unwrap();
        db.set_setting("test-marker", "changed").await.unwrap();
        db.replace_from(&snapshot).await.unwrap();
        assert_eq!(
            db.setting("test-marker").await.unwrap().as_deref(),
            Some("snapshot")
        );
        let invalid = root.join("invalid.db");
        {
            let candidate = Connection::open(&invalid).unwrap();
            candidate
                .execute_batch("CREATE TABLE books (invalid_column TEXT);")
                .unwrap();
        }
        assert!(db.replace_from(&invalid).await.is_err());
        assert_eq!(
            db.setting("test-marker").await.unwrap().as_deref(),
            Some("snapshot")
        );
        db.set_setting("test-marker", "still-writable")
            .await
            .unwrap();
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
#[path = "../tests/rust/catalog-regression.rs"]
mod regression_tests;

#[cfg(test)]
#[path = "../tests/rust/db-worker.rs"]
mod worker_tests;

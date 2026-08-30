use std::path::{Path, PathBuf};

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path as AxumPath, Query, State},
    http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{delete, get, post, put},
    Json, Router,
};
use chrono::{Duration, Utc};
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;

use crate::{
    config::Config,
    db::Database,
    error::{AppError, AppResult},
    metadata,
    models::{CreateShare, MetadataCandidate, SaveNote, SaveProgress, SaveProvider, UpdateBook},
    readers,
    scanner::{self, ScanReport, SUPPORTED_EXTENSIONS},
};

#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub config: Config,
    pub http: reqwest::Client,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/settings", get(get_settings).put(save_settings))
        .route("/scan", post(scan_library))
        .route(
            "/upload",
            post(upload_book).layer(DefaultBodyLimit::max(1024 * 1024 * 1024)),
        )
        .route("/books", get(list_books))
        .route("/books/{id}", get(get_book).patch(update_book))
        .route("/books/{id}/file", get(book_file))
        .route("/books/{id}/cover", get(book_cover))
        .route("/books/{id}/progress", put(save_progress))
        .route("/books/{id}/notes", get(list_notes).post(create_note))
        .route("/notes/{id}", delete(delete_note))
        .route("/books/{id}/epub", get(epub_manifest))
        .route("/books/{id}/epub/resource/{*resource}", get(epub_resource))
        .route("/books/{id}/comic", get(comic_manifest))
        .route(
            "/books/{id}/comic/resource/{*resource}",
            get(comic_resource),
        )
        .route("/books/{id}/text", get(text_reader))
        .route("/books/{id}/mobi", get(mobi_reader))
        .route(
            "/metadata/providers",
            get(list_providers).post(add_provider),
        )
        .route("/metadata/providers/{id}", delete(delete_provider))
        .route("/metadata/search", get(search_metadata))
        .route("/books/{id}/metadata", put(apply_metadata))
        .route("/books/{id}/shares", get(list_shares).post(create_share))
        .route("/shares/{id}", delete(revoke_share))
        .route("/public/{token}/file", get(shared_file))
        .layer(middleware::from_fn(enforce_same_origin))
        .with_state(state)
}

/// Impede que uma pagina de outro site altere uma biblioteca exposta em localhost/LAN.
async fn enforce_same_origin(request: Request<Body>, next: Next) -> Result<Response, StatusCode> {
    if matches!(
        *request.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    ) {
        if let Some(origin) = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
        {
            let origin_authority = url::Url::parse(origin).ok().and_then(|url| {
                url.host_str()
                    .map(|host| (host.to_ascii_lowercase(), url.port()))
            });
            let request_authority = request
                .headers()
                .get(header::HOST)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| url::Url::parse(&format!("http://{value}")).ok())
                .and_then(|url| {
                    url.host_str()
                        .map(|host| (host.to_ascii_lowercase(), url.port()))
                });
            if origin_authority.is_none() || origin_authority != request_authority {
                return Err(StatusCode::FORBIDDEN);
            }
        }
    }
    Ok(next.run(request).await)
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "offline_capable": true }))
}

#[derive(Serialize)]
struct SettingsResponse {
    library_root: String,
    network_metadata_enabled: bool,
    supported_formats: &'static [&'static str],
}

#[derive(Deserialize)]
struct SaveSettings {
    library_root: String,
    network_metadata_enabled: bool,
}

async fn get_settings(State(state): State<AppState>) -> AppResult<Json<SettingsResponse>> {
    Ok(Json(SettingsResponse {
        library_root: library_root(&state).await?.to_string_lossy().into_owned(),
        network_metadata_enabled: network_enabled(&state).await?,
        supported_formats: SUPPORTED_EXTENSIONS,
    }))
}

async fn save_settings(
    State(state): State<AppState>,
    Json(input): Json<SaveSettings>,
) -> AppResult<Json<Value>> {
    let root = PathBuf::from(input.library_root.trim());
    if !root.is_dir() {
        return Err(AppError::BadRequest(
            "a pasta informada nao existe dentro do servidor/container".into(),
        ));
    }
    let canonical = root.canonicalize()?;
    state
        .db
        .set_setting("library_root", &canonical.to_string_lossy())
        .await?;
    state
        .db
        .set_setting(
            "network_metadata_enabled",
            if input.network_metadata_enabled {
                "true"
            } else {
                "false"
            },
        )
        .await?;
    Ok(Json(json!({ "saved": true })))
}

async fn scan_library(State(state): State<AppState>) -> AppResult<Json<ScanReport>> {
    let root = library_root(&state).await?;
    let (books, skipped, warnings) = tokio::task::spawn_blocking(move || scanner::scan(&root))
        .await
        .map_err(|error| AppError::Internal(error.to_string()))??;
    let found = books.len();
    let (added, updated) = state.db.sync_scan(&books).await?;
    Ok(Json(ScanReport {
        found,
        added,
        updated,
        skipped,
        warnings,
    }))
}

async fn upload_book(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> AppResult<Json<Value>> {
    let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?
    else {
        return Err(AppError::BadRequest("nenhum arquivo enviado".into()));
    };
    let original = field
        .file_name()
        .ok_or_else(|| AppError::BadRequest("arquivo sem nome".into()))?;
    let filename = Path::new(original)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::BadRequest("nome de arquivo invalido".into()))?
        .to_string();
    let extension = Path::new(&filename)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !SUPPORTED_EXTENSIONS.contains(&extension.as_str()) {
        return Err(AppError::BadRequest(format!(
            "formato .{extension} nao suportado"
        )));
    }
    let root = library_root(&state).await?;
    let destination = unique_destination(&root, &filename).await?;
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .await?;
    let mut written = 0_u64;
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?
    {
        written += chunk.len() as u64;
        if written > 1024 * 1024 * 1024 {
            drop(output);
            let _ = tokio::fs::remove_file(&destination).await;
            return Err(AppError::BadRequest("o arquivo excede 1 GiB".into()));
        }
        if let Err(error) = output.write_all(&chunk).await {
            drop(output);
            let _ = tokio::fs::remove_file(&destination).await;
            return Err(error.into());
        }
    }
    output.flush().await?;
    let saved_as = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&filename)
        .to_string();
    Ok(Json(
        json!({ "saved": true, "filename": saved_as, "bytes": written }),
    ))
}

#[derive(Deserialize)]
struct SearchQuery {
    q: Option<String>,
}

async fn list_books(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        json!({ "books": state.db.list_books(query.q.as_deref()).await? }),
    ))
}

async fn get_book(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "book": state.db.book(id).await? })))
}

async fn update_book(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
    Json(input): Json<UpdateBook>,
) -> AppResult<Json<Value>> {
    state.db.update_book(id, &input).await?;
    Ok(Json(json!({ "book": state.db.book(id).await? })))
}

async fn book_file(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
    headers: HeaderMap,
    Query(query): Query<FileQuery>,
) -> AppResult<Response> {
    let book = state.db.book(id).await?;
    let path = resolve_path(&state, &book.relative_path).await?;
    stream_file(
        path,
        &book.filename,
        query.download.unwrap_or(false),
        &headers,
    )
    .await
}

async fn book_cover(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> AppResult<Response> {
    let filename = state.db.cover_filename(id).await?;
    let path = state.config.covers_dir.join(filename);
    if !path.is_file() {
        return Err(AppError::NotFound);
    }
    stream_file(path, "cover", false, &HeaderMap::new()).await
}

#[derive(Deserialize)]
struct FileQuery {
    download: Option<bool>,
}

async fn save_progress(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
    Json(input): Json<SaveProgress>,
) -> AppResult<Json<Value>> {
    state.db.book(id).await?;
    state.db.save_progress(id, &input).await?;
    Ok(Json(json!({ "saved": true })))
}

async fn list_notes(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "notes": state.db.notes(id).await? })))
}

async fn create_note(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
    Json(input): Json<SaveNote>,
) -> AppResult<(StatusCode, Json<Value>)> {
    state.db.book(id).await?;
    let note = state.db.create_note(id, &input).await?;
    Ok((StatusCode::CREATED, Json(json!({ "note": note }))))
}

async fn delete_note(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> AppResult<StatusCode> {
    state.db.delete_note(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn epub_manifest(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> AppResult<Json<Value>> {
    let path = book_path_by_format(&state, id, &["epub"]).await?;
    let manifest = tokio::task::spawn_blocking(move || readers::epub_manifest(&path))
        .await
        .map_err(|error| AppError::Internal(error.to_string()))??;
    Ok(Json(json!({ "manifest": manifest })))
}

async fn epub_resource(
    State(state): State<AppState>,
    AxumPath((id, resource)): AxumPath<(i64, String)>,
) -> AppResult<Response> {
    let path = book_path_by_format(&state, id, &["epub"]).await?;
    archive_response(path, resource, 32 * 1024 * 1024).await
}

async fn comic_manifest(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> AppResult<Json<Value>> {
    let path = book_path_by_format(&state, id, &["cbz"]).await?;
    let manifest = tokio::task::spawn_blocking(move || readers::comic_manifest(&path))
        .await
        .map_err(|error| AppError::Internal(error.to_string()))??;
    Ok(Json(json!({ "manifest": manifest })))
}

async fn comic_resource(
    State(state): State<AppState>,
    AxumPath((id, resource)): AxumPath<(i64, String)>,
) -> AppResult<Response> {
    let path = book_path_by_format(&state, id, &["cbz"]).await?;
    archive_response(path, resource, 64 * 1024 * 1024).await
}

async fn text_reader(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> AppResult<Response> {
    let book = state.db.book(id).await?;
    if !matches!(book.format.as_str(), "txt" | "md" | "html" | "htm" | "fb2") {
        return Err(AppError::Unsupported(book.format));
    }
    let path = resolve_path(&state, &book.relative_path).await?;
    let format = book.format;
    let bytes = tokio::task::spawn_blocking(move || readers::text_html(&path, &format))
        .await
        .map_err(|error| AppError::Internal(error.to_string()))??;
    Ok(sandboxed_html(bytes))
}

async fn mobi_reader(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> AppResult<Response> {
    let path = book_path_by_format(&state, id, &["mobi", "azw", "azw3"]).await?;
    let bytes = tokio::task::spawn_blocking(move || readers::mobi_html(&path))
        .await
        .map_err(|error| AppError::Internal(error.to_string()))??;
    Ok(sandboxed_html(bytes))
}

async fn list_providers(State(state): State<AppState>) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "providers": state.db.providers().await? })))
}

async fn add_provider(
    State(state): State<AppState>,
    Json(input): Json<SaveProvider>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let provider = state.db.add_provider(&input).await?;
    Ok((StatusCode::CREATED, Json(json!({ "provider": provider }))))
}

async fn delete_provider(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> AppResult<StatusCode> {
    state.db.delete_provider(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct MetadataQuery {
    provider_id: i64,
    q: String,
}

async fn search_metadata(
    State(state): State<AppState>,
    Query(query): Query<MetadataQuery>,
) -> AppResult<Json<Value>> {
    if !network_enabled(&state).await? {
        return Err(AppError::NetworkDisabled);
    }
    let provider = state
        .db
        .providers()
        .await?
        .into_iter()
        .find(|provider| provider.id == query.provider_id && provider.enabled)
        .ok_or(AppError::NotFound)?;
    let candidates = metadata::search(
        &state.http,
        &provider,
        &query.q,
        state.config.google_books_api_key.as_deref(),
    )
    .await?;
    Ok(Json(
        json!({ "results": candidates, "disclosure": "Somente o texto da busca foi enviado a fonte selecionada." }),
    ))
}

async fn apply_metadata(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
    Json(candidate): Json<MetadataCandidate>,
) -> AppResult<Json<Value>> {
    let update = UpdateBook {
        title: candidate.title,
        author: optional_join(candidate.authors),
        description: candidate.description,
        publisher: candidate.publisher,
        published_date: candidate.published_date,
        isbn: candidate.isbn,
        language: candidate.language,
        subjects: candidate.subjects,
    };
    state.db.update_book(id, &update).await?;
    let mut cover_saved = false;
    if let Some(url) = candidate.cover_url {
        if !network_enabled(&state).await? {
            return Err(AppError::NetworkDisabled);
        }
        let (bytes, extension) = metadata::download_cover(&state.http, &url).await?;
        tokio::fs::create_dir_all(&state.config.covers_dir).await?;
        let filename = format!("book-{id}.{extension}");
        tokio::fs::write(state.config.covers_dir.join(&filename), bytes).await?;
        state.db.set_cover(id, &filename).await?;
        cover_saved = true;
    }
    Ok(Json(
        json!({ "book": state.db.book(id).await?, "cover_saved": cover_saved }),
    ))
}

async fn create_share(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
    Json(input): Json<CreateShare>,
) -> AppResult<(StatusCode, Json<Value>)> {
    state.db.book(id).await?;
    let token: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(48)
        .map(char::from)
        .collect();
    let expires = input
        .expires_in_hours
        .map(|hours| (Utc::now() + Duration::hours(i64::from(hours.clamp(1, 8760)))).to_rfc3339());
    let share = state
        .db
        .create_share(id, &token, expires.as_deref())
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "share": share, "path": format!("/api/public/{token}/file") })),
    ))
}

async fn list_shares(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "shares": state.db.shares(id).await? })))
}

async fn revoke_share(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> AppResult<StatusCode> {
    state.db.revoke_share(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn shared_file(
    State(state): State<AppState>,
    AxumPath(token): AxumPath<String>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let (_, relative) = state.db.shared_book_path(&token).await?;
    let path = resolve_path(&state, &relative).await?;
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("livro")
        .to_string();
    stream_file(path, &filename, true, &headers).await
}

async fn library_root(state: &AppState) -> AppResult<PathBuf> {
    state
        .db
        .setting("library_root")
        .await?
        .map(PathBuf::from)
        .ok_or_else(|| AppError::Internal("pasta da biblioteca nao configurada".into()))
}

async fn network_enabled(state: &AppState) -> AppResult<bool> {
    Ok(state
        .db
        .setting("network_metadata_enabled")
        .await?
        .as_deref()
        == Some("true"))
}

async fn resolve_path(state: &AppState, relative: &str) -> AppResult<PathBuf> {
    let root = library_root(state).await?;
    let relative = relative.to_string();
    tokio::task::spawn_blocking(move || scanner::resolve_book_path(&root, &relative))
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?
}

async fn book_path_by_format(state: &AppState, id: i64, formats: &[&str]) -> AppResult<PathBuf> {
    let book = state.db.book(id).await?;
    if !formats.contains(&book.format.as_str()) {
        return Err(AppError::Unsupported(book.format));
    }
    resolve_path(state, &book.relative_path).await
}

async fn unique_destination(root: &Path, filename: &str) -> AppResult<PathBuf> {
    let original = Path::new(filename);
    let stem = original
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("livro");
    let extension = original
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    for suffix in 0..10_000 {
        let candidate_name = if suffix == 0 {
            filename.to_string()
        } else {
            format!("{stem}-{suffix}.{extension}")
        };
        let candidate = root.join(candidate_name);
        if !tokio::fs::try_exists(&candidate).await? {
            return Ok(candidate);
        }
    }
    Err(AppError::Internal(
        "nao foi possivel escolher nome para o upload".into(),
    ))
}

async fn archive_response(path: PathBuf, resource: String, max_size: u64) -> AppResult<Response> {
    let (bytes, mime) =
        tokio::task::spawn_blocking(move || readers::archive_resource(&path, &resource, max_size))
            .await
            .map_err(|error| AppError::Internal(error.to_string()))??;
    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&mime)
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    add_sandbox_headers(response.headers_mut());
    Ok(response)
}

fn sandboxed_html(bytes: Vec<u8>) -> Response {
    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    add_sandbox_headers(response.headers_mut());
    response
}

fn add_sandbox_headers(headers: &mut HeaderMap) {
    headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox; default-src 'self' data: blob:; script-src 'none'; connect-src 'none'; object-src 'none'; form-action 'none'; base-uri 'none'; navigate-to 'none'"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=3600"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
}

async fn stream_file(
    path: PathBuf,
    filename: &str,
    download: bool,
    headers: &HeaderMap,
) -> AppResult<Response> {
    let mut file = tokio::fs::File::open(&path).await?;
    let size = file.metadata().await?.len();
    let range = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| parse_range(value, size));
    let (status, start, end) = range
        .map(|(start, end)| (StatusCode::PARTIAL_CONTENT, start, end))
        .unwrap_or((StatusCode::OK, 0, size.saturating_sub(1)));
    let length = if size == 0 { 0 } else { end - start + 1 };
    file.seek(std::io::SeekFrom::Start(start)).await?;
    let stream = ReaderStream::new(file.take(length));
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = status;
    let mime = mime_guess::from_path(&path)
        .first_or_octet_stream()
        .to_string();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&mime)
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    response
        .headers_mut()
        .insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&length.to_string()).unwrap(),
    );
    if status == StatusCode::PARTIAL_CONTENT {
        response.headers_mut().insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {start}-{end}/{size}")).unwrap(),
        );
    }
    let safe_filename = filename.replace(['"', '\r', '\n'], "_");
    let disposition = if download { "attachment" } else { "inline" };
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("{disposition}; filename=\"{safe_filename}\""))
            .unwrap_or(HeaderValue::from_static("attachment")),
    );
    Ok(response)
}

fn parse_range(value: &str, size: u64) -> Option<(u64, u64)> {
    if size == 0 {
        return None;
    }
    let range = value.strip_prefix("bytes=")?;
    if range.contains(',') {
        return None;
    }
    let (start, end) = range.split_once('-')?;
    if start.is_empty() {
        let suffix: u64 = end.parse().ok()?;
        if suffix == 0 {
            return None;
        }
        return Some((size.saturating_sub(suffix.min(size)), size - 1));
    }
    let start: u64 = start.parse().ok()?;
    if start >= size {
        return None;
    }
    let end = if end.is_empty() {
        size - 1
    } else {
        end.parse::<u64>().ok()?.min(size - 1)
    };
    (start <= end).then_some((start, end))
}

fn optional_join(values: Vec<String>) -> Option<String> {
    let value = values
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_http_ranges() {
        assert_eq!(parse_range("bytes=0-99", 200), Some((0, 99)));
        assert_eq!(parse_range("bytes=100-", 200), Some((100, 199)));
        assert_eq!(parse_range("bytes=-20", 200), Some((180, 199)));
        assert_eq!(parse_range("bytes=300-", 200), None);
    }
}

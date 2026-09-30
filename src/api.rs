use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, OriginalUri, Path as AxumPath, Query, State},
    http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
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
    auth, backup,
    config::Config,
    covers,
    db::Database,
    error::{AppError, AppResult},
    metadata,
    models::{
        CatalogQuery, CreateShare, MetadataCandidate, SaveCollection, SaveNote, SaveProgress,
        SaveProvider, UpdateBook,
    },
    opds, readers,
    scanner::{self, ScanEvent, ScanProfile, ScanStatus, SUPPORTED_EXTENSIONS},
};

#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub config: Config,
    pub http: reqwest::Client,
    pub scan_status: Arc<Mutex<ScanStatus>>,
}

pub fn router(state: AppState) -> Router {
    let auth_config = state.config.clone();
    let maintenance_gate = Arc::new(tokio::sync::RwLock::new(()));
    Router::new()
        .route("/health", get(health))
        .route("/settings", get(get_settings).put(save_settings))
        .route("/scan", get(get_scan_status).post(start_scan))
        .route(
            "/settings/branding/{kind}",
            get(branding_asset)
                .post(upload_branding)
                .delete(delete_branding)
                .layer(DefaultBodyLimit::max(6 * 1024 * 1024)),
        )
        .route(
            "/upload",
            post(upload_book).layer(DefaultBodyLimit::max(1024 * 1024 * 1024)),
        )
        .route("/books", get(list_books))
        .route("/reading-desk", get(reading_desk))
        .route("/suggestions", get(suggestions))
        .route("/books/{id}", get(get_book).patch(update_book))
        .route("/books/{id}/file", get(book_file).delete(delete_book_file))
        .route("/books/{id}/cover", get(book_cover))
        .route(
            "/books/{id}/progress",
            put(save_progress).delete(remove_progress),
        )
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
        .route(
            "/collections",
            get(list_collections).post(create_collection),
        )
        .route(
            "/collections/{id}",
            put(update_collection).delete(delete_collection),
        )
        .route(
            "/collections/{collection_id}/books/{book_id}",
            put(add_book_to_collection).delete(remove_book_from_collection),
        )
        .route("/maintenance/health", get(library_health))
        .route("/maintenance/duplicates", get(list_duplicates))
        .route(
            "/maintenance/backups",
            get(list_backups).post(create_backup),
        )
        .route(
            "/maintenance/backups/{filename}",
            get(download_backup).delete(delete_backup),
        )
        .route(
            "/maintenance/restore",
            post(restore_backup).layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024)),
        )
        .route("/maintenance/covers", post(start_cover_generation))
        .route("/opds", get(opds_catalog))
        .route("/opds/import", post(import_opds))
        .layer(middleware::from_fn_with_state(
            maintenance_gate,
            coordinate_restore,
        ))
        .layer(middleware::from_fn(enforce_same_origin))
        .layer(middleware::from_fn_with_state(
            auth_config,
            auth::require_auth,
        ))
        .with_state(state)
}

/// Espera as operacoes HTTP em curso antes de restaurar e impede que novos
/// handlers observem banco e assets em etapas diferentes da troca.
async fn coordinate_restore(
    State(gate): State<Arc<tokio::sync::RwLock<()>>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if request.method() == Method::POST && request.uri().path() == "/maintenance/restore" {
        let _exclusive = gate.write().await;
        next.run(request).await
    } else {
        let _shared = gate.read().await;
        next.run(request).await
    }
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
    scan_profile: String,
    carousel_enabled: bool,
    carousel_interval_seconds: u64,
    auto_cover_enabled: bool,
    auth_enabled: bool,
    branding: BrandingAssets,
    supported_formats: &'static [&'static str],
}

#[derive(Serialize)]
struct BrandingAssets {
    logo: Option<String>,
    favicon: Option<String>,
    hero: Option<String>,
}

#[derive(Deserialize)]
struct SaveSettings {
    library_root: String,
    #[serde(default)]
    network_metadata_enabled: bool,
    #[serde(default = "default_scan_profile")]
    scan_profile: String,
    #[serde(default)]
    carousel_enabled: bool,
    #[serde(default = "default_carousel_interval")]
    carousel_interval_seconds: u64,
    #[serde(default)]
    auto_cover_enabled: bool,
}

fn default_scan_profile() -> String {
    "economical".into()
}

fn default_carousel_interval() -> u64 {
    12
}

async fn get_settings(State(state): State<AppState>) -> AppResult<Json<SettingsResponse>> {
    Ok(Json(SettingsResponse {
        library_root: library_root(&state).await?.to_string_lossy().into_owned(),
        network_metadata_enabled: network_enabled(&state).await?,
        scan_profile: configured_scan_profile(&state).await?.as_str().into(),
        carousel_enabled: setting_bool(&state, "carousel_enabled").await?,
        carousel_interval_seconds: state
            .db
            .setting("carousel_interval_seconds")
            .await?
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(default_carousel_interval),
        auto_cover_enabled: setting_bool(&state, "auto_cover_enabled").await?,
        auth_enabled: state.config.auth_enabled(),
        branding: branding_assets(&state).await?,
        supported_formats: SUPPORTED_EXTENSIONS,
    }))
}

async fn save_settings(
    State(state): State<AppState>,
    Json(input): Json<SaveSettings>,
) -> AppResult<Json<Value>> {
    let scan_profile = ScanProfile::parse(input.scan_profile.trim())
        .ok_or_else(|| AppError::BadRequest("perfil de varredura invalido".into()))?;
    if !(5..=300).contains(&input.carousel_interval_seconds) {
        return Err(AppError::BadRequest(
            "o intervalo do carrossel deve ficar entre 5 e 300 segundos".into(),
        ));
    }
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
            "auto_cover_enabled",
            if input.auto_cover_enabled {
                "true"
            } else {
                "false"
            },
        )
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
    state
        .db
        .set_setting("scan_profile", scan_profile.as_str())
        .await?;
    state
        .db
        .set_setting(
            "carousel_enabled",
            if input.carousel_enabled {
                "true"
            } else {
                "false"
            },
        )
        .await?;
    state
        .db
        .set_setting(
            "carousel_interval_seconds",
            &input.carousel_interval_seconds.to_string(),
        )
        .await?;
    Ok(Json(json!({ "saved": true })))
}

async fn branding_assets(state: &AppState) -> AppResult<BrandingAssets> {
    Ok(BrandingAssets {
        logo: branding_url(state, "logo").await?,
        favicon: branding_url(state, "favicon").await?,
        hero: branding_url(state, "hero").await?,
    })
}

async fn branding_url(state: &AppState, kind: &str) -> AppResult<Option<String>> {
    let key = branding_key(kind)?;
    Ok(state.db.setting(key).await?.and_then(|filename| {
        safe_branding_path(&state.config.branding_dir, &filename)
            .map(|_| format!("/api/settings/branding/{kind}?v={filename}"))
    }))
}

async fn branding_asset(
    State(state): State<AppState>,
    AxumPath(kind): AxumPath<String>,
) -> AppResult<Response> {
    let key = branding_key(&kind)?;
    let filename = state.db.setting(key).await?.ok_or(AppError::NotFound)?;
    let path = safe_branding_path(&state.config.branding_dir, &filename)
        .ok_or_else(|| AppError::Internal("arquivo de identidade visual invalido".into()))?;
    if !path.is_file() {
        return Err(AppError::NotFound);
    }
    stream_file(path, &filename, false, &HeaderMap::new()).await
}

async fn upload_branding(
    State(state): State<AppState>,
    AxumPath(kind): AxumPath<String>,
    mut multipart: Multipart,
) -> AppResult<Json<Value>> {
    let key = branding_key(&kind)?;
    let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?
    else {
        return Err(AppError::BadRequest("nenhuma imagem enviada".into()));
    };
    let mut bytes = Vec::new();
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?
    {
        if bytes.len() + chunk.len() > 5 * 1024 * 1024 {
            return Err(AppError::BadRequest(
                "a imagem de identidade visual deve ter no maximo 5 MiB".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    let extension = detect_image_extension(&bytes).ok_or_else(|| {
        AppError::BadRequest("use uma imagem PNG, JPEG, WebP, GIF ou ICO valida".into())
    })?;
    tokio::fs::create_dir_all(&state.config.branding_dir).await?;
    let filename = format!(
        "{kind}-{}-{}.{}",
        Utc::now().timestamp_millis(),
        rand::thread_rng().gen_range(1000_u16..9999_u16),
        extension
    );
    let path = state.config.branding_dir.join(&filename);
    let temporary = state
        .config
        .branding_dir
        .join(format!(".{filename}.upload"));
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .await?;
    output.write_all(&bytes).await?;
    output.flush().await?;
    drop(output);
    tokio::fs::rename(&temporary, &path).await?;

    let previous = state.db.setting(key).await?;
    if let Err(error) = state.db.set_setting(key, &filename).await {
        let _ = tokio::fs::remove_file(&path).await;
        return Err(error);
    }
    if let Some(previous) = previous {
        if let Some(previous_path) = safe_branding_path(&state.config.branding_dir, &previous) {
            if previous_path != path {
                let _ = tokio::fs::remove_file(previous_path).await;
            }
        }
    }
    Ok(Json(json!({
        "saved": true,
        "url": format!("/api/settings/branding/{kind}?v={filename}")
    })))
}

async fn delete_branding(
    State(state): State<AppState>,
    AxumPath(kind): AxumPath<String>,
) -> AppResult<StatusCode> {
    let key = branding_key(&kind)?;
    let previous = state.db.setting(key).await?;
    state.db.delete_setting(key).await?;
    if let Some(previous) = previous {
        if let Some(path) = safe_branding_path(&state.config.branding_dir, &previous) {
            match tokio::fs::remove_file(path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

fn branding_key(kind: &str) -> AppResult<&'static str> {
    match kind {
        "logo" => Ok("branding_logo"),
        "favicon" => Ok("branding_favicon"),
        "hero" => Ok("branding_hero"),
        _ => Err(AppError::BadRequest(
            "tipo de identidade visual invalido".into(),
        )),
    }
}

fn safe_branding_path(root: &Path, filename: &str) -> Option<PathBuf> {
    let path = Path::new(filename);
    if path.components().count() != 1 || path.file_name()?.to_str()? != filename {
        return None;
    }
    Some(root.join(path))
}

fn detect_image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        Some("ico")
    } else {
        None
    }
}

async fn get_scan_status(State(state): State<AppState>) -> AppResult<Json<ScanStatus>> {
    let status = state
        .scan_status
        .lock()
        .map_err(|_| AppError::Internal("estado da varredura indisponivel".into()))?
        .clone();
    Ok(Json(status))
}

async fn start_scan(State(state): State<AppState>) -> AppResult<(StatusCode, Json<ScanStatus>)> {
    let root = library_root(&state).await?;
    let profile = configured_scan_profile(&state).await?;
    let status = {
        let mut current = state
            .scan_status
            .lock()
            .map_err(|_| AppError::Internal("estado da varredura indisponivel".into()))?;
        if current.running {
            return Ok((StatusCode::OK, Json(current.clone())));
        }
        *current = ScanStatus {
            running: true,
            phase: "discovering".into(),
            profile: profile.as_str().into(),
            total: 0,
            processed: 0,
            percent: 0.0,
            found: 0,
            added: 0,
            updated: 0,
            unchanged: 0,
            skipped: 0,
            current_file: None,
            warnings: Vec::new(),
            error: None,
            started_at: Some(Utc::now().to_rfc3339()),
            finished_at: None,
        };
        current.clone()
    };
    let task_state = state.clone();
    tokio::spawn(async move {
        run_scan(task_state, root, profile).await;
    });
    Ok((StatusCode::ACCEPTED, Json(status)))
}

async fn run_scan(state: AppState, root: PathBuf, profile: ScanProfile) {
    let known_books = match state.db.scan_index().await {
        Ok(value) => value,
        Err(error) => {
            finish_scan_error(&state, error.to_string());
            return;
        }
    };
    let progress_state = state.scan_status.clone();
    let result = tokio::task::spawn_blocking(move || {
        scanner::scan(&root, profile, &known_books, |event| {
            let Ok(mut status) = progress_state.lock() else {
                return;
            };
            match event {
                ScanEvent::Discovered { total, skipped } => {
                    status.phase = "indexing".into();
                    status.total = total;
                    status.skipped = skipped;
                }
                ScanEvent::Indexed {
                    total,
                    processed,
                    found,
                    unchanged,
                    skipped,
                    current_file,
                } => {
                    status.total = total;
                    status.processed = processed;
                    status.percent = if total == 0 {
                        100.0
                    } else {
                        processed as f64 / total as f64 * 100.0
                    };
                    status.found = found;
                    status.unchanged = unchanged;
                    status.skipped = skipped;
                    status.current_file = Some(current_file);
                }
            }
        })
    })
    .await;

    let (books, skipped, warnings) = match result {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            finish_scan_error(&state, error.to_string());
            return;
        }
        Err(error) => {
            finish_scan_error(&state, error.to_string());
            return;
        }
    };
    if let Ok(mut status) = state.scan_status.lock() {
        status.phase = "synchronizing".into();
        status.current_file = None;
    }
    let found = books.len();
    match state.db.sync_scan(&books).await {
        Ok((added, updated, unchanged)) => {
            if let Err(error) = state.db.save_scan_issues(&warnings).await {
                finish_scan_error(&state, error.to_string());
                return;
            }
            if let Ok(mut status) = state.scan_status.lock() {
                status.found = found;
                status.added = added;
                status.updated = updated;
                status.unchanged = unchanged;
                status.skipped = skipped;
                status.warnings = warnings.clone();
            }
            if setting_bool(&state, "auto_cover_enabled")
                .await
                .unwrap_or(false)
            {
                if let Err(error) = generate_missing_covers(&state).await {
                    finish_scan_error(&state, error.to_string());
                    return;
                }
            }
            if let Ok(mut status) = state.scan_status.lock() {
                status.running = false;
                status.phase = "completed".into();
                status.percent = 100.0;
                status.processed = status.total;
                status.current_file = None;
                status.finished_at = Some(Utc::now().to_rfc3339());
            }
        }
        Err(error) => finish_scan_error(&state, error.to_string()),
    }
}

fn finish_scan_error(state: &AppState, message: String) {
    if let Ok(mut status) = state.scan_status.lock() {
        status.running = false;
        status.phase = "failed".into();
        status.error = Some(message);
        status.current_file = None;
        status.finished_at = Some(Utc::now().to_rfc3339());
    }
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
    let mut cleanup = crate::pending_file::PendingFile::default();
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .await?;
    cleanup.track(destination.clone());
    let mut written = 0_u64;
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?
    {
        written += chunk.len() as u64;
        if written > 1024 * 1024 * 1024 {
            return Err(AppError::BadRequest("o arquivo excede 1 GiB".into()));
        }
        output.write_all(&chunk).await?;
    }
    output.flush().await?;
    cleanup.commit();
    let saved_as = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&filename)
        .to_string();
    Ok(Json(
        json!({ "saved": true, "filename": saved_as, "bytes": written }),
    ))
}

async fn list_books(
    State(state): State<AppState>,
    Query(query): Query<CatalogQuery>,
) -> AppResult<Json<Value>> {
    let page = state.db.list_books(&query).await?;
    let facets = state.db.catalog_facets().await?;
    Ok(Json(json!({
        "books": page.books, "facets": facets, "total": page.total,
        "limit": page.limit, "offset": page.offset
    })))
}

async fn get_book(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> AppResult<Json<Value>> {
    Ok(Json(json!({
        "book": state.db.book(id).await?,
        "collections": state.db.collections().await?,
        "book_collection_ids": state.db.book_collection_ids(id).await?
    })))
}

async fn reading_desk(State(state): State<AppState>) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "books": state.db.reading_desk().await? })))
}

async fn suggestions(State(state): State<AppState>) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "books": state.db.suggestions(10).await? })))
}

async fn list_collections(State(state): State<AppState>) -> AppResult<Json<Value>> {
    Ok(Json(
        json!({ "collections": state.db.collections().await? }),
    ))
}

async fn create_collection(
    State(state): State<AppState>,
    Json(input): Json<SaveCollection>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let collection = state.db.create_collection(&input).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "collection": collection })),
    ))
}

async fn update_collection(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
    Json(input): Json<SaveCollection>,
) -> AppResult<Json<Value>> {
    state.db.update_collection(id, &input).await?;
    Ok(Json(json!({ "saved": true })))
}

async fn delete_collection(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> AppResult<StatusCode> {
    state.db.delete_collection(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn add_book_to_collection(
    State(state): State<AppState>,
    AxumPath((collection_id, book_id)): AxumPath<(i64, i64)>,
) -> AppResult<Json<Value>> {
    state.db.book(book_id).await?;
    state.db.add_to_collection(collection_id, book_id).await?;
    Ok(Json(json!({ "saved": true })))
}

async fn remove_book_from_collection(
    State(state): State<AppState>,
    AxumPath((collection_id, book_id)): AxumPath<(i64, i64)>,
) -> AppResult<StatusCode> {
    state
        .db
        .remove_from_collection(collection_id, book_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn library_health(State(state): State<AppState>) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "health": state.db.library_health().await? })))
}

async fn list_duplicates(State(state): State<AppState>) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "groups": state.db.duplicates().await? })))
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

#[derive(Deserialize)]
struct ConfirmFileDeletion {
    filename: String,
}

async fn delete_book_file(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
    Json(input): Json<ConfirmFileDeletion>,
) -> AppResult<Json<Value>> {
    let book = state.db.book(id).await?;
    if input.filename != book.filename {
        return Err(AppError::BadRequest(
            "a confirmacao deve repetir exatamente o nome do arquivo".into(),
        ));
    }
    let path = resolve_path(&state, &book.relative_path).await?;
    tokio::fs::remove_file(&path).await?;
    state.db.mark_unavailable(id).await?;
    Ok(Json(json!({
        "deleted": true,
        "filename": book.filename,
        "recoverable_from_application": false
    })))
}

async fn book_cover(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> AppResult<Response> {
    let filename = state.db.cover_filename(id).await?;
    let path = confined_asset_path(&state.config.covers_dir, &filename).await?;
    stream_file(path, "cover", false, &HeaderMap::new()).await
}

/// Resolve um nome simples e rejeita links que apontem para fora das capas.
async fn confined_asset_path(root: &Path, filename: &str) -> AppResult<PathBuf> {
    let candidate = safe_branding_path(root, filename).ok_or(AppError::NotFound)?;
    let root = tokio::fs::canonicalize(root).await?;
    let candidate = tokio::fs::canonicalize(candidate).await?;
    if !candidate.starts_with(&root) || !tokio::fs::metadata(&candidate).await?.is_file() {
        return Err(AppError::NotFound);
    }
    Ok(candidate)
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

async fn remove_progress(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> AppResult<StatusCode> {
    state.db.book(id).await?;
    state.db.remove_progress(id).await?;
    Ok(StatusCode::NO_CONTENT)
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

async fn list_backups(State(state): State<AppState>) -> AppResult<Json<Value>> {
    Ok(Json(
        json!({ "backups": backup::list(&state.config).await? }),
    ))
}

async fn create_backup(State(state): State<AppState>) -> AppResult<(StatusCode, Json<Value>)> {
    let info = backup::create(&state.db, &state.config).await?;
    Ok((StatusCode::CREATED, Json(json!({ "backup": info }))))
}

async fn download_backup(
    State(state): State<AppState>,
    AxumPath(filename): AxumPath<String>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let path = backup::safe_backup_path(&state.config, &filename)?;
    if !path.is_file() {
        return Err(AppError::NotFound);
    }
    stream_file(path, &filename, true, &headers).await
}

async fn delete_backup(
    State(state): State<AppState>,
    AxumPath(filename): AxumPath<String>,
) -> AppResult<StatusCode> {
    let path = backup::safe_backup_path(&state.config, &filename)?;
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(AppError::NotFound),
        Err(error) => Err(error.into()),
    }
}

/// Reserva o mesmo estado usado por scans/capas e libera em qualquer saida.
struct RestoreReservation(Arc<Mutex<ScanStatus>>);

impl RestoreReservation {
    fn acquire(state: &AppState) -> AppResult<Self> {
        let mut status = state
            .scan_status
            .lock()
            .map_err(|_| AppError::Internal("estado da tarefa indisponivel".into()))?;
        if status.running {
            return Err(AppError::BadRequest(
                "aguarde a tarefa atual terminar antes de restaurar".into(),
            ));
        }
        *status = ScanStatus::idle();
        status.running = true;
        status.phase = "restoring".into();
        Ok(Self(state.scan_status.clone()))
    }
}

impl Drop for RestoreReservation {
    fn drop(&mut self) {
        if let Ok(mut status) = self.0.lock() {
            *status = ScanStatus::idle();
        }
    }
}

async fn restore_backup(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> AppResult<Json<Value>> {
    let _reservation = RestoreReservation::acquire(&state)?;
    let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?
    else {
        return Err(AppError::BadRequest("nenhum backup enviado".into()));
    };
    tokio::fs::create_dir_all(&state.config.backup_dir).await?;
    let upload = state.config.backup_dir.join(format!(
        ".restore-upload-{}-{}.zip",
        Utc::now().timestamp_millis(),
        rand::thread_rng().gen_range(1000_u16..9999_u16)
    ));
    let mut cleanup = crate::pending_file::PendingFile::default();
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&upload)
        .await?;
    cleanup.track(upload.clone());
    let mut written = 0_u64;
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?
    {
        written += chunk.len() as u64;
        if written > 2 * 1024 * 1024 * 1024 {
            return Err(AppError::BadRequest("backup excede 2 GiB".into()));
        }
        output.write_all(&chunk).await?;
    }
    output.flush().await?;
    drop(output);

    let package_result = backup::unpack(upload.clone(), &state.config).await;
    drop(cleanup);
    let package = package_result?;
    let automatic = backup::create(&state.db, &state.config).await?;
    let restore_result = async {
        let assets = backup::AssetRestore::install(&package, &state.config)?;
        state.db.replace_from(&package.database).await?;
        assets.commit();
        Ok::<(), AppError>(())
    }
    .await;
    restore_result?;
    Ok(Json(json!({
        "restored": true,
        "safety_backup": automatic,
        "original_books_unchanged": true
    })))
}

async fn start_cover_generation(
    State(state): State<AppState>,
) -> AppResult<(StatusCode, Json<ScanStatus>)> {
    let status = {
        let mut current = state
            .scan_status
            .lock()
            .map_err(|_| AppError::Internal("estado da tarefa indisponivel".into()))?;
        if current.running {
            return Ok((StatusCode::OK, Json(current.clone())));
        }
        let mut status = ScanStatus::idle();
        status.running = true;
        status.phase = "covers".into();
        status.profile = "low_priority".into();
        status.started_at = Some(Utc::now().to_rfc3339());
        *current = status.clone();
        status
    };
    let task_state = state.clone();
    tokio::spawn(async move {
        if let Err(error) = generate_missing_covers(&task_state).await {
            finish_scan_error(&task_state, error.to_string());
            return;
        }
        if let Ok(mut current) = task_state.scan_status.lock() {
            current.running = false;
            current.phase = "completed".into();
            current.percent = 100.0;
            current.current_file = None;
            current.finished_at = Some(Utc::now().to_rfc3339());
        }
    });
    Ok((StatusCode::ACCEPTED, Json(status)))
}

async fn generate_missing_covers(state: &AppState) -> AppResult<()> {
    let root = library_root(state).await?;
    let candidates = state.db.missing_pdf_covers(10_000).await?;
    if let Ok(mut status) = state.scan_status.lock() {
        status.phase = "covers".into();
        status.total = candidates.len();
        status.processed = 0;
        status.percent = if candidates.is_empty() { 100.0 } else { 0.0 };
    }
    for (index, (book_id, relative_path)) in candidates.iter().enumerate() {
        if let Ok(mut status) = state.scan_status.lock() {
            status.current_file = Some(relative_path.clone());
        }
        if let Err(error) =
            covers::generate_one(&state.db, &state.config, &root, *book_id, relative_path).await
        {
            if let Ok(mut status) = state.scan_status.lock() {
                if status.warnings.len() < 30 {
                    status.warnings.push(error.to_string());
                }
            }
        }
        if let Ok(mut status) = state.scan_status.lock() {
            status.processed = index + 1;
            status.percent = (index + 1) as f64 / candidates.len() as f64 * 100.0;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    Ok(())
}

#[derive(Deserialize)]
struct OpdsImportRequest {
    url: String,
}

async fn opds_catalog(
    State(state): State<AppState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    Query(query): Query<CatalogQuery>,
) -> AppResult<Response> {
    let page = state.db.list_books(&query).await?;
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("localhost:20000");
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .filter(|value| matches!(*value, "http" | "https"))
        .unwrap_or("http");
    let base_url = format!("{scheme}://{host}");
    let next = if u64::from(page.offset) + u64::from(page.limit) < page.total {
        let mut url = url::Url::parse(&format!("{base_url}{uri}"))
            .map_err(|_| AppError::BadRequest("URL do catalogo invalida".into()))?;
        let pairs: Vec<_> = url
            .query_pairs()
            .filter(|(key, _)| key != "offset" && key != "limit")
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        url.query_pairs_mut()
            .clear()
            .extend_pairs(pairs)
            .append_pair("limit", &page.limit.to_string())
            .append_pair(
                "offset",
                &(u64::from(page.offset) + u64::from(page.limit)).to_string(),
            );
        Some(url.to_string())
    } else {
        None
    };
    let body = opds::catalog_xml(&page.books, &base_url, next.as_deref());
    Ok((
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=acquisition; charset=utf-8",
        )],
        body,
    )
        .into_response())
}

async fn import_opds(
    State(state): State<AppState>,
    Json(input): Json<OpdsImportRequest>,
) -> AppResult<Json<Value>> {
    if !network_enabled(&state).await? {
        return Err(AppError::NetworkDisabled);
    }
    let root = library_root(&state).await?;
    let report = opds::import_catalog(&state.http, &input.url, &root).await?;
    Ok(Json(json!({
        "report": report,
        "next_step": "inicie uma varredura para catalogar os arquivos importados"
    })))
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
    // Prepare todos os recursos antes de alterar dados do catalogo.
    state.db.book(id).await?;
    if scanner::sanitize_metadata_text(&update.title).is_none() {
        return Err(AppError::BadRequest("informe um titulo legivel".into()));
    }
    let mut cleanup = crate::pending_file::PendingFile::default();
    let mut cover_filename = None;
    if let Some(url) = candidate.cover_url {
        if !network_enabled(&state).await? {
            return Err(AppError::NetworkDisabled);
        }
        let (bytes, extension) = metadata::download_cover(&state.http, &url).await?;
        tokio::fs::create_dir_all(&state.config.covers_dir).await?;
        // Nomes imutaveis evitam sobrescrever a capa vigente em falhas/concorrencia.
        let filename = format!("book-{id}-{}.{extension}", rand::random::<u64>());
        let path = state.config.covers_dir.join(&filename);
        let mut output = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await?;
        cleanup.track(path);
        output.write_all(&bytes).await?;
        output.flush().await?;
        drop(output);
        cover_filename = Some(filename);
    }
    state
        .db
        .update_book_with_cover(id, &update, cover_filename.as_deref())
        .await?;
    cleanup.commit();
    let cover_saved = cover_filename.is_some();
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
    setting_bool(state, "network_metadata_enabled").await
}

async fn setting_bool(state: &AppState, key: &str) -> AppResult<bool> {
    Ok(state.db.setting(key).await?.as_deref() == Some("true"))
}

async fn configured_scan_profile(state: &AppState) -> AppResult<ScanProfile> {
    Ok(state
        .db
        .setting("scan_profile")
        .await?
        .as_deref()
        .and_then(ScanProfile::parse)
        .unwrap_or_default())
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
    headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox; default-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; script-src 'none'; connect-src 'none'; object-src 'none'; form-action 'none'; base-uri 'none'; navigate-to 'none'"));
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
    // Somente formatos passivos conhecidos podem ser abertos na origem do app.
    let inline_safe = matches!(
        mime.as_str(),
        "application/pdf"
            | "image/png"
            | "image/jpeg"
            | "image/gif"
            | "image/webp"
            | "image/x-icon"
            | "image/vnd.microsoft.icon"
            | "text/plain"
    );
    let disposition = if download || !inline_safe {
        "attachment"
    } else {
        "inline"
    };
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if !inline_safe {
        add_sandbox_headers(response.headers_mut());
    }
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

    #[tokio::test]
    async fn active_originals_are_downloaded_and_sandboxed() {
        let root = PathBuf::from(format!(".test-stream-{}", rand::random::<u64>()));
        tokio::fs::create_dir(&root).await.unwrap();
        let path = root.join("book.html");
        tokio::fs::write(&path, b"<script>alert(1)</script>")
            .await
            .unwrap();
        for headers in [HeaderMap::new(), {
            let mut headers = HeaderMap::new();
            headers.insert(header::RANGE, HeaderValue::from_static("bytes=0-4"));
            headers
        }] {
            let response = stream_file(path.clone(), "book.html", false, &headers)
                .await
                .unwrap();
            assert!(response.headers()[header::CONTENT_DISPOSITION]
                .to_str()
                .unwrap()
                .starts_with("attachment;"));
            assert_eq!(
                response.headers()[header::X_CONTENT_TYPE_OPTIONS],
                "nosniff"
            );
            assert!(response.headers()[header::CONTENT_SECURITY_POLICY]
                .to_str()
                .unwrap()
                .contains("script-src 'none'"));
        }
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn cover_paths_reject_traversal() {
        assert!(confined_asset_path(Path::new("unused"), "../secret.txt")
            .await
            .is_err());
        assert!(confined_asset_path(Path::new("unused"), "/secret.txt")
            .await
            .is_err());
    }

    #[test]
    fn parses_http_ranges() {
        assert_eq!(parse_range("bytes=0-99", 200), Some((0, 99)));
        assert_eq!(parse_range("bytes=100-", 200), Some((100, 199)));
        assert_eq!(parse_range("bytes=-20", 200), Some((180, 199)));
        assert_eq!(parse_range("bytes=300-", 200), None);
    }
}

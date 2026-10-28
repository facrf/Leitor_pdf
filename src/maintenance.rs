//! Manutencao local: lixeira recuperavel e coleta conservadora de capas.
use crate::{
    error::{AppError, AppResult},
    scanner,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
    time::{Duration, SystemTime},
};

#[derive(Debug, Serialize, Deserialize)]
pub struct TrashItem {
    pub token: String,
    pub book_id: i64,
    pub filename: String,
    pub relative_path: String,
    pub removed_at: String,
}

fn trash_dir(root: &Path) -> AppResult<PathBuf> {
    let root = root.canonicalize()?;
    let directory = root.join(".estante-trash");
    fs::create_dir_all(&directory)?;
    if fs::symlink_metadata(&directory)?.file_type().is_symlink()
        || directory.canonicalize()? != directory
    {
        return Err(AppError::BadRequest("pasta da lixeira insegura".into()));
    }
    Ok(directory)
}

fn validate_token(token: &str) -> AppResult<()> {
    if token.len() != 32 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::BadRequest(
            "identificador da lixeira invalido".into(),
        ));
    }
    Ok(())
}

/// Move sem sobrescrever; origem e destino precisam estar no mesmo filesystem.
fn move_no_replace(source: &Path, destination: &Path) -> AppResult<()> {
    fs::hard_link(source, destination)?;
    if let Err(error) = fs::remove_file(source) {
        let _ = fs::remove_file(destination);
        return Err(error.into());
    }
    Ok(())
}

/// Guarda o original e um manifesto; nao apaga definitivamente o livro.
pub fn trash(
    root: &Path,
    book_id: i64,
    filename: String,
    relative_path: String,
) -> AppResult<TrashItem> {
    let source = scanner::resolve_book_path(root, &relative_path)?;
    let directory = trash_dir(root)?;
    let item = TrashItem {
        token: format!("{:032x}", rand::random::<u128>()),
        book_id,
        filename,
        relative_path,
        removed_at: chrono::Utc::now().to_rfc3339(),
    };
    let manifest = directory.join(format!("{}.json", item.token));
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&manifest)?;
    use std::io::Write;
    output.write_all(
        &serde_json::to_vec(&item).map_err(|error| AppError::Internal(error.to_string()))?,
    )?;
    output.sync_all()?;
    if let Err(error) = move_no_replace(&source, &directory.join(format!("{}.book", item.token))) {
        let _ = fs::remove_file(manifest);
        return Err(error);
    }
    Ok(item)
}

/// Le manifestos limitados e recusa links simbolicos.
fn read_manifest(path: &Path) -> AppResult<TrashItem> {
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(AppError::BadRequest("manifesto inseguro".into()));
    }
    let bytes = crate::bounded_io::read_limited(&mut fs::File::open(path)?, 16 * 1024)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| AppError::BadRequest(format!("manifesto da lixeira invalido: {error}")))
}

/// Lista somente manifestos que ainda possuem um original recuperavel.
pub fn list_trash(root: &Path) -> AppResult<Vec<TrashItem>> {
    let directory = trash_dir(root)?;
    let mut items = Vec::new();
    for entry in fs::read_dir(&directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file()
            || entry.path().extension().and_then(|s| s.to_str()) != Some("json")
        {
            continue;
        }
        let token = entry
            .path()
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_owned();
        if validate_token(&token).is_err() || !directory.join(format!("{token}.book")).is_file() {
            continue;
        }
        let item = match read_manifest(&entry.path()) {
            Ok(item) if item.token == token => item,
            result => {
                tracing::warn!(path = %entry.path().display(), ?result, "manifesto da lixeira recusado");
                continue;
            }
        };
        items.push(item);
    }
    items.sort_by(|a, b| b.removed_at.cmp(&a.removed_at));
    Ok(items)
}

/// Restaura no caminho original, recusando travessia, links e sobrescrita.
pub fn restore_trash(root: &Path, token: &str) -> AppResult<TrashItem> {
    validate_token(token)?;
    let directory = trash_dir(root)?;
    let manifest = directory.join(format!("{token}.json"));
    let item = read_manifest(&manifest)?;
    if item.token != token
        || Path::new(&item.relative_path)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        || item.relative_path.contains('\\')
        || item.relative_path.starts_with(".estante-trash/")
    {
        return Err(AppError::BadRequest("caminho da lixeira invalido".into()));
    }
    let destination = root.join(&item.relative_path);
    if let Some(parent) = destination.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }
    let parent = destination
        .parent()
        .ok_or(AppError::NotFound)?
        .canonicalize()?;
    if !parent.starts_with(root.canonicalize()?) {
        return Err(AppError::BadRequest("destino fora da biblioteca".into()));
    }
    let source = directory.join(format!("{token}.book"));
    if !fs::symlink_metadata(&source)?.is_file() {
        return Err(AppError::BadRequest("original inseguro".into()));
    }
    move_no_replace(&source, &destination)?;
    if let Err(error) = fs::remove_file(manifest) {
        tracing::warn!(%error, "manifesto residual da lixeira");
    }
    Ok(item)
}

#[derive(Debug, Serialize)]
pub struct OrphanCover {
    pub filename: String,
    pub size: u64,
}

/// Apenas arquivos planos sem referencias e com sete dias de idade sao elegiveis.
pub fn orphan_covers(
    directory: &Path,
    referenced: &HashSet<String>,
) -> AppResult<Vec<OrphanCover>> {
    let mut result = Vec::new();
    let now = SystemTime::now();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let Some(filename) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if referenced.contains(&filename) {
            continue;
        }
        let metadata = entry.metadata()?;
        if now.duration_since(metadata.modified()?).unwrap_or_default()
            < Duration::from_secs(7 * 86400)
        {
            continue;
        }
        result.push(OrphanCover {
            filename,
            size: metadata.len(),
        });
    }
    result.sort_by(|a, b| a.filename.cmp(&b.filename));
    Ok(result)
}

#[cfg(test)]
#[path = "../tests/rust/maintenance.rs"]
mod tests;

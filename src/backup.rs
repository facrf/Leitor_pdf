use std::{
    fs::File,
    io::Write,
    path::{Component, Path, PathBuf},
};

use chrono::Utc;
use rand::Rng;
use serde::Serialize;
use walkdir::WalkDir;
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

use crate::{
    config::Config,
    db::Database,
    error::{AppError, AppResult},
};

#[derive(Debug, Serialize)]
pub struct BackupInfo {
    pub filename: String,
    pub size: u64,
    pub modified_at: String,
}

#[derive(Debug)]
pub struct RestorePackage {
    pub root: PathBuf,
    pub database: PathBuf,
    pub covers: PathBuf,
    pub branding: PathBuf,
}

/// Cria um pacote consistente do catalogo e dos recursos gerados. Os livros
/// originais nao sao incluidos para que um acervo de 150 GB continue pratico.
pub async fn create(db: &Database, config: &Config) -> AppResult<BackupInfo> {
    tokio::fs::create_dir_all(&config.backup_dir).await?;
    let suffix = rand::thread_rng().gen_range(1000_u16..9999_u16);
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
    let filename = format!("estante-{timestamp}-{suffix}.zip");
    let destination = config.backup_dir.join(&filename);
    let snapshot = config
        .backup_dir
        .join(format!(".catalog-{timestamp}-{suffix}.sqlite"));
    db.snapshot_to(&snapshot).await?;

    let covers = config.covers_dir.clone();
    let branding = config.branding_dir.clone();
    let destination_for_task = destination.clone();
    let snapshot_for_task = snapshot.clone();
    let result = tokio::task::spawn_blocking(move || {
        write_package(
            &destination_for_task,
            &snapshot_for_task,
            &covers,
            &branding,
        )
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?;
    let _ = tokio::fs::remove_file(&snapshot).await;
    result?;
    let metadata = tokio::fs::metadata(&destination).await?;
    Ok(BackupInfo {
        filename,
        size: metadata.len(),
        modified_at: Utc::now().to_rfc3339(),
    })
}

pub async fn list(config: &Config) -> AppResult<Vec<BackupInfo>> {
    tokio::fs::create_dir_all(&config.backup_dir).await?;
    let mut entries = tokio::fs::read_dir(&config.backup_dir).await?;
    let mut backups = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let Some(filename) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !filename.starts_with("estante-") || !filename.ends_with(".zip") {
            continue;
        }
        let metadata = entry.metadata().await?;
        let modified_at = metadata
            .modified()
            .ok()
            .map(chrono::DateTime::<Utc>::from)
            .unwrap_or_else(Utc::now)
            .to_rfc3339();
        backups.push(BackupInfo {
            filename,
            size: metadata.len(),
            modified_at,
        });
    }
    backups.sort_by(|left, right| right.filename.cmp(&left.filename));
    Ok(backups)
}

pub fn safe_backup_path(config: &Config, filename: &str) -> AppResult<PathBuf> {
    let path = Path::new(filename);
    if path.components().count() != 1
        || !filename.starts_with("estante-")
        || !filename.ends_with(".zip")
    {
        return Err(AppError::BadRequest("nome de backup invalido".into()));
    }
    Ok(config.backup_dir.join(path))
}

pub async fn unpack(archive_path: PathBuf, config: &Config) -> AppResult<RestorePackage> {
    let suffix = rand::thread_rng().gen_range(1000_u16..9999_u16);
    let root = config.backup_dir.join(format!(
        ".restore-{}-{suffix}",
        Utc::now().timestamp_millis()
    ));
    tokio::fs::create_dir_all(&root).await?;
    let root_for_task = root.clone();
    let extraction =
        tokio::task::spawn_blocking(move || extract_package(&archive_path, &root_for_task))
            .await
            .map_err(|error| AppError::Internal(error.to_string()))
            .and_then(|result| result);
    if let Err(error) = extraction {
        let _ = tokio::fs::remove_dir_all(&root).await;
        return Err(match error {
            AppError::Zip(error) => AppError::BadRequest(format!("arquivo ZIP invalido: {error}")),
            other => other,
        });
    }
    let database = root.join("library.db");
    if !database.is_file() {
        let _ = tokio::fs::remove_dir_all(&root).await;
        return Err(AppError::BadRequest(
            "o pacote nao contem library.db".into(),
        ));
    }
    Ok(RestorePackage {
        database,
        covers: root.join("covers"),
        branding: root.join("branding"),
        root,
    })
}

pub async fn copy_restored_assets(package: &RestorePackage, config: &Config) -> AppResult<()> {
    copy_tree(&package.covers, &config.covers_dir).await?;
    copy_tree(&package.branding, &config.branding_dir).await?;
    Ok(())
}

async fn copy_tree(source: &Path, destination: &Path) -> AppResult<()> {
    if !source.is_dir() {
        return Ok(());
    }
    tokio::fs::create_dir_all(destination).await?;
    let mut entries = tokio::fs::read_dir(source).await?;
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_type().await?.is_file() {
            tokio::fs::copy(entry.path(), destination.join(entry.file_name())).await?;
        }
    }
    Ok(())
}

fn write_package(
    destination: &Path,
    database: &Path,
    covers: &Path,
    branding: &Path,
) -> AppResult<()> {
    let output = File::create(destination)?;
    let mut writer = ZipWriter::new(output);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o600);
    add_file(&mut writer, database, "library.db", options)?;
    add_directory(&mut writer, covers, "covers", options)?;
    add_directory(&mut writer, branding, "branding", options)?;
    writer.start_file("manifest.json", options)?;
    writer.write_all(
        serde_json::json!({
            "application": "Estante Livre",
            "format_version": 1,
            "created_at": Utc::now().to_rfc3339(),
            "includes_original_books": false,
            "author": "FACRF",
            "website": "https://www.fabianocesar.com"
        })
        .to_string()
        .as_bytes(),
    )?;
    writer.finish()?;
    Ok(())
}

fn add_directory(
    writer: &mut ZipWriter<File>,
    directory: &Path,
    prefix: &str,
    options: SimpleFileOptions,
) -> AppResult<()> {
    if !directory.is_dir() {
        return Ok(());
    }
    for entry in WalkDir::new(directory).follow_links(false) {
        let entry = entry.map_err(|error| AppError::Internal(error.to_string()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(directory)
            .map_err(|_| AppError::Internal("recurso fora da pasta de backup".into()))?;
        if relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            continue;
        }
        let name = format!("{prefix}/{}", relative.to_string_lossy().replace('\\', "/"));
        add_file(writer, entry.path(), &name, options)?;
    }
    Ok(())
}

fn add_file(
    writer: &mut ZipWriter<File>,
    path: &Path,
    name: &str,
    options: SimpleFileOptions,
) -> AppResult<()> {
    writer.start_file(name, options)?;
    let mut input = File::open(path)?;
    std::io::copy(&mut input, writer)?;
    Ok(())
}

fn extract_package(archive_path: &Path, destination: &Path) -> AppResult<()> {
    let file = File::open(archive_path)?;
    let mut archive = ZipArchive::new(file)?;
    if archive.len() > 10_000 {
        return Err(AppError::BadRequest("backup contem entradas demais".into()));
    }
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        total = total.saturating_add(entry.size());
        if total > 2 * 1024 * 1024 * 1024 {
            return Err(AppError::BadRequest(
                "backup descompactado excede 2 GiB".into(),
            ));
        }
        let Some(relative) = entry.enclosed_name() else {
            return Err(AppError::BadRequest("caminho inseguro no backup".into()));
        };
        let allowed = relative == Path::new("library.db")
            || relative == Path::new("manifest.json")
            || relative.starts_with("covers")
            || relative.starts_with("branding");
        if !allowed {
            continue;
        }
        let output = destination.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = File::create(output)?;
        std::io::copy(&mut entry, &mut file)?;
        file.flush()?;
    }
    Ok(())
}

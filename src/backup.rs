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

impl Drop for RestorePackage {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.root) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(path = %self.root.display(), %error, "falha ao limpar pacote extraido");
            }
        }
    }
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
    // A propriedade acompanha o worker: cancelar o await nao remove uma
    // pasta que ainda esta sendo escrita pelo spawn_blocking.
    std::fs::create_dir(&root)?;
    let package = RestorePackage {
        database: root.join("library.db"),
        covers: root.join("covers"),
        branding: root.join("branding"),
        root,
    };
    tokio::task::spawn_blocking(move || {
        extract_package(&archive_path, &package.root).map_err(|error| match error {
            AppError::Zip(error) => AppError::BadRequest(format!("arquivo ZIP invalido: {error}")),
            other => other,
        })?;
        if !package.database.is_file() {
            return Err(AppError::BadRequest(
                "o pacote nao contem library.db".into(),
            ));
        }
        Ok(package)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

/// Guarda as pastas anteriores ate o banco confirmar a restauracao.
/// Drop reverte inclusive quando a requisicao e cancelada durante um await.
pub struct AssetRestore {
    entries: Vec<(PathBuf, PathBuf, PathBuf, bool, bool)>,
    committed: bool,
}

impl AssetRestore {
    pub fn install(package: &RestorePackage, config: &Config) -> AppResult<Self> {
        let mut restore = Self {
            entries: Vec::new(),
            committed: false,
        };
        for (source, target) in [
            (&package.covers, &config.covers_dir),
            (&package.branding, &config.branding_dir),
        ] {
            let suffix = rand::random::<u64>();
            let stage = target.with_extension(format!("restore-stage-{suffix}"));
            let old = target.with_extension(format!("restore-old-{suffix}"));
            std::fs::create_dir(&stage)?;
            restore
                .entries
                .push((target.clone(), stage.clone(), old.clone(), false, false));
            if source.is_dir() {
                for entry in WalkDir::new(source).follow_links(false) {
                    let entry = entry.map_err(|error| AppError::Internal(error.to_string()))?;
                    let relative = entry
                        .path()
                        .strip_prefix(source)
                        .map_err(|error| AppError::Internal(error.to_string()))?;
                    if entry.file_type().is_dir() {
                        std::fs::create_dir_all(stage.join(relative))?;
                    } else if entry.file_type().is_file() {
                        std::fs::copy(entry.path(), stage.join(relative))?;
                    } else {
                        return Err(AppError::BadRequest(
                            "recurso de backup nao e arquivo regular".into(),
                        ));
                    }
                }
            }
            let entry = restore
                .entries
                .last_mut()
                .ok_or_else(|| AppError::Internal("restauracao vazia".into()))?;
            if target.exists() {
                std::fs::rename(target, &old)?;
                entry.3 = true;
            }
            std::fs::rename(&stage, target)?;
            entry.4 = true;
        }
        Ok(restore)
    }

    pub fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for AssetRestore {
    fn drop(&mut self) {
        for (target, stage, old, had_old, installed) in self.entries.iter().rev() {
            let result = if self.committed {
                if *had_old {
                    std::fs::remove_dir_all(old)
                } else {
                    Ok(())
                }
            } else {
                (|| -> std::io::Result<()> {
                    if *installed {
                        std::fs::remove_dir_all(target)?;
                    }
                    if *had_old {
                        std::fs::rename(old, target)?;
                    }
                    Ok(())
                })()
            };
            if let Err(error) = result {
                tracing::error!(path = %old.display(), %error, "restauracao: pasta de recuperacao preservada");
            }
            if stage.exists() {
                let _ = std::fs::remove_dir_all(stage);
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn invalid_archive_does_not_leave_extraction_directory() {
        let root = PathBuf::from(format!(".test-unpack-{}", rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        let archive = root.join("invalid.zip");
        std::fs::write(&archive, b"not a ZIP").unwrap();
        let mut config = Config::from_env().unwrap();
        config.backup_dir = root.clone();
        assert!(unpack(archive, &config).await.is_err());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn assets_roll_back_on_drop_and_replace_on_commit() {
        let root = PathBuf::from(format!(".test-restore-{}", rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        let mut config = Config::from_env().unwrap();
        config.covers_dir = root.join("live-covers");
        config.branding_dir = root.join("live-branding");
        let package = RestorePackage {
            root: root.clone(),
            database: root.join("unused.db"),
            covers: root.join("incoming-covers"),
            branding: root.join("incoming-branding"),
        };
        for path in [
            &config.covers_dir,
            &config.branding_dir,
            &package.covers,
            &package.branding,
        ] {
            std::fs::create_dir(path).unwrap();
        }
        std::fs::write(config.covers_dir.join("old.png"), b"old").unwrap();
        std::fs::write(package.covers.join("new.png"), b"new").unwrap();
        {
            let _transaction = AssetRestore::install(&package, &config).unwrap();
            assert!(config.covers_dir.join("new.png").exists());
            assert!(!config.covers_dir.join("old.png").exists());
        }
        assert_eq!(
            std::fs::read(config.covers_dir.join("old.png")).unwrap(),
            b"old"
        );
        assert!(!config.covers_dir.join("new.png").exists());
        // A segunda pasta falha; a primeira deve ser reposta tambem.
        let branding = config.branding_dir.clone();
        config.branding_dir = root.join("missing-parent").join("branding");
        assert!(AssetRestore::install(&package, &config).is_err());
        assert!(config.covers_dir.join("old.png").exists());
        config.branding_dir = branding;
        AssetRestore::install(&package, &config).unwrap().commit();
        assert_eq!(
            std::fs::read(config.covers_dir.join("new.png")).unwrap(),
            b"new"
        );
        assert!(!config.covers_dir.join("old.png").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}

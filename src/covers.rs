use std::{path::Path, process::ExitStatus, time::Duration};

use chrono::Utc;
use tokio::process::Command;

use crate::{
    config::Config,
    db::Database,
    error::{AppError, AppResult},
    scanner,
};

/// Renderiza somente a primeira pagina com prioridade baixa. `pdftoppm` roda
/// em processo separado para que uma falha em PDF nao derrube o servidor.
pub async fn generate_one(
    db: &Database,
    config: &Config,
    library_root: &Path,
    book_id: i64,
    relative_path: &str,
) -> AppResult<()> {
    let source = scanner::resolve_book_path(library_root, relative_path)?;
    let extension = source
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    if matches!(extension.as_str(), "mp3" | "m4a" | "m4b") {
        if let Some((image_data, ext)) = crate::audio_metadata::extract_embedded_cover(&source) {
            tokio::fs::create_dir_all(&config.covers_dir).await?;
            let base_name = format!(
                "auto-{book_id}-{}-{}",
                Utc::now().timestamp_millis(),
                rand::random::<u64>(),
            );
            let filename = format!("{base_name}.{ext}");
            let output_file = config.covers_dir.join(&filename);
            tokio::fs::write(&output_file, &image_data).await?;
            db.set_cover(book_id, &filename).await?;
            return Ok(());
        }
        return Err(AppError::BadRequest("audiolivro sem capa embutida".into()));
    }

    if matches!(extension.as_str(), "mp4" | "webm" | "mkv" | "m4v") {
        tokio::fs::create_dir_all(&config.covers_dir).await?;
        let base_name = format!(
            "auto-{book_id}-{}-{}",
            Utc::now().timestamp_millis(),
            rand::random::<u64>(),
        );
        let output_file = config.covers_dir.join(format!("{base_name}.jpg"));
        let mut cleanup = crate::pending_file::PendingFile::default();
        cleanup.track(output_file.clone());
        let mut command = Command::new("nice");
        command
            .arg("-n")
            .arg("15")
            .arg("ffmpeg")
            .arg("-ss")
            .arg("00:00:01")
            .arg("-i")
            .arg(&source)
            .arg("-frames:v")
            .arg("1")
            .arg("-update")
            .arg("1")
            .arg("-vf")
            .arg("scale=min(900\\,iw):-1")
            .arg("-q:v")
            .arg("2")
            .arg("-y")
            .arg(&output_file);
        let status = run_renderer(
            &mut command,
            Duration::from_secs(config.cover_timeout_seconds),
        )
        .await;

        let ok = match status {
            Ok(s) => s.success() && output_file.is_file(),
            Err(_) => false,
        };

        if !ok {
            let mut retry_command = Command::new("nice");
            retry_command
                .arg("-n")
                .arg("15")
                .arg("ffmpeg")
                .arg("-ss")
                .arg("00:00:00")
                .arg("-i")
                .arg(&source)
                .arg("-frames:v")
                .arg("1")
                .arg("-update")
                .arg("1")
                .arg("-vf")
                .arg("scale=min(900\\,iw):-1")
                .arg("-q:v")
                .arg("2")
                .arg("-y")
                .arg(&output_file);
            let retry_status = run_renderer(
                &mut retry_command,
                Duration::from_secs(config.cover_timeout_seconds),
            )
            .await;
            if !matches!(retry_status, Ok(s) if s.success()) || !output_file.is_file() {
                let _ = tokio::fs::remove_file(&output_file).await;
                return Err(AppError::BadRequest(format!(
                    "nao foi possivel extrair thumbnail de video {}",
                    source.display()
                )));
            }
        }

        if tokio::fs::metadata(&output_file).await?.len() > 15 * 1024 * 1024 {
            let _ = tokio::fs::remove_file(&output_file).await;
            return Err(AppError::BadRequest("capa gerada grande demais".into()));
        }
        db.set_cover(book_id, &format!("{base_name}.jpg")).await?;
        cleanup.commit();
        return Ok(());
    }

    tokio::fs::create_dir_all(&config.covers_dir).await?;
    let base_name = format!(
        "auto-{book_id}-{}-{}",
        Utc::now().timestamp_millis(),
        rand::random::<u64>(),
    );
    let output_base = config.covers_dir.join(&base_name);
    let output_file = config.covers_dir.join(format!("{base_name}.jpg"));
    let mut cleanup = crate::pending_file::PendingFile::default();
    cleanup.track(output_file.clone());
    let mut command = Command::new("nice");
    command
        .arg("-n")
        .arg("15")
        .arg("pdftoppm")
        .arg("-f")
        .arg("1")
        .arg("-singlefile")
        .arg("-scale-to")
        .arg("900")
        .arg("-jpeg")
        .arg(&source)
        .arg(&output_base);
    let status = run_renderer(
        &mut command,
        Duration::from_secs(config.cover_timeout_seconds),
    )
    .await?;
    if !status.success() || !output_file.is_file() {
        let _ = tokio::fs::remove_file(&output_file).await;
        return Err(AppError::BadRequest(format!(
            "nao foi possivel renderizar a primeira pagina de {}",
            source.display()
        )));
    }
    if tokio::fs::metadata(&output_file).await?.len() > 15 * 1024 * 1024 {
        let _ = tokio::fs::remove_file(&output_file).await;
        return Err(AppError::BadRequest("capa gerada grande demais".into()));
    }
    db.set_cover(book_id, &format!("{base_name}.jpg")).await?;
    cleanup.commit();
    Ok(())
}

/// Encerra e aguarda o renderizador ao exceder o prazo; cancelamento tambem o mata.
async fn run_renderer(command: &mut Command, timeout: Duration) -> AppResult<ExitStatus> {
    let mut child = command
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| AppError::Internal(format!("renderizador indisponivel: {error}")))?;
    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(status) => Ok(status?),
        Err(_) => {
            child.kill().await?;
            Err(AppError::BadRequest(
                "tempo limite excedido ao gerar capa".into(),
            ))
        }
    }
}

#[cfg(test)]
#[path = "../tests/rust/cover-timeout.rs"]
mod regression_tests;

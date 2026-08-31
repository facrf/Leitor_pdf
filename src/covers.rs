use std::path::Path;

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
    tokio::fs::create_dir_all(&config.covers_dir).await?;
    let base_name = format!("auto-{book_id}-{}", Utc::now().timestamp_millis());
    let output_base = config.covers_dir.join(&base_name);
    let output_file = config.covers_dir.join(format!("{base_name}.jpg"));
    let status = Command::new("nice")
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
        .arg(&output_base)
        .status()
        .await
        .map_err(|error| AppError::Internal(format!("pdftoppm indisponivel: {error}")))?;
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
    Ok(())
}

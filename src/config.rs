use std::{env, path::PathBuf};

use crate::error::{AppError, AppResult};

/// Configuracao operacional carregada somente de variaveis de ambiente.
#[derive(Clone, Debug)]
pub struct Config {
    pub bind: String,
    pub initial_library_root: PathBuf,
    pub database_path: PathBuf,
    pub covers_dir: PathBuf,
    pub branding_dir: PathBuf,
    pub backup_dir: PathBuf,
    pub google_books_api_key: Option<String>,
    pub auth_username: Option<String>,
    pub auth_password: Option<String>,
    pub cover_timeout_seconds: u64,
}

impl Config {
    /// Carrega o ambiente e recusa valores invalidos antes de iniciar o servidor.
    pub fn from_env() -> AppResult<Self> {
        let cover_timeout_seconds = env::var("COVER_TIMEOUT_SECONDS")
            .unwrap_or_else(|_| "60".into())
            .parse()
            .map_err(|_| AppError::BadRequest("COVER_TIMEOUT_SECONDS deve ser inteiro".into()))?;
        let config = Self {
            cover_timeout_seconds,
            bind: env::var("APP_BIND").unwrap_or_else(|_| "0.0.0.0:20000".into()),
            initial_library_root: env::var("LIBRARY_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/pdf")),
            database_path: env::var("DATABASE_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("./data/library.db")),
            covers_dir: env::var("COVERS_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("./data/covers")),
            branding_dir: env::var("BRANDING_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("./data/branding")),
            backup_dir: env::var("BACKUP_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("./data/backups")),
            google_books_api_key: env::var("GOOGLE_BOOKS_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            auth_username: env::var("AUTH_USERNAME")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            auth_password: env::var("AUTH_PASSWORD")
                .ok()
                .filter(|value| !value.is_empty()),
        };
        config.validate()?;
        Ok(config)
    }

    /// Recusa configuracao parcial de autenticacao e limites impraticaveis.
    pub fn validate(&self) -> AppResult<()> {
        if self.auth_username.is_some() != self.auth_password.is_some() {
            return Err(AppError::BadRequest(
                "defina AUTH_USERNAME e AUTH_PASSWORD juntos, ou deixe ambos vazios".into(),
            ));
        }
        if self
            .auth_username
            .as_deref()
            .is_some_and(|value| value.contains(':'))
        {
            return Err(AppError::BadRequest(
                "AUTH_USERNAME nao pode conter dois-pontos".into(),
            ));
        }
        if !(1..=600).contains(&self.cover_timeout_seconds) {
            return Err(AppError::BadRequest(
                "COVER_TIMEOUT_SECONDS deve ficar entre 1 e 600".into(),
            ));
        }
        Ok(())
    }

    pub fn auth_enabled(&self) -> bool {
        self.auth_username.is_some() && self.auth_password.is_some()
    }
}

#[cfg(test)]
#[path = "../tests/rust/config-validation.rs"]
mod regression_tests;

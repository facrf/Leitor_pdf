use std::{env, path::PathBuf};

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
}

impl Config {
    pub fn from_env() -> Self {
        Self {
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
        }
    }

    pub fn auth_enabled(&self) -> bool {
        self.auth_username.is_some() && self.auth_password.is_some()
    }
}

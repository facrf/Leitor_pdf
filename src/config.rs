use std::{env, path::PathBuf};

/// Configuracao operacional carregada somente de variaveis de ambiente.
#[derive(Clone, Debug)]
pub struct Config {
    pub bind: String,
    pub initial_library_root: PathBuf,
    pub database_path: PathBuf,
    pub covers_dir: PathBuf,
    pub google_books_api_key: Option<String>,
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
            google_books_api_key: env::var("GOOGLE_BOOKS_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty()),
        }
    }
}

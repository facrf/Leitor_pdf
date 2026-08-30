use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("recurso nao encontrado")]
    NotFound,
    #[error("entrada invalida: {0}")]
    BadRequest(String),
    #[error("acesso a rede esta desativado nas configuracoes de privacidade")]
    NetworkDisabled,
    #[error("formato nao suportado para leitura: {0}")]
    Unsupported(String),
    #[error("banco de dados: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("arquivo: {0}")]
    Io(#[from] std::io::Error),
    #[error("arquivo compactado: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("servico de metadados: {0}")]
    Http(#[from] reqwest::Error),
    #[error("erro interno: {0}")]
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::BadRequest(_) | Self::Unsupported(_) => StatusCode::BAD_REQUEST,
            Self::NetworkDisabled => StatusCode::FORBIDDEN,
            Self::Database(_) | Self::Io(_) | Self::Zip(_) | Self::Internal(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            Self::Http(_) => StatusCode::BAD_GATEWAY,
        };
        let public_message = self.to_string();
        (status, Json(json!({ "error": public_message }))).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;

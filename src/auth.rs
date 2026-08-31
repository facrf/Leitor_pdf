use axum::{
    body::Body,
    extract::State,
    http::{header, Request, StatusCode},
    middleware::Next,
    response::Response,
};
use base64::{engine::general_purpose::STANDARD, Engine};

use crate::config::Config;

/// Autenticacao HTTP Basic opcional. E adequada para VPN/LAN e deve ser usada
/// atras de TLS quando a conexao sair da maquina local.
pub async fn require_auth(
    State(config): State<Config>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let path = request.uri().path();
    if !config.auth_enabled() || path == "/health" || path.starts_with("/public/") {
        return next.run(request).await;
    }
    let authenticated = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Basic "))
        .and_then(|encoded| STANDARD.decode(encoded).ok())
        .and_then(|decoded| String::from_utf8(decoded).ok())
        .and_then(|credentials| {
            credentials
                .split_once(':')
                .map(|(username, password)| (username.to_string(), password.to_string()))
        })
        .is_some_and(|(username, password)| {
            constant_time_eq(
                username.as_bytes(),
                config
                    .auth_username
                    .as_deref()
                    .unwrap_or_default()
                    .as_bytes(),
            ) && constant_time_eq(
                password.as_bytes(),
                config
                    .auth_password
                    .as_deref()
                    .unwrap_or_default()
                    .as_bytes(),
            )
        });
    if authenticated {
        return next.run(request).await;
    }
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(
            header::WWW_AUTHENTICATE,
            "Basic realm=\"Estante Livre\", charset=\"UTF-8\"",
        )
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{\"error\":\"autenticacao obrigatoria\"}"))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let max_len = left.len().max(right.len());
    let mut difference = left.len() ^ right.len();
    for index in 0..max_len {
        difference |= usize::from(*left.get(index).unwrap_or(&0) ^ *right.get(index).unwrap_or(&0));
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_credentials_without_early_exit() {
        assert!(constant_time_eq(b"facrf", b"facrf"));
        assert!(!constant_time_eq(b"facrf", b"facrg"));
        assert!(!constant_time_eq(b"curto", b"mais-longo"));
    }
}

use serde::Deserialize;

use crate::{
    error::{AppError, AppResult},
    models::{MetadataCandidate, Provider},
};

pub async fn search(
    client: &reqwest::Client,
    provider: &Provider,
    query: &str,
    google_api_key: Option<&str>,
) -> AppResult<Vec<MetadataCandidate>> {
    if query.trim().is_empty() {
        return Err(AppError::BadRequest("informe titulo, autor ou ISBN".into()));
    }
    match provider.kind.as_str() {
        "open_library" => search_open_library(client, provider, query).await,
        "google_books" => search_google_books(client, provider, query, google_api_key).await,
        kind => Err(AppError::BadRequest(format!(
            "tipo de fonte desconhecido: {kind}"
        ))),
    }
}

async fn search_open_library(
    client: &reqwest::Client,
    provider: &Provider,
    query: &str,
) -> AppResult<Vec<MetadataCandidate>> {
    let endpoint = format!("{}/search.json", provider.base_url.trim_end_matches('/'));
    let response = client
        .get(endpoint)
        .query(&[("q", query), ("limit", "12")])
        .send()
        .await?
        .error_for_status()?
        .json::<OpenLibraryResponse>()
        .await?;
    Ok(response
        .docs
        .into_iter()
        .map(|item| {
            let cover_url = item
                .cover_i
                .map(|id| format!("https://covers.openlibrary.org/b/id/{id}-L.jpg"));
            MetadataCandidate {
                provider: provider.name.clone(),
                provider_id: item.key,
                title: item.title,
                authors: item.author_name.unwrap_or_default(),
                description: None,
                publisher: item.publisher.and_then(|values| values.into_iter().next()),
                published_date: item.first_publish_year.map(|year| year.to_string()),
                isbn: preferred_isbn(item.isbn.unwrap_or_default()),
                language: item.language.and_then(|values| values.into_iter().next()),
                subjects: item
                    .subject
                    .unwrap_or_default()
                    .into_iter()
                    .take(12)
                    .collect(),
                cover_url,
            }
        })
        .collect())
}

async fn search_google_books(
    client: &reqwest::Client,
    provider: &Provider,
    query: &str,
    api_key: Option<&str>,
) -> AppResult<Vec<MetadataCandidate>> {
    let endpoint = format!("{}/volumes", provider.base_url.trim_end_matches('/'));
    let mut request = client
        .get(endpoint)
        .query(&[("q", query), ("maxResults", "12")]);
    if let Some(key) = api_key {
        request = request.query(&[("key", key)]);
    }
    let response = request
        .send()
        .await?
        .error_for_status()?
        .json::<GoogleBooksResponse>()
        .await?;
    Ok(response
        .items
        .unwrap_or_default()
        .into_iter()
        .map(|item| {
            let info = item.volume_info;
            let identifiers = info.industry_identifiers.unwrap_or_default();
            let isbn = identifiers
                .iter()
                .find(|value| value.kind == "ISBN_13")
                .or_else(|| identifiers.first())
                .map(|value| value.identifier.clone());
            let cover_url = info
                .image_links
                .and_then(|links| links.thumbnail.or(links.small_thumbnail))
                .map(|url| url.replacen("http://", "https://", 1));
            MetadataCandidate {
                provider: provider.name.clone(),
                provider_id: Some(item.id),
                title: info.title,
                authors: info.authors.unwrap_or_default(),
                description: info.description,
                publisher: info.publisher,
                published_date: info.published_date,
                isbn,
                language: info.language,
                subjects: info.categories.unwrap_or_default(),
                cover_url,
            }
        })
        .collect())
}

pub async fn download_cover(
    _client: &reqwest::Client,
    url: &str,
) -> AppResult<(Vec<u8>, &'static str)> {
    let parsed = validate_public_http_url(url)?;
    let response = public_get(parsed).await?;
    if response
        .content_length()
        .is_some_and(|length| length > 10 * 1024 * 1024)
    {
        return Err(AppError::BadRequest("capa grande demais".into()));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let extension = if content_type.contains("png") {
        "png"
    } else if content_type.contains("webp") {
        "webp"
    } else if content_type.contains("gif") {
        "gif"
    } else if content_type.contains("jpeg") || content_type.contains("jpg") {
        "jpg"
    } else {
        return Err(AppError::BadRequest(
            "a resposta da capa nao e uma imagem aceita".into(),
        ));
    };
    let bytes = limited_body(response, 10 * 1024 * 1024).await?;
    Ok((bytes, extension))
}

/// Limita memoria durante a leitura, mesmo sem Content-Length.
pub async fn limited_body(mut response: reqwest::Response, limit: usize) -> AppResult<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(AppError::BadRequest(
            "resposta externa grande demais".into(),
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(AppError::BadRequest(
                "resposta externa grande demais".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Resolve, valida e fixa os IPs por salto; redirects nunca usam DNS novo
/// sem validacao. Proxies de ambiente sao desativados neste caminho publico.
pub async fn public_get(mut url: url::Url) -> AppResult<reqwest::Response> {
    for hop in 0..=3 {
        validate_public_http_url(url.as_str())?;
        let host = url
            .host_str()
            .ok_or_else(|| AppError::BadRequest("URL sem host".into()))?;
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let port = url
            .port_or_known_default()
            .ok_or_else(|| AppError::BadRequest("porta invalida".into()))?;
        let addresses: Vec<_> = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            tokio::net::lookup_host((host, port)),
        )
        .await
        .map_err(|_| AppError::BadRequest("tempo DNS excedido".into()))??
        .collect();
        if addresses.is_empty() || addresses.iter().any(|address| is_private_ip(address.ip())) {
            return Err(AppError::BadRequest("destino de rede nao publico".into()));
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .resolve_to_addrs(host, &addresses)
            .user_agent(concat!("EstanteLivre/", env!("CARGO_PKG_VERSION")))
            .build()?;
        let response = client.get(url.clone()).send().await?;
        if response.status().is_redirection() {
            if hop == 3 {
                return Err(AppError::BadRequest("redirecionamentos demais".into()));
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| AppError::BadRequest("redirecionamento sem destino".into()))?;
            url = url
                .join(location)
                .map_err(|_| AppError::BadRequest("destino invalido".into()))?;
        } else {
            return Ok(response.error_for_status()?);
        }
    }
    Err(AppError::BadRequest("redirecionamentos demais".into()))
}

pub fn validate_public_http_url(url: &str) -> AppResult<url::Url> {
    let parsed =
        url::Url::parse(url).map_err(|_| AppError::BadRequest("URL externa invalida".into()))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AppError::BadRequest("URL deve usar HTTP(S)".into()));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(AppError::BadRequest("credenciais na URL recusadas".into()));
    }
    if let Some(host) = parsed.host_str() {
        if host.eq_ignore_ascii_case("localhost") || host.ends_with(".local") {
            return Err(AppError::BadRequest("host local recusado".into()));
        }
        if let Ok(ip) = host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
        {
            if ip.is_loopback() || ip.is_unspecified() || is_private_ip(ip) {
                return Err(AppError::BadRequest("endereco privado recusado".into()));
            }
        }
    }
    Ok(parsed)
}

fn is_private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(value) => {
            let [a, b, _, _] = value.octets();
            value.is_private()
                || value.is_link_local()
                || value.is_broadcast()
                || value.is_loopback()
                || value.is_unspecified()
                || value.is_multicast()
                || value.is_documentation()
                || a == 0
                || a >= 240
                || (a == 100 && (64..=127).contains(&b))
                || (a == 198 && (18..=19).contains(&b))
                || (a == 192 && b == 0)
        }
        std::net::IpAddr::V6(value) => {
            let segments = value.segments();
            // Somente unicast global; recusa mapeados IPv4 e faixas especiais.
            segments[0] & 0xe000 != 0x2000
                || (segments[0] == 0x2001 && (segments[1] <= 0x1ff || segments[1] == 0xdb8))
                || segments[0] == 0x2002
        }
    }
}

fn preferred_isbn(values: Vec<String>) -> Option<String> {
    values
        .iter()
        .find(|value| value.len() == 13)
        .cloned()
        .or_else(|| values.into_iter().next())
}

#[derive(Debug, Deserialize)]
struct OpenLibraryResponse {
    docs: Vec<OpenLibraryDoc>,
}

#[derive(Debug, Deserialize)]
struct OpenLibraryDoc {
    key: Option<String>,
    title: String,
    author_name: Option<Vec<String>>,
    first_publish_year: Option<i32>,
    isbn: Option<Vec<String>>,
    language: Option<Vec<String>>,
    subject: Option<Vec<String>>,
    publisher: Option<Vec<String>>,
    cover_i: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct GoogleBooksResponse {
    items: Option<Vec<GoogleBook>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleBook {
    id: String,
    volume_info: GoogleVolumeInfo,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleVolumeInfo {
    title: String,
    authors: Option<Vec<String>>,
    publisher: Option<String>,
    published_date: Option<String>,
    description: Option<String>,
    industry_identifiers: Option<Vec<IndustryIdentifier>>,
    categories: Option<Vec<String>>,
    language: Option<String>,
    image_links: Option<ImageLinks>,
}

#[derive(Debug, Deserialize)]
struct IndustryIdentifier {
    #[serde(rename = "type")]
    kind: String,
    identifier: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImageLinks {
    small_thumbnail: Option<String>,
    thumbnail: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_non_public_literal_addresses() {
        for address in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "224.0.0.1",
            "::1",
            "::ffff:127.0.0.1",
            "fc00::1",
            "fe80::1",
        ] {
            assert!(is_private_ip(address.parse().unwrap()), "{address}");
        }
        for url in [
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://127.0.0.1/",
            "http://user:secret@example.com/",
        ] {
            assert!(validate_public_http_url(url).is_err(), "{url}");
        }
        assert!(!is_private_ip("8.8.8.8".parse().unwrap()));
        assert!(!is_private_ip("2606:4700:4700::1111".parse().unwrap()));
    }

    #[tokio::test]
    async fn limits_responses_without_content_length() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for (body, accepted) in [("abc", true), ("abcde", false)] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 4096];
                let bytes_read = socket.read(&mut request).await.unwrap();
                assert!(bytes_read > 0, "cliente encerrou a conexao sem requisicao");
                socket
                    .write_all(
                        format!("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{body}").as_bytes(),
                    )
                    .await
                    .unwrap();
            });
            let client = reqwest::Client::builder()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .unwrap();
            let response = client
                .get(format!("http://{address}/"))
                .send()
                .await
                .unwrap();
            assert!(response.content_length().is_none());
            assert_eq!(limited_body(response, 4).await.is_ok(), accepted);
            server.await.unwrap();
        }
    }
}

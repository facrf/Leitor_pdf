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
    client: &reqwest::Client,
    url: &str,
) -> AppResult<(Vec<u8>, &'static str)> {
    let parsed =
        url::Url::parse(url).map_err(|_| AppError::BadRequest("URL de capa invalida".into()))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AppError::BadRequest("URL de capa deve usar HTTP(S)".into()));
    }
    if let Some(host) = parsed.host_str() {
        if host.eq_ignore_ascii_case("localhost") || host.ends_with(".local") {
            return Err(AppError::BadRequest("host de capa local recusado".into()));
        }
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            if ip.is_loopback() || ip.is_unspecified() || is_private_ip(ip) {
                return Err(AppError::BadRequest(
                    "endereco de capa privado recusado".into(),
                ));
            }
        }
    }
    let response = client.get(parsed).send().await?.error_for_status()?;
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
    let bytes = response.bytes().await?;
    if bytes.len() > 10 * 1024 * 1024 {
        return Err(AppError::BadRequest("capa grande demais".into()));
    }
    Ok((bytes.to_vec(), extension))
}

fn is_private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(value) => {
            value.is_private() || value.is_link_local() || value.is_broadcast()
        }
        std::net::IpAddr::V6(value) => value.is_unique_local() || value.is_unicast_link_local(),
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

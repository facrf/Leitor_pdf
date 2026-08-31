use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct BookSummary {
    pub id: i64,
    pub title: String,
    pub author: Option<String>,
    pub filename: String,
    pub format: String,
    pub subjects: Vec<String>,
    pub publisher: Option<String>,
    pub published_date: Option<String>,
    pub language: Option<String>,
    pub size: i64,
    pub is_available: bool,
    pub has_cover: bool,
    pub progress_percent: f64,
    pub updated_at: String,
}

/// Valores disponiveis para organizar e filtrar a estante.
#[derive(Debug, Clone, Serialize)]
pub struct CatalogFacets {
    pub formats: Vec<String>,
    pub authors: Vec<String>,
    pub subjects: Vec<String>,
    pub publishers: Vec<String>,
    pub languages: Vec<String>,
    pub years: Vec<String>,
    pub collections: Vec<Collection>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CatalogQuery {
    pub q: Option<String>,
    pub format: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub publisher: Option<String>,
    pub language: Option<String>,
    pub year: Option<String>,
    pub min_size: Option<i64>,
    pub max_size: Option<i64>,
    pub progress: Option<String>,
    pub availability: Option<String>,
    pub collection_id: Option<i64>,
    pub sort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collection {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub book_count: usize,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveCollection {
    pub name: String,
    #[serde(default = "default_collection_color")]
    pub color: String,
}

fn default_collection_color() -> String {
    "#6e816a".into()
}

#[derive(Debug, Clone, Serialize)]
pub struct DuplicateBook {
    pub id: i64,
    pub title: String,
    pub filename: String,
    pub relative_path: String,
    pub size: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DuplicateGroup {
    pub fingerprint: String,
    pub size: i64,
    pub books: Vec<DuplicateBook>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthIssue {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LibraryHealth {
    pub total: usize,
    pub available: usize,
    pub unavailable: usize,
    pub without_author: usize,
    pub without_subject: usize,
    pub without_cover: usize,
    pub without_language: usize,
    pub duplicate_groups: usize,
    pub scan_issues: Vec<HealthIssue>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Book {
    pub id: i64,
    pub title: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub published_date: Option<String>,
    pub isbn: Option<String>,
    pub language: Option<String>,
    pub subjects: Vec<String>,
    pub filename: String,
    pub relative_path: String,
    pub format: String,
    pub size: i64,
    pub modified_at: i64,
    pub page_count: Option<i64>,
    pub is_available: bool,
    pub has_cover: bool,
    pub created_at: String,
    pub updated_at: String,
    pub progress: Option<ReadingProgress>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateBook {
    pub title: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub published_date: Option<String>,
    pub isbn: Option<String>,
    pub language: Option<String>,
    #[serde(default)]
    pub subjects: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingProgress {
    pub location: serde_json::Value,
    pub percent: f64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveProgress {
    pub location: serde_json::Value,
    pub percent: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Note {
    pub id: i64,
    pub book_id: i64,
    pub location: serde_json::Value,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveNote {
    #[serde(default)]
    pub location: serde_json::Value,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Provider {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub enabled: bool,
    pub builtin: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveProvider {
    pub name: String,
    pub kind: String,
    pub base_url: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataCandidate {
    pub provider: String,
    pub provider_id: Option<String>,
    pub title: String,
    pub authors: Vec<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub published_date: Option<String>,
    pub isbn: Option<String>,
    pub language: Option<String>,
    #[serde(default)]
    pub subjects: Vec<String>,
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Share {
    pub id: i64,
    pub book_id: i64,
    pub token: String,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateShare {
    pub expires_in_hours: Option<u32>,
}

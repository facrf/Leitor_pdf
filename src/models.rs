use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct BookSummary {
    pub id: i64,
    pub title: String,
    pub author: Option<String>,
    pub filename: String,
    pub format: String,
    pub size: i64,
    pub is_available: bool,
    pub has_cover: bool,
    pub progress_percent: f64,
    pub updated_at: String,
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

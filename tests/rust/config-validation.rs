use super::*;

fn fixture_config() -> Config {
    Config {
        bind: "127.0.0.1:20000".into(),
        initial_library_root: "tests/fixtures/e2e".into(),
        database_path: "tests/runtime-data-config/library.db".into(),
        covers_dir: "data/covers".into(),
        branding_dir: "data/branding".into(),
        backup_dir: "data/backups".into(),
        google_books_api_key: None,
        auth_username: None,
        auth_password: None,
        cover_timeout_seconds: 60,
    }
}

#[test]
fn authentication_requires_both_credentials_and_a_valid_username() {
    let mut config = fixture_config();
    config.validate().unwrap();
    assert!(!config.auth_enabled());
    config.auth_username = Some("reader".into());
    assert!(config.validate().is_err());
    config.auth_password = Some("synthetic-password".into());
    config.validate().unwrap();
    assert!(config.auth_enabled());
    config.auth_username = None;
    assert!(config.validate().is_err());
    config.auth_username = Some("reader:invalid".into());
    assert!(config.validate().is_err());
}

#[test]
fn cover_deadline_must_be_bounded() {
    let mut config = fixture_config();
    for seconds in [1, 60, 600] {
        config.cover_timeout_seconds = seconds;
        config.validate().unwrap();
    }
    for seconds in [0, 601, u64::MAX] {
        config.cover_timeout_seconds = seconds;
        assert!(config.validate().is_err());
    }
}

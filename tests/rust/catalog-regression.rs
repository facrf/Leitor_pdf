use super::*;
use serde_json::json;

fn fixture_database() -> Database {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    migrate(&conn).unwrap();
    for id in 1..=205 {
        conn.execute("INSERT INTO books(id,title,filename,relative_path,format,size,modified_at,created_at,updated_at) VALUES(?1,'Same title',?2,?2,'txt',1,1,'now','now')", params![id, format!("book-{id}.txt")]).unwrap();
    }
    Database::from_connection(conn).unwrap()
}

#[tokio::test]
async fn catalog_counts_filters_and_pages_with_stable_ties() {
    let db = fixture_database();
    let first = db.list_books(&CatalogQuery::default()).await.unwrap();
    assert_eq!(first.total, 205);
    assert_eq!(first.books.len(), 60);
    assert_eq!(first.books[0].id, 1);
    let next = db
        .list_books(&CatalogQuery {
            offset: Some(60),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(next.books[0].id, 61);
    assert_eq!(next.total, 205);
    let last = db
        .list_books(&CatalogQuery {
            offset: Some(200),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(last.books.len(), 5);
    let filtered = db
        .list_books(&CatalogQuery {
            format: Some("epub".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(filtered.total, 0);
    assert!(filtered.books.is_empty());
    for limit in [0, 201] {
        assert!(db
            .list_books(&CatalogQuery {
                limit: Some(limit),
                ..Default::default()
            })
            .await
            .is_err());
    }
}

#[tokio::test]
async fn accepted_notes_and_progress_pass_restore_validation() {
    let db = fixture_database();
    let note: SaveNote = serde_json::from_value(json!({"content":"note"})).unwrap();
    db.create_note(1, &note).await.unwrap();
    for location in [json!([]), json!({"page":"bad"}), json!({"percent":101})] {
        assert!(db
            .create_note(
                1,
                &SaveNote {
                    content: "bad".into(),
                    location: location.clone()
                }
            )
            .await
            .is_err());
        assert!(db
            .save_progress(
                1,
                &SaveProgress {
                    percent: 37.0,
                    location
                }
            )
            .await
            .is_err());
    }
    db.save_progress(
        1,
        &SaveProgress {
            percent: 37.0,
            location: json!({"type":"percent","percent":37}),
        },
    )
    .await
    .unwrap();
    db.run(|conn| validate_restore_data(conn)).await.unwrap();
}

#[tokio::test]
async fn metadata_and_cover_change_together_and_validation_preserves_old_values() {
    let db = fixture_database();
    db.set_cover(1, "old.jpg").await.unwrap();
    let mut update: UpdateBook =
        serde_json::from_value(json!({"title":"New title","author":"New author"})).unwrap();
    update.title = "".into();
    assert!(db
        .update_book_with_cover(1, &update, Some("new.jpg"))
        .await
        .is_err());
    assert_eq!(db.book(1).await.unwrap().title, "Same title");
    assert_eq!(db.cover_filename(1).await.unwrap(), "old.jpg");
    update.title = "New title".into();
    db.update_book_with_cover(1, &update, Some("new.jpg"))
        .await
        .unwrap();
    assert_eq!(db.book(1).await.unwrap().title, "New title");
    assert_eq!(db.cover_filename(1).await.unwrap(), "new.jpg");
    db.update_book(1, &update).await.unwrap();
    assert_eq!(db.cover_filename(1).await.unwrap(), "new.jpg");
}

#[tokio::test]
async fn providers_reject_private_or_internal_addresses() {
    let db = fixture_database();
    for bad_url in [
        "http://127.0.0.1:8080",
        "http://localhost:5000",
        "http://169.254.169.254/metadata",
        "http://10.0.0.1/books",
    ] {
        let provider = SaveProvider {
            name: "Fonte insegura".into(),
            kind: "open_library".into(),
            base_url: bad_url.into(),
            enabled: true,
        };
        assert!(db.add_provider(&provider).await.is_err(), "{bad_url}");
    }
    let valid_provider = SaveProvider {
        name: "Fonte publica".into(),
        kind: "open_library".into(),
        base_url: "https://example.org/api".into(),
        enabled: true,
    };
    assert!(db.add_provider(&valid_provider).await.is_ok());
}

#[tokio::test]
async fn facets_extract_subjects_and_years_correctly() {
    let db = fixture_database();
    db.run(|conn| {
        conn.execute(
            "UPDATE books SET subjects = '[\"Ficção\", \"Aventura\"]', published_date = '1984-05-12' WHERE id = 1",
            [],
        ).unwrap();
        conn.execute(
            "UPDATE books SET subjects = '[\"Ficção\", \"Drama\"]', published_date = '2001' WHERE id = 2",
            [],
        ).unwrap();
        Ok(())
    }).await.unwrap();
    let facets = db.catalog_facets().await.unwrap();
    assert_eq!(facets.subjects, vec!["Aventura", "Drama", "Ficção"]);
    assert_eq!(facets.years, vec!["2001", "1984"]);
}

#[tokio::test]
async fn fts5_indexing_and_search_regression() {
    let db = fixture_database();
    let status_before = db.fts_status().await.unwrap();
    assert_eq!(status_before.indexed_books, 0);
    assert_eq!(status_before.total_books, 205);

    db.index_book_content(
        1,
        "Dom Casmurro",
        Some("Machado de Assis"),
        "[\"Literatura Brasileira\"]",
        "Capitu traiu ou não traiu Bento Santiago? Uma tragédia de ciúmes.",
    )
    .await
    .unwrap();

    let status_after = db.fts_status().await.unwrap();
    assert_eq!(status_after.indexed_books, 1);

    let search_traiu = db
        .list_books(&CatalogQuery {
            q: Some("traiu".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(search_traiu.total, 1);
    assert_eq!(search_traiu.books[0].id, 1);

    let search_tragedia = db
        .list_books(&CatalogQuery {
            q: Some("tragedia".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(search_tragedia.total, 1);
    assert_eq!(search_tragedia.books[0].id, 1);

    let search_res = db.search_content("ciumes", 10, 0).await.unwrap();
    assert_eq!(search_res.total, 1);
    assert_eq!(search_res.results.len(), 1);
    assert_eq!(search_res.results[0].book.id, 1);
    assert!(search_res.results[0]
        .snippet
        .as_ref()
        .unwrap()
        .contains("<mark>"));

    let search_content_only = db
        .list_books(&CatalogQuery {
            q: Some("Same title".into()),
            content_only: Some(true),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(search_content_only.total, 0);

    for weird_query in ["& * %", "ciúmes (teste) \"", "NOT OR AND : *"] {
        assert!(db
            .list_books(&CatalogQuery {
                q: Some(weird_query.into()),
                ..Default::default()
            })
            .await
            .is_ok());
    }

    db.unindex_book_content(1).await.unwrap();
    let status_removed = db.fts_status().await.unwrap();
    assert_eq!(status_removed.indexed_books, 0);
}

#[tokio::test]
async fn media_types_filter_and_count_correctly() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    migrate(&conn).unwrap();
    conn.execute(
        "INSERT INTO books(id,title,filename,relative_path,format,size,modified_at,created_at,updated_at)
         VALUES (1,'Livro PDF','livro.pdf','livro.pdf','pdf',100,1,'now','now'),
                (2,'Audio Livro','audio.m4b','audio.m4b','m4b',200,1,'now','now'),
                (3,'Musica MP3','aula.mp3','aula.mp3','mp3',300,1,'now','now'),
                (4,'Video Aula','aula.mp4','aula.mp4','mp4',400,1,'now','now')",
        [],
    ).unwrap();
    let db = Database::from_connection(conn).unwrap();

    let facets = db.catalog_facets().await.unwrap();
    assert_eq!(facets.media_counts.books, 1);
    assert_eq!(facets.media_counts.audiobooks, 2);
    assert_eq!(facets.media_counts.videos, 1);

    let books = db
        .list_books(&CatalogQuery {
            media_type: Some("book".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(books.total, 1);
    assert_eq!(books.books[0].format, "pdf");
    assert_eq!(books.books[0].media_type, "book");

    let audiobooks = db
        .list_books(&CatalogQuery {
            media_type: Some("audiobook".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(audiobooks.total, 2);
    assert!(audiobooks.books.iter().all(|b| b.media_type == "audiobook"));

    let videos = db
        .list_books(&CatalogQuery {
            media_type: Some("video".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(videos.total, 1);
    assert_eq!(videos.books[0].format, "mp4");
    assert_eq!(videos.books[0].media_type, "video");
}

#[tokio::test]
async fn course_info_groups_lessons_and_calculates_progress() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    migrate(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO books(id,title,filename,relative_path,format,size,modified_at,created_at,updated_at) VALUES
            (10,'Aula 01','01_intro.mp4','Cursos/Rust/01_intro.mp4','mp4',100,1,'now','now'),
            (11,'Aula 02','02_tipos.mp4','Cursos/Rust/02_tipos.mp4','mp4',120,1,'now','now'),
            (12,'Aula 10','10_macros.mp4','Cursos/Rust/10_macros.mp4','mp4',150,1,'now','now');
         INSERT INTO reading_progress(book_id,location,percent,updated_at) VALUES
            (10,'{\"type\":\"video\",\"time\":100.0,\"duration\":100.0}',100.0,'now');",
    ).unwrap();
    let db = Database::from_connection(conn).unwrap();

    let course = db.course_info(11).await.unwrap().expect("curso esperado");
    assert_eq!(course.course_name, "Rust");
    assert_eq!(course.parent_dir, "Cursos/Rust");
    assert_eq!(course.total_lessons, 3);
    assert_eq!(course.completed_lessons, 1);
    assert!((course.course_percent - 33.33).abs() < 1.0);
    // Ordenacao natural: 01, 02, 10
    assert_eq!(course.lessons[0].filename, "01_intro.mp4");
    assert!(course.lessons[0].completed);
    assert!(!course.lessons[0].is_current);

    assert_eq!(course.lessons[1].filename, "02_tipos.mp4");
    assert!(course.lessons[1].is_current);
    assert!(!course.lessons[1].completed);

    assert_eq!(course.lessons[2].filename, "10_macros.mp4");
}

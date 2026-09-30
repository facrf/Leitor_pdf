use super::*;
use serde_json::json;

fn fixture_database() -> Database {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    migrate(&conn).unwrap();
    for id in 1..=205 {
        conn.execute("INSERT INTO books(id,title,filename,relative_path,format,size,modified_at,created_at,updated_at) VALUES(?1,'Same title',?2,?2,'txt',1,1,'now','now')", params![id, format!("book-{id}.txt")]).unwrap();
    }
    Database {
        connection: Arc::new(Mutex::new(conn)),
    }
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
    let conn = db.connection.lock().await;
    validate_restore_data(&conn).unwrap();
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

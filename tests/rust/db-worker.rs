use super::*;

#[tokio::test(flavor = "current_thread")]
async fn sql_worker_does_not_block_async_executor_and_survives_cancellation() {
    let root = std::env::current_dir()
        .unwrap()
        .join(".cache/tmp")
        .join(format!("db-worker-{:032x}", rand::random::<u128>()));
    std::fs::create_dir_all(&root).unwrap();
    let db = Database::open(&root.join("library.db"), &root)
        .await
        .unwrap();
    let (started, ready) = tokio::sync::oneshot::channel();
    let cloned = db.clone();
    let task = tokio::spawn(async move {
        cloned
            .run(move |conn| {
                started.send(()).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(100));
                conn.execute(
                    "INSERT INTO settings(key,value) VALUES('worker-test','done')",
                    [],
                )?;
                Ok(())
            })
            .await
    });
    ready.await.unwrap();
    let now = std::time::Instant::now();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    assert!(now.elapsed() < std::time::Duration::from_millis(80));
    task.abort();
    assert_eq!(
        db.setting("worker-test").await.unwrap().as_deref(),
        Some("done")
    );
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}

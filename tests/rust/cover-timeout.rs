use super::*;

#[tokio::test]
async fn renderer_timeout_terminates_and_next_command_can_finish() {
    let mut stalled = Command::new("sleep");
    stalled.arg("10");
    let started = std::time::Instant::now();
    let error = run_renderer(&mut stalled, Duration::from_millis(30))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("tempo limite"));
    assert!(started.elapsed() < Duration::from_secs(3));
    let mut next = Command::new("true");
    assert!(run_renderer(&mut next, Duration::from_secs(2))
        .await
        .unwrap()
        .success());
}

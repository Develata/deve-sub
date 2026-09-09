use super::*;

#[tokio::test]
async fn thousands_of_completions_are_reclaimed_during_operation() {
    let supervisor = JobSupervisor::new();
    for _ in 0..10_000 {
        let (tx, rx) = tokio::sync::oneshot::channel();
        supervisor
            .spawn(async move {
                let _ = tx.send(());
            })
            .expect("admit");
        rx.await.expect("finished");
        tokio::task::yield_now().await;
        supervisor.reap_finished();
        assert!(supervisor.len() <= 1);
    }
    supervisor.shutdown(Duration::from_secs(1)).await;
    assert!(supervisor.is_empty());
}

#[tokio::test]
async fn panic_is_observed_and_reclaimed() {
    let supervisor = JobSupervisor::new();
    supervisor
        .spawn(async {
            panic!("test panic");
        })
        .expect("admit");
    tokio::task::yield_now().await;
    supervisor.reap_finished();
    assert_eq!(supervisor.panic_count(), 1);
    assert!(supervisor.is_empty());
}

#[tokio::test]
async fn admission_and_shutdown_are_bounded() {
    let supervisor = JobSupervisor::new();
    for _ in 0..MAX_JOBS {
        supervisor.spawn(std::future::pending()).expect("admit");
    }
    assert!(matches!(
        supervisor.spawn(async {}),
        Err(SpawnError::AtCapacity)
    ));
    assert_eq!(supervisor.len(), MAX_JOBS);
    supervisor.shutdown(Duration::from_millis(1)).await;
    assert_eq!(supervisor.cancellation_count(), MAX_JOBS as u64);
    assert!(supervisor.is_empty());
    assert!(matches!(
        supervisor.spawn(async {}),
        Err(SpawnError::ShuttingDown)
    ));
}

#[tokio::test]
async fn close_rejects_new_work_before_drain() {
    let supervisor = JobSupervisor::new();
    supervisor.close();
    assert!(matches!(
        supervisor.spawn(async {}),
        Err(SpawnError::ShuttingDown)
    ));
    supervisor.shutdown(Duration::ZERO).await;
}

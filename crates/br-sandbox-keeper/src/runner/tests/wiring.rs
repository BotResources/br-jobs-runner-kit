use bytes::Bytes;

use super::*;

fn shared_of(clock: &Arc<dyn Clock>) -> Shared {
    Shared {
        views: Views::new(),
        config: Arc::new(testing::config()),
        metrics: Arc::new(Metrics::new()),
        readiness: Arc::new(Readiness::starting()),
        leadership: Leadership::new(clock.clone()),
        shutdown: CancellationToken::new(),
    }
}

#[tokio::test]
async fn the_store_client_of_the_sweeper_writes_only_under_tombstones_and_touches_only_its_three_prefixes()
 {
    let clock: Arc<dyn Clock> = Arc::new(TokioClock::starting_at(at(START)));
    let bucket = Arc::new(FakeBucket::default());
    let sweeper = sweeper_of(&shared_of(&clock), bucket.clone(), clock);
    for key in ["saves/x", "records/x", "elsewhere/x"] {
        assert!(matches!(
            sweeper.bucket.put(key, Bytes::from_static(b"{}")).await,
            Err(BucketError::Forbidden { .. })
        ));
    }
    assert!(matches!(
        sweeper.bucket.get("elsewhere/x").await,
        Err(BucketError::Forbidden { .. })
    ));
    assert!(matches!(
        sweeper.bucket.delete("elsewhere/x").await,
        Err(BucketError::Forbidden { .. })
    ));
    assert!(matches!(
        sweeper.bucket.list_page("elsewhere/", None, 10).await,
        Err(BucketError::Forbidden { .. })
    ));
    sweeper
        .bucket
        .put("tombstones/x", Bytes::from_static(b"{}"))
        .await
        .unwrap();
    assert_eq!(bucket.writes(), vec!["put tombstones/x".to_owned()]);
}

#[tokio::test]
async fn the_sweeper_takes_its_pace_and_its_rules_from_the_configuration() {
    let clock: Arc<dyn Clock> = Arc::new(TokioClock::starting_at(at(START)));
    let shared = shared_of(&clock);
    let sweeper = sweeper_of(&shared, Arc::new(FakeBucket::default()), clock);
    assert_eq!(sweeper.interval, shared.config.sweep_interval);
    assert_eq!(sweeper.page_size, shared.config.sweep_page_size);
    assert_eq!(sweeper.rules.save_ttl, shared.config.save_ttl);
}

use std::time::Duration;

use super::*;
use crate::error::Error;
use crate::model::{Capacity, PresenceStatus};
use crate::test_double::{Call, PresenceRecord};

#[test]
fn given_a_capacity_below_one_then_it_cannot_be_made_and_one_or_more_can() {
    assert!(matches!(Capacity::new(0), Err(Error::InvalidInput(_))));
    assert_eq!(Capacity::new(1).map(Capacity::get), Ok(1));
    assert_eq!(Capacity::new(u32::MAX).map(Capacity::get), Ok(u32::MAX));
}

#[tokio::test]
async fn given_a_client_when_it_publishes_its_presence_then_the_version_the_status_and_the_capacity_are_sent()
 {
    let (jobs, client) = scripted();
    client
        .publish_presence(
            "1.2.3",
            PresenceStatus::Ready,
            Capacity::new(4).expect("cap"),
        )
        .await
        .expect("published");
    client
        .publish_presence(
            "1.2.4",
            PresenceStatus::Draining,
            Capacity::new(1).expect("cap"),
        )
        .await
        .expect("published");
    assert_eq!(
        jobs.presence_records(),
        vec![
            PresenceRecord {
                runner_version: "1.2.3".to_owned(),
                status: PresenceStatus::Ready,
                capacity: 4
            },
            PresenceRecord {
                runner_version: "1.2.4".to_owned(),
                status: PresenceStatus::Draining,
                capacity: 1
            },
        ]
    );
}

#[tokio::test]
async fn given_a_published_presence_when_it_is_deleted_then_the_delete_is_sent_once_and_the_ttl_is_given()
 {
    let (jobs, client) = scripted();
    client.delete_presence().await.expect("deleted");
    assert_eq!(jobs.presence_deletions(), 1);
    assert_eq!(
        client.presence_time_to_live().await,
        Ok(Duration::from_secs(90))
    );
    jobs.set_presence_ttl(Err(Error::InfraMissing("no ttl".to_owned())));
    assert!(matches!(
        client.presence_time_to_live().await,
        Err(Error::InfraMissing(_))
    ));
}

#[tokio::test]
async fn given_a_bucket_that_fails_when_the_presence_is_published_then_the_error_is_returned_and_it_is_not_sent_again()
 {
    let (jobs, client) = scripted();
    jobs.fail_next_presence(Error::wait_fired("no confirmation"));
    let failed = client
        .publish_presence("1", PresenceStatus::Ready, Capacity::new(1).expect("cap"))
        .await;
    assert!(matches!(failed, Err(Error::Lost(_))));
    let puts = jobs
        .calls()
        .into_iter()
        .filter(|call| matches!(call, Call::PresencePut))
        .count();
    assert_eq!(puts, 1);
    jobs.fail_next_presence(Error::Unauthenticated);
    assert_eq!(client.delete_presence().await, Err(Error::Unauthenticated));
}

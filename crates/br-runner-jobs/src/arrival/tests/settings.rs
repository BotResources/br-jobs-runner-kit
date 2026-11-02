use std::time::Duration;

use super::{scripted, work};
use crate::arrival::{ArrivalSettings, settings_of_tests};
use crate::error::Error;
use crate::test_double::Call;

#[tokio::test]
async fn given_a_zero_acknowledgement_wait_when_the_feed_is_opened_then_it_is_refused_with_invalid_input()
 {
    let jobs = scripted();
    let settings = ArrivalSettings {
        ack_wait: Duration::ZERO,
        ..settings_of_tests()
    };
    let refused = work(&jobs).arrival_feed(settings).await;
    assert!(matches!(refused, Err(Error::InvalidInput(_))));
}

#[tokio::test]
async fn given_a_zero_inactivity_limit_when_the_feed_is_opened_then_it_is_refused_with_invalid_input()
 {
    let jobs = scripted();
    let settings = ArrivalSettings {
        inactivity_limit: Duration::ZERO,
        ..settings_of_tests()
    };
    let refused = work(&jobs).arrival_feed(settings).await;
    assert!(matches!(refused, Err(Error::InvalidInput(_))));
}

#[tokio::test]
async fn given_settings_of_the_importer_when_the_feed_is_opened_then_the_backend_gets_those_values()
{
    let jobs = scripted();
    let settings = ArrivalSettings {
        ack_wait: Duration::from_secs(7),
        inactivity_limit: Duration::from_secs(90),
    };
    work(&jobs).arrival_feed(settings).await.expect("feed");
    let opened: Vec<_> = jobs
        .calls()
        .into_iter()
        .filter(|call| matches!(call, Call::ArrivalFeed(_)))
        .collect();
    assert_eq!(opened, vec![Call::ArrivalFeed(settings)]);
}

use std::future::Future;

pub(crate) async fn detach<T: Send + 'static>(
    work: impl Future<Output = T> + Send + 'static,
) -> Option<T> {
    match tokio::spawn(work).await {
        Ok(output) => Some(output),
        Err(failure) if failure.is_panic() => std::panic::resume_unwind(failure.into_panic()),
        Err(_) => None,
    }
}

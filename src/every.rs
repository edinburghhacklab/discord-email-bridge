use std::time::Duration;

use tokio::{sync::oneshot, time};
use tokio_stream::{StreamExt, wrappers::IntervalStream};

/// Every `duration`, call `func`. Return when `func` errors.
pub async fn every<Func, E>(duration: Duration, mut func: Func) -> Result<(), E>
where
    Func: AsyncFnMut() -> Result<(), E>,
{
    let mut stream = IntervalStream::new(time::interval(duration));

    while let Some(_ts) = stream.next().await {
        func().await?;
    }

    Ok(())
}

/// Every `duration`, call `func`. Return when `func` errors, or a message is sent to `shutdown`
pub async fn every_until<Func, Fut, E>(
    duration: Duration,
    mut func: Func,
    mut shutdown: oneshot::Receiver<()>,
) -> Result<(), E>
where
    Func: FnMut() -> Fut,
    Fut: Future<Output = Result<(), E>>,
{
    let mut stream = IntervalStream::new(time::interval(duration));

    loop {
        tokio::select! {
            Some(_ts) = stream.next() => {
                func().await?;
            }
            _ = &mut shutdown => {
                return Ok(());
            }
        }
    }
}

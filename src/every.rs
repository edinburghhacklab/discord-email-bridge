use std::time::Duration;

use tokio::time;
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

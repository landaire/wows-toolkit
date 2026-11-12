//! Following a background job's progress reports while it runs.

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use futures::future::Either;
use futures::future::select;

/// Awaits `work`, handing each report that arrives on `reports` to `report` as it comes, so a
/// view follows the work rather than waiting for it.
pub async fn follow<T, P>(
    work: impl Future<Output = T>,
    mut reports: UnboundedReceiver<P>,
    mut report: impl FnMut(P),
) -> T {
    let mut work = Box::pin(work);
    loop {
        match select(&mut work, reports.next()).await {
            Either::Left((finished, _)) => return finished,
            Either::Right((Some(step), _)) => report(step),
            // The sender went with the work, which is about to finish.
            Either::Right((None, _)) => return work.await,
        }
    }
}

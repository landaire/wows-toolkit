//! UI-bound channels built on `egui_inbox`, so background work wakes the UI
//! when it has something to show instead of the update loop polling on a
//! timer.
//!
//! Two gaps in plain `UiInbox` are covered here:
//! - `UiInbox` cannot tell when every sender is gone, but several consumers
//!   need mpsc's disconnect signal (a dropped or panicked worker must still
//!   finish its task entry). [`GuardedSender`] restores it by sending
//!   [`StreamEvent::Closed`] when the last sender clone drops.
//! - `UiInboxSender::send` wakes the UI per message, which would repaint at
//!   the producer's rate. Hot loops (downloads, directory walks) instead queue
//!   the item and defer the wake through `request_repaint_after`, so bursts
//!   coalesce into at most one repaint per delay window.

use std::sync::Arc;
use std::time::Duration;

use egui_inbox::SendError;
use egui_inbox::UiInbox;
use egui_inbox::UiInboxSender;

/// One message on a guarded channel.
pub enum StreamEvent<T> {
    Item(T),
    /// The last [`GuardedSender`] clone was dropped. Sent from `Drop`, so it
    /// arrives even when the worker panicked.
    Closed,
}

enum Wake {
    Immediate,
    /// Queue silently, then arm a deferred repaint so a burst of sends paints
    /// at most once per window.
    Deferred {
        ctx: egui::Context,
        delay: Duration,
    },
}

impl Wake {
    fn send<T>(&self, tx: &UiInboxSender<T>, item: T) -> Result<(), SendError<T>> {
        match self {
            Wake::Immediate => tx.send(item),
            Wake::Deferred { ctx, delay } => {
                tx.send_without_request_repaint(item)?;
                ctx.request_repaint_after(*delay);
                Ok(())
            }
        }
    }
}

/// Sender half of [`guarded_channel`]. Clonable; the channel closes when the
/// last clone drops.
pub struct GuardedSender<T> {
    shared: Arc<GuardedSenderShared<T>>,
}

struct GuardedSenderShared<T> {
    tx: UiInboxSender<StreamEvent<T>>,
    wake: Wake,
}

impl<T> Drop for GuardedSenderShared<T> {
    fn drop(&mut self) {
        // Not deferred: closing is rare and the consumer reacts to it
        // (clearing a progress bar, reaping a task), so it should paint now.
        let _ = self.tx.send(StreamEvent::Closed);
    }
}

impl<T> Clone for GuardedSender<T> {
    fn clone(&self) -> Self {
        Self { shared: Arc::clone(&self.shared) }
    }
}

impl<T> GuardedSender<T> {
    /// Queue an item and wake the UI per this channel's wake policy. Errs when
    /// the inbox was dropped, which means nobody is listening any more.
    pub fn send(&self, item: T) -> Result<(), SendError<T>> {
        self.shared.wake.send(&self.shared.tx, StreamEvent::Item(item)).map_err(|SendError(event)| match event {
            StreamEvent::Item(item) => SendError(item),
            // Only `Item` is ever passed in above.
            StreamEvent::Closed => unreachable!("send failures return the event that was sent"),
        })
    }
}

/// Channel whose receiver can distinguish "no message yet" from "every sender
/// is gone". Sends wake the UI only once the inbox has a context (its first
/// `read` registers one); use [`guarded_channel_with_ctx`] when the consumer
/// drains with `read_without_ctx`.
pub fn guarded_channel<T>() -> (GuardedSender<T>, UiInbox<StreamEvent<T>>) {
    let inbox = UiInbox::new();
    let sender = GuardedSender { shared: Arc::new(GuardedSenderShared { tx: inbox.sender(), wake: Wake::Immediate }) };
    (sender, inbox)
}

/// [`guarded_channel`] with the wake context registered up front, so every
/// send wakes the UI even when the consumer never reads with a context.
pub fn guarded_channel_with_ctx<T>(ctx: &egui::Context) -> (GuardedSender<T>, UiInbox<StreamEvent<T>>) {
    let inbox = UiInbox::new_with_ctx(ctx);
    let sender = GuardedSender { shared: Arc::new(GuardedSenderShared { tx: inbox.sender(), wake: Wake::Immediate }) };
    (sender, inbox)
}

/// [`guarded_channel`] for hot producers: items are queued immediately, but
/// repaints are deferred by `delay` so a tight send loop paints at most once
/// per window. The context is registered on the inbox as well, so `Closed`
/// wakes immediately even for consumers that drain with `read_without_ctx`.
pub fn guarded_channel_throttled<T>(
    ctx: egui::Context,
    delay: Duration,
) -> (GuardedSender<T>, UiInbox<StreamEvent<T>>) {
    let inbox = UiInbox::new_with_ctx(&ctx);
    let sender = GuardedSender {
        shared: Arc::new(GuardedSenderShared { tx: inbox.sender(), wake: Wake::Deferred { ctx, delay } }),
    };
    (sender, inbox)
}

/// Unguarded sender that defers repaints: every item lands in the inbox, but a
/// hot loop repaints at most once per `delay`. For progress streams whose
/// consumer only keeps the latest value and learns about completion elsewhere.
pub struct ThrottledSender<T> {
    tx: UiInboxSender<T>,
    ctx: egui::Context,
    delay: Duration,
}

impl<T> Clone for ThrottledSender<T> {
    fn clone(&self) -> Self {
        Self { tx: self.tx.clone(), ctx: self.ctx.clone(), delay: self.delay }
    }
}

impl<T> ThrottledSender<T> {
    /// Queue an item and arm a deferred repaint. Errs when the inbox was
    /// dropped, which means nobody is listening any more.
    pub fn send(&self, item: T) -> Result<(), SendError<T>> {
        self.tx.send_without_request_repaint(item)?;
        self.ctx.request_repaint_after(self.delay);
        Ok(())
    }
}

pub fn throttled_channel<T>(ctx: egui::Context, delay: Duration) -> (ThrottledSender<T>, UiInbox<T>) {
    let inbox = UiInbox::new();
    let sender = ThrottledSender { tx: inbox.sender(), ctx, delay };
    (sender, inbox)
}

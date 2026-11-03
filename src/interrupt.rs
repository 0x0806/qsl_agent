//! Cooperative cancellation for a running turn (Ctrl+C stops the turn, not
//! the program). Cloned handles share one flag.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;

#[derive(Clone, Default)]
pub struct Interrupt {
    flag: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl Interrupt {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn trigger(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_set(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    pub fn reset(&self) {
        self.flag.store(false, Ordering::SeqCst);
    }

    /// Resolves once `trigger` has been called (immediately if already set).
    pub async fn cancelled(&self) {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_set() {
                return;
            }
            notified.await;
        }
    }

    /// Sleep that ends early on interrupt. Returns false if interrupted.
    pub async fn sleep(&self, d: std::time::Duration) -> bool {
        tokio::select! {
            _ = tokio::time::sleep(d) => true,
            _ = self.cancelled() => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn trigger_wakes_waiter() {
        let i = Interrupt::new();
        let j = i.clone();
        let h = tokio::spawn(async move { j.cancelled().await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        i.trigger();
        tokio::time::timeout(Duration::from_secs(1), h)
            .await
            .unwrap()
            .unwrap();
        assert!(!i.sleep(Duration::from_secs(5)).await);
        i.reset();
        assert!(i.sleep(Duration::from_millis(1)).await);
    }
}

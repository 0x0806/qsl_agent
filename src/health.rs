use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::{warn, info};

use crate::config::HealthConfig;

#[derive(Clone)]
pub struct HealthMonitor {
    last_heartbeat_unix: Arc<AtomicU64>,
    step_count: Arc<AtomicU64>,
    cfg: HealthConfig,
    _tx: Arc<watch::Sender<u64>>,
}

impl HealthMonitor {
    pub fn spawn(cfg: HealthConfig, shutdown: CancellationToken) -> Self {
        let last = Arc::new(AtomicU64::new(now_unix()));
        let step_count = Arc::new(AtomicU64::new(0));
        let (tx, mut rx) = watch::channel(0u64);

        {
            let last = last.clone();
            let cfg = cfg.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(cfg.heartbeat_interval_s));
                loop {
                    tokio::select! {
                        _ = shutdown.cancelled() => break,
                        _ = interval.tick() => {
                            let idle_s = now_unix().saturating_sub(last.load(Ordering::Relaxed));
                            if idle_s > cfg.stall_threshold_s {
                                warn!(idle_s, "session stalled — no activity");
                                let _ = tx.send(idle_s);
                            } else {
                                info!(idle_s, "heartbeat ok");
                            }
                        }
                        _ = rx.changed() => {}
                    }
                }
            });
        }

        Self { last_heartbeat_unix: last, step_count, cfg, _tx: Arc::new(tx) }
    }

    pub fn beat(&self) {
        self.last_heartbeat_unix.store(now_unix(), Ordering::Relaxed);
        self.step_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn steps(&self) -> u64 { self.step_count.load(Ordering::Relaxed) }
    pub fn idle_seconds(&self) -> u64 {
        now_unix().saturating_sub(self.last_heartbeat_unix.load(Ordering::Relaxed))
    }
    pub fn is_stalled(&self) -> bool { self.idle_seconds() > self.cfg.stall_threshold_s }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

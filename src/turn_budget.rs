use std::time::{Duration, Instant};

pub struct TurnBudget {
    started: Instant,
    max: Duration,
}

impl TurnBudget {
    pub fn new(max: Duration) -> Self { Self { started: Instant::now(), max } }
    pub fn expired(&self) -> bool { self.started.elapsed() >= self.max }
    pub fn remaining(&self) -> Duration { self.max.saturating_sub(self.started.elapsed()) }
    pub fn max(&self) -> Duration { self.max }
}

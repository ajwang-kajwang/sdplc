//! Scan-cycle timing metrics for validation.

use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ScanTiming {
    target: Duration,
    cycles: u64,
    exec_total_us: f64,
    exec_max_us: f64,
    jitter_total_us: f64,
    jitter_max_us: f64,
    started_at: Instant,
}

impl ScanTiming {
    pub fn new(target: Duration) -> Self {
        Self {
            target,
            cycles: 0,
            exec_total_us: 0.0,
            exec_max_us: 0.0,
            jitter_total_us: 0.0,
            jitter_max_us: 0.0,
            started_at: Instant::now(),
        }
    }

    pub fn target(&self) -> Duration {
        self.target
    }

    pub fn record_cycle(&mut self, exec_time: Duration, total_cycle_time: Duration) {
        let exec_us = exec_time.as_secs_f64() * 1_000_000.0;
        let jitter_us = if total_cycle_time > self.target {
            (total_cycle_time - self.target).as_secs_f64() * 1_000_000.0
        } else {
            0.0
        };

        self.cycles += 1;
        self.exec_total_us += exec_us;
        self.exec_max_us = self.exec_max_us.max(exec_us);
        self.jitter_total_us += jitter_us;
        self.jitter_max_us = self.jitter_max_us.max(jitter_us);
    }

    pub fn cycles(&self) -> u64 {
        self.cycles
    }

    pub fn avg_exec_us(&self) -> f64 {
        if self.cycles == 0 {
            0.0
        } else {
            self.exec_total_us / self.cycles as f64
        }
    }

    pub fn max_exec_us(&self) -> f64 {
        self.exec_max_us
    }

    pub fn avg_jitter_us(&self) -> f64 {
        if self.cycles == 0 {
            0.0
        } else {
            self.jitter_total_us / self.cycles as f64
        }
    }

    pub fn max_jitter_us(&self) -> f64 {
        self.jitter_max_us
    }

    pub fn uptime(&self) -> Duration {
        self.started_at.elapsed()
    }

    pub fn csv_header() -> &'static str {
        "cycles,target_ms,avg_exec_us,max_exec_us,avg_jitter_us,max_jitter_us,uptime_s"
    }

    pub fn csv_row(&self) -> String {
        format!(
            "{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3}",
            self.cycles,
            self.target.as_secs_f64() * 1000.0,
            self.avg_exec_us(),
            self.max_exec_us(),
            self.avg_jitter_us(),
            self.max_jitter_us(),
            self.uptime().as_secs_f64(),
        )
    }
}

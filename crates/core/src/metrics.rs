//! CPU use from two observations of the same process identity.
//!
//! `sysinfo`'s own `cpu_usage` is not used: on macOS it divides by the time since the previous
//! refresh of *any* process in the table, and keeps its old value when a process used no CPU in
//! the interval. Here each identity keeps its own baseline (accumulated CPU time and the instant
//! it was read), so the figure is exactly "CPU time used / wall time elapsed" between two scans.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use crate::model::{CpuUsage, ProcessIdentity};

/// Shorter windows are dominated by the 1 ms resolution of the accumulated CPU time. A scan
/// that comes sooner reports the previous figure and keeps the older baseline.
pub const MIN_CPU_WINDOW: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy)]
struct Baseline {
    cpu_time_ms: u64,
    read_at: Instant,
    last: Option<f64>,
}

/// Baselines for the processes of the latest scan only, so it never outgrows the live set.
#[derive(Debug, Default)]
pub struct CpuSampler {
    baselines: HashMap<ProcessIdentity, Baseline>,
}

impl CpuSampler {
    /// CPU use for each identity read at `read_at`. `cpu_time_ms` is `None` when the OS withheld
    /// it. Identities absent from `readings` are forgotten.
    pub fn sample(
        &mut self,
        readings: &[(ProcessIdentity, Option<u64>)],
        read_at: Instant,
    ) -> HashMap<ProcessIdentity, CpuUsage> {
        let mut previous = std::mem::take(&mut self.baselines);
        let mut usage = HashMap::with_capacity(readings.len());
        for &(identity, cpu_time_ms) in readings {
            // Without a start time a recycled PID could not be told apart from the process the
            // baseline belongs to.
            let (Some(cpu_time_ms), Some(_)) = (cpu_time_ms, identity.started_at_secs) else {
                usage.insert(identity, CpuUsage::Unavailable);
                continue;
            };
            let current = Baseline {
                cpu_time_ms,
                read_at,
                last: None,
            };
            let (reported, kept) = match previous.remove(&identity) {
                None => (CpuUsage::WarmingUp, current),
                Some(earlier) if cpu_time_ms < earlier.cpu_time_ms => {
                    // Accumulated CPU time never decreases for one process; start over.
                    (CpuUsage::WarmingUp, current)
                }
                Some(earlier) => {
                    let window = read_at.saturating_duration_since(earlier.read_at);
                    if window < MIN_CPU_WINDOW {
                        let reported = earlier
                            .last
                            .map_or(CpuUsage::WarmingUp, |percent| CpuUsage::Sampled { percent });
                        (reported, earlier)
                    } else {
                        let percent =
                            percent_of_one_core(cpu_time_ms - earlier.cpu_time_ms, window);
                        (
                            CpuUsage::Sampled { percent },
                            Baseline {
                                last: Some(percent),
                                ..current
                            },
                        )
                    }
                }
            };
            usage.insert(identity, reported);
            self.baselines.insert(identity, kept);
        }
        usage
    }
}

/// Rounded to 0.1 %: finer digits would be noise at millisecond CPU-time resolution.
fn percent_of_one_core(cpu_ms: u64, window: Duration) -> f64 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "CPU milliseconds within one scan window are far below 2^52"
    )]
    let cpu_ms = cpu_ms as f64;
    let percent = cpu_ms / (window.as_secs_f64() * 1000.0) * 100.0;
    (percent * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(pid: u32) -> ProcessIdentity {
        ProcessIdentity {
            pid,
            started_at_secs: Some(1_000),
        }
    }

    #[test]
    fn first_observation_warms_up_and_second_is_sampled() {
        let mut sampler = CpuSampler::default();
        let start = Instant::now();
        let first = sampler.sample(&[(identity(1), Some(500))], start);
        assert_eq!(first[&identity(1)], CpuUsage::WarmingUp);

        // 1500 ms of CPU in 3 s: half a core.
        let second = sampler.sample(
            &[(identity(1), Some(2_000))],
            start + Duration::from_secs(3),
        );
        assert_eq!(second[&identity(1)], CpuUsage::Sampled { percent: 50.0 });
    }

    #[test]
    fn idle_process_reads_zero_only_after_two_observations() {
        let mut sampler = CpuSampler::default();
        let start = Instant::now();
        sampler.sample(&[(identity(1), Some(40))], start);
        let idle = sampler.sample(&[(identity(1), Some(40))], start + Duration::from_secs(3));
        assert_eq!(idle[&identity(1)], CpuUsage::Sampled { percent: 0.0 });
    }

    #[test]
    fn short_window_repeats_the_last_figure_and_keeps_the_baseline() {
        let mut sampler = CpuSampler::default();
        let start = Instant::now();
        sampler.sample(&[(identity(1), Some(0))], start);
        let early = sampler.sample(
            &[(identity(1), Some(10))],
            start + Duration::from_millis(50),
        );
        assert_eq!(early[&identity(1)], CpuUsage::WarmingUp);

        let first_window = sampler.sample(
            &[(identity(1), Some(1_000))],
            start + Duration::from_secs(1),
        );
        assert_eq!(
            first_window[&identity(1)],
            CpuUsage::Sampled { percent: 100.0 }
        );
        let early = sampler.sample(
            &[(identity(1), Some(1_001))],
            start + Duration::from_millis(1_100),
        );
        assert_eq!(early[&identity(1)], CpuUsage::Sampled { percent: 100.0 });
        // The baseline is still the one from 1 s: 1000 ms of CPU over 1 s.
        let later = sampler.sample(
            &[(identity(1), Some(2_000))],
            start + Duration::from_secs(2),
        );
        assert_eq!(later[&identity(1)], CpuUsage::Sampled { percent: 100.0 });
    }

    #[test]
    fn recycled_pid_with_a_new_start_time_warms_up_again() {
        let mut sampler = CpuSampler::default();
        let start = Instant::now();
        sampler.sample(&[(identity(1), Some(5_000))], start);
        let reused = ProcessIdentity {
            pid: 1,
            started_at_secs: Some(2_000),
        };
        let usage = sampler.sample(&[(reused, Some(10))], start + Duration::from_secs(3));
        assert_eq!(usage[&reused], CpuUsage::WarmingUp);
    }

    #[test]
    fn withheld_cpu_time_or_start_time_is_unavailable_not_zero() {
        let mut sampler = CpuSampler::default();
        let no_start = ProcessIdentity {
            pid: 2,
            started_at_secs: None,
        };
        let usage = sampler.sample(&[(identity(1), None), (no_start, Some(10))], Instant::now());
        assert_eq!(usage[&identity(1)], CpuUsage::Unavailable);
        assert_eq!(usage[&no_start], CpuUsage::Unavailable);
    }

    #[test]
    fn processes_missing_from_a_scan_are_forgotten() {
        let mut sampler = CpuSampler::default();
        let start = Instant::now();
        sampler.sample(&[(identity(1), Some(0)), (identity(2), Some(0))], start);
        sampler.sample(&[(identity(2), Some(0))], start + Duration::from_secs(3));
        assert_eq!(sampler.baselines.len(), 1);
        let back = sampler.sample(&[(identity(1), Some(0))], start + Duration::from_secs(6));
        assert_eq!(back[&identity(1)], CpuUsage::WarmingUp);
    }
}

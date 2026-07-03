use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use crate::history::RepositoryMetric;
use borg_core::progress::ProgressEvent;

#[derive(Debug, Default, Clone, Copy)]
pub struct MetricTotals {
    pub original_size: u64,
    pub compressed_size: u64,
    pub deduplicated_size: u64,
}

impl MetricTotals {
    pub fn observe(&mut self, event: &ProgressEvent) {
        if let ProgressEvent::ArchiveProgress {
            original_size,
            compressed_size,
            deduplicated_size,
            ..
        } = event
        {
            self.original_size = original_size.unwrap_or(self.original_size);
            self.compressed_size = compressed_size.unwrap_or(self.compressed_size);
            self.deduplicated_size = deduplicated_size.unwrap_or(self.deduplicated_size);
        }
    }

    pub fn into_metric(
        self,
        profile_id: String,
        destination: String,
        duration_seconds: u64,
    ) -> RepositoryMetric {
        RepositoryMetric {
            timestamp: Utc::now().to_rfc3339(),
            profile_id,
            destination,
            original_size: self.original_size,
            compressed_size: self.compressed_size,
            deduplicated_size: self.deduplicated_size,
            stored_size: None,
            duration_seconds,
            transfer_rate: if duration_seconds == 0 {
                self.deduplicated_size as f64
            } else {
                self.deduplicated_size as f64 / duration_seconds as f64
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StorageForecast {
    pub observations: usize,
    pub deduplication_savings_percent: Option<f64>,
    pub recent_growth_bytes: Option<i64>,
    pub throughput_bytes_per_second: Option<f64>,
    pub estimated_next_duration_seconds: Option<u64>,
    pub projected_capacity_date: Option<String>,
    pub local_free_space: Option<u64>,
    pub remote_quota: Option<u64>,
    pub status: String,
}

pub fn calculate(
    metrics: &[RepositoryMetric],
    free_space: Option<u64>,
    remote_quota: Option<u64>,
) -> StorageForecast {
    let latest = metrics.last();
    let savings = latest.and_then(|metric| {
        (metric.original_size > 0)
            .then(|| 100.0 * (1.0 - metric.deduplicated_size as f64 / metric.original_size as f64))
    });
    let throughput = latest.map(|metric| metric.transfer_rate);
    let estimated_duration = latest.and_then(|metric| {
        (metric.transfer_rate > 0.0)
            .then(|| (metric.deduplicated_size as f64 / metric.transfer_rate).ceil() as u64)
    });

    if metrics.len() < 7 {
        return StorageForecast {
            observations: metrics.len(),
            deduplication_savings_percent: savings,
            recent_growth_bytes: None,
            throughput_bytes_per_second: throughput,
            estimated_next_duration_seconds: estimated_duration,
            projected_capacity_date: None,
            local_free_space: free_space,
            remote_quota,
            status: "insufficient_history".into(),
        };
    }

    let mut growth: Vec<i64> = metrics
        .windows(2)
        .map(|pair| pair[1].deduplicated_size as i64 - pair[0].deduplicated_size as i64)
        .collect();
    growth.sort_unstable();
    let median_growth = growth[growth.len() / 2];
    let capacity = free_space.or(remote_quota);
    let projected = capacity.filter(|_| median_growth > 0).and_then(|bytes| {
        let intervals = bytes / median_growth as u64;
        let latest_time = latest
            .and_then(|metric| DateTime::parse_from_rfc3339(&metric.timestamp).ok())?
            .with_timezone(&Utc);
        Some((latest_time + Duration::days(intervals.min(36_500) as i64)).to_rfc3339())
    });
    StorageForecast {
        observations: metrics.len(),
        deduplication_savings_percent: savings,
        recent_growth_bytes: Some(median_growth),
        throughput_bytes_per_second: throughput,
        estimated_next_duration_seconds: estimated_duration,
        projected_capacity_date: projected,
        local_free_space: free_space,
        remote_quota,
        status: "ready".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metric(index: u64, size: u64) -> RepositoryMetric {
        RepositoryMetric {
            timestamp: format!("2026-01-{:02}T00:00:00Z", index + 1),
            profile_id: "p".into(),
            destination: "primary".into(),
            original_size: size * 2,
            compressed_size: size,
            deduplicated_size: size,
            stored_size: None,
            duration_seconds: 10,
            transfer_rate: 100.0,
        }
    }

    #[test]
    fn requires_seven_observations() {
        let metrics = (0..6).map(|i| metric(i, 100 + i)).collect::<Vec<_>>();
        assert_eq!(
            calculate(&metrics, Some(1000), None).status,
            "insufficient_history"
        );
    }

    #[test]
    fn median_growth_ignores_single_outlier() {
        let sizes = [100, 110, 120, 1000, 1010, 1020, 1030];
        let metrics = sizes
            .into_iter()
            .enumerate()
            .map(|(i, size)| metric(i as u64, size))
            .collect::<Vec<_>>();
        assert_eq!(
            calculate(&metrics, Some(1000), None).recent_growth_bytes,
            Some(10)
        );
    }
}

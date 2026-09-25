use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Instant;

use anyhow::Context;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct LogRow {
    status_code: i32,
}

pub struct NaiveScanResult {
    pub rows_scanned: u64,
    pub rows_matched: u64,
    pub elapsed_ms: f64,
}

pub fn scan_count_all(jsonl_path: &Path) -> anyhow::Result<NaiveScanResult> {
    scan_with_filter(jsonl_path, |_| true)
}

pub fn scan_count_errors(jsonl_path: &Path) -> anyhow::Result<NaiveScanResult> {
    scan_with_filter(jsonl_path, |row| row.status_code >= 500)
}

fn scan_with_filter(
    jsonl_path: &Path,
    mut predicate: impl FnMut(&LogRow) -> bool,
) -> anyhow::Result<NaiveScanResult> {
    let file = File::open(jsonl_path)
        .with_context(|| format!("open jsonl {}", jsonl_path.display()))?;
    let reader = BufReader::new(file);

    let start = Instant::now();
    let mut rows_scanned = 0u64;
    let mut rows_matched = 0u64;

    for line in reader.lines() {
        let line = line.context("read jsonl line")?;
        if line.trim().is_empty() {
            continue;
        }
        let row: LogRow = serde_json::from_str(&line).context("parse json line")?;
        rows_scanned += 1;
        if predicate(&row) {
            rows_matched += 1;
        }
    }

    Ok(NaiveScanResult {
        rows_scanned,
        rows_matched,
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
    })
}

pub fn median_ms(samples: Vec<f64>) -> f64 {
    percentile_ms(samples, 50.0)
}

pub fn percentile_ms(mut samples: Vec<f64>, p: f64) -> f64 {
    assert!(!samples.is_empty(), "percentile requires at least one sample");
    samples.sort_by(f64::total_cmp);
    let rank = (p / 100.0) * (samples.len() as f64 - 1.0);
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    if lo == hi {
        samples[lo]
    } else {
        let weight = rank - lo as f64;
        samples[lo] * (1.0 - weight) + samples[hi] * weight
    }
}

#[derive(Debug, Clone)]
pub struct LatencyStats {
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
}

pub fn latency_stats(samples: Vec<f64>) -> LatencyStats {
    LatencyStats {
        p50_ms: percentile_ms(samples.clone(), 50.0),
        p95_ms: percentile_ms(samples.clone(), 95.0),
        p99_ms: percentile_ms(samples, 99.0),
    }
}

pub fn run_median(
    runs: usize,
    mut f: impl FnMut() -> anyhow::Result<NaiveScanResult>,
) -> anyhow::Result<(NaiveScanResult, f64)> {
    let mut times = Vec::with_capacity(runs);
    let mut last = None;

    for _ in 0..runs {
        let result = f()?;
        times.push(result.elapsed_ms);
        last = Some(result);
    }

    Ok((last.expect("at least one run"), median_ms(times)))
}

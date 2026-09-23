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

pub fn median_ms(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    let mid = samples.len() / 2;
    if samples.len() % 2 == 0 {
        (samples[mid - 1] + samples[mid]) / 2.0
    } else {
        samples[mid]
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

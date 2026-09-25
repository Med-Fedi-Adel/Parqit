use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use futures::future::join_all;
use workloads::{
    build_workloads, concurrent_workload_ids, filter_workloads, load_manifest,
};

use crate::datafusion_bench::MinioConfig;
use crate::naive::{self, LatencyStats};
use crate::v3_bench::{build_context, run_query_once};

#[derive(Debug, Clone)]
pub struct TimedQuery {
    pub workload: String,
    pub latency_ms: f64,
}

#[derive(Debug, Clone)]
pub struct ConcurrentRun {
    pub label: String,
    pub minio_path: String,
    pub workers: usize,
    pub rounds: usize,
    pub overall: LatencyStats,
    pub by_workload: Vec<(String, LatencyStats, usize)>,
}

pub async fn run_concurrent(
    config: &MinioConfig,
    minio_path: &str,
    layout_label: &str,
    manifest_path: &Path,
    queries_dir: &Path,
    workers: usize,
    rounds: usize,
) -> anyhow::Result<ConcurrentRun> {
    let manifest = load_manifest(manifest_path)?;
    let all_workloads = build_workloads(&manifest, queries_dir)?;
    let mix = filter_workloads(&all_workloads, concurrent_workload_ids());
    if mix.is_empty() {
        anyhow::bail!("no workloads matched concurrent mix");
    }

    println!(
        "v3 concurrent on {layout_label} (s3://{}/{minio_path}/, {} workers × {rounds} rounds):\n",
        config.bucket, workers
    );

    let ctx = Arc::new(build_context(config, minio_path).await?);
    let mut timed = Vec::with_capacity(workers * rounds);

    for round in 0..rounds {
        let mut tasks = Vec::with_capacity(workers);
        for worker in 0..workers {
            let workload = &mix[(round * workers + worker) % mix.len()];
            let ctx = Arc::clone(&ctx);
            let sql = workload.sql.clone();
            let name = workload.name.clone();
            tasks.push(tokio::spawn(async move {
                let latency_ms = run_query_once(&ctx, &sql).await?;
                Ok::<TimedQuery, anyhow::Error>(TimedQuery {
                    workload: name,
                    latency_ms,
                })
            }));
        }

        let results = join_all(tasks).await;
        for result in results {
            let timed_query = result.context("join concurrent task")??;
            println!(
                "  round {}: {} {:.1} ms",
                round + 1,
                timed_query.workload,
                timed_query.latency_ms
            );
            timed.push(timed_query);
        }
    }

    let overall = naive::latency_stats(timed.iter().map(|t| t.latency_ms).collect());

    let mut grouped: HashMap<String, Vec<f64>> = HashMap::new();
    for sample in &timed {
        grouped
            .entry(sample.workload.clone())
            .or_default()
            .push(sample.latency_ms);
    }

    let mut by_workload: Vec<(String, LatencyStats, usize)> = grouped
        .into_iter()
        .map(|(name, samples)| {
            let count = samples.len();
            (name, naive::latency_stats(samples), count)
        })
        .collect();
    by_workload.sort_by(|a, b| a.0.cmp(&b.0));

    println!(
        "\n  Overall: p50={:.1} ms  p95={:.1} ms  p99={:.1} ms",
        overall.p50_ms, overall.p95_ms, overall.p99_ms
    );

    Ok(ConcurrentRun {
        label: layout_label.to_string(),
        minio_path: minio_path.to_string(),
        workers,
        rounds,
        overall,
        by_workload,
    })
}

pub fn format_step4_markdown(raw: &ConcurrentRun, compacted: Option<&ConcurrentRun>) -> String {
    let mut md = String::from("\n## Step 4 — Concurrent load\n\n");
    md.push_str(&format_concurrent_section(raw));
    if let Some(comp) = compacted {
        md.push_str(&format_concurrent_section(comp));
        md.push_str("\n### Raw vs compacted (overall p50)\n\n");
        md.push_str("| Layout | p50 (ms) | p95 (ms) | p99 (ms) |\n");
        md.push_str("|--------|----------:|---------:|---------:|\n");
        md.push_str(&format!(
            "| Raw | {:.1} | {:.1} | {:.1} |\n",
            raw.overall.p50_ms, raw.overall.p95_ms, raw.overall.p99_ms
        ));
        md.push_str(&format!(
            "| Compacted | {:.1} | {:.1} | {:.1} |\n",
            comp.overall.p50_ms, comp.overall.p95_ms, comp.overall.p99_ms
        ));
        if comp.overall.p50_ms > 0.0 {
            md.push_str(&format!(
                "\n_Compacted p50 is {:.2}× raw p50 under concurrent load._\n",
                raw.overall.p50_ms / comp.overall.p50_ms
            ));
        }
    }
    md
}

fn format_concurrent_section(run: &ConcurrentRun) -> String {
    let mut section = format!(
        "### {} (`{}`, {} workers × {} rounds = {} queries)\n\n",
        run.label,
        run.minio_path,
        run.workers,
        run.rounds,
        run.workers * run.rounds
    );
    section.push_str("| Metric | p50 (ms) | p95 (ms) | p99 (ms) |\n");
    section.push_str("|--------|----------:|---------:|---------:|\n");
    section.push_str(&format!(
        "| **Overall** | **{:.1}** | **{:.1}** | **{:.1}** |\n",
        run.overall.p50_ms, run.overall.p95_ms, run.overall.p99_ms
    ));
    section.push_str("\n| Workload | p50 (ms) | p95 (ms) | p99 (ms) | Samples |\n");
    section.push_str("|----------|----------:|---------:|---------:|--------:|\n");
    for (name, stats, count) in &run.by_workload {
        section.push_str(&format!(
            "| {} | {:.1} | {:.1} | {:.1} | {} |\n",
            name, stats.p50_ms, stats.p95_ms, stats.p99_ms, count
        ));
    }
    section.push('\n');
    section
}

pub fn append_step4_to_file(path: &Path, step4_md: &str) -> anyhow::Result<()> {
    let existing = std::fs::read_to_string(path).unwrap_or_else(|_| {
        "# v3 Workload Benchmarks\n\n_Run step3-all first._\n".to_string()
    });
    let combined = if existing.contains("## Step 4") {
        existing
            .split("## Step 4")
            .next()
            .unwrap_or(&existing)
            .to_string()
            + step4_md
    } else {
        existing + step4_md
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, combined)?;
    Ok(())
}

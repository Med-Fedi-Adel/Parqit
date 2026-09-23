mod compression;
mod datafusion_bench;
mod export;
mod naive;

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "benchmark",
    about = "Day 3 benchmarks — naive baseline and compression"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Export Layout B Parquet to JSON Lines (same rows, raw text baseline)
    ExportJsonl {
        #[arg(long, default_value = "data/layout_b/part-000.parquet")]
        input: PathBuf,

        #[arg(long, default_value = "data/logs.jsonl")]
        output: PathBuf,
    },

    /// Print JSON vs Parquet size comparison
    Compression {
        #[arg(long, default_value = "data/logs.jsonl")]
        jsonl: PathBuf,

        #[arg(long, default_value = "data/layout_b/part-000.parquet")]
        flat: PathBuf,

        #[arg(long, default_value = "data/layout_a")]
        hive: PathBuf,
    },

    /// Naive JSONL full scan + selective filter (median of N runs)
    Naive {
        #[arg(long, default_value = "data/logs.jsonl")]
        file: PathBuf,

        #[arg(long, default_value_t = 3)]
        runs: usize,
    },

    /// Step 3.1 all-in-one: export (if needed), compression table, naive benchmarks
    Step1 {
        #[arg(long, default_value = "data/layout_b/part-000.parquet")]
        parquet: PathBuf,

        #[arg(long, default_value = "data/logs.jsonl")]
        jsonl: PathBuf,

        #[arg(long, default_value = "data/layout_a")]
        hive: PathBuf,

        #[arg(long, default_value_t = 3)]
        runs: usize,

        /// Skip JSONL export if file already exists
        #[arg(long, default_value_t = false)]
        skip_export: bool,
    },

    /// Step 3.2: DataFusion benchmarks against MinIO
    Step2 {
        #[arg(long, default_value = "http://127.0.0.1:9000")]
        endpoint: String,

        #[arg(long, default_value = "minioadmin")]
        access_key: String,

        #[arg(long, default_value = "minioadmin")]
        secret_key: String,

        #[arg(long, default_value = "logs")]
        bucket: String,

        #[arg(long, default_value_t = 3)]
        runs: usize,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::ExportJsonl { input, output } => cmd_export(&input, &output),
        Command::Compression { jsonl, flat, hive } => cmd_compression(&jsonl, &flat, &hive),
        Command::Naive { file, runs } => cmd_naive(&file, runs),
        Command::Step1 {
            parquet,
            jsonl,
            hive,
            runs,
            skip_export,
        } => cmd_step1(&parquet, &jsonl, &hive, runs, skip_export),
        Command::Step2 {
            endpoint,
            access_key,
            secret_key,
            bucket,
            runs,
        } => cmd_step2(endpoint, access_key, secret_key, bucket, runs).await,
    }
}

fn cmd_export(input: &PathBuf, output: &PathBuf) -> anyhow::Result<()> {
    println!("Exporting {} → {}", input.display(), output.display());
    let rows = export::parquet_to_jsonl(input, output)?;
    let bytes = std::fs::metadata(output)?.len();
    println!(
        "Exported {rows} rows ({})",
        compression::format_bytes(bytes)
    );
    Ok(())
}

fn cmd_compression(jsonl: &PathBuf, flat: &PathBuf, hive: &PathBuf) -> anyhow::Result<()> {
    let report = compression::SizeReport::collect(jsonl, flat, hive)?;
    print_compression_table(&report);
    Ok(())
}

fn cmd_naive(file: &PathBuf, runs: usize) -> anyhow::Result<()> {
    if !file.exists() {
        anyhow::bail!("JSONL not found at {} — run export first", file.display());
    }

    let (full, full_ms) = naive::run_median(runs, || naive::scan_count_all(file))?;
    let (errors, errors_ms) = naive::run_median(runs, || naive::scan_count_errors(file))?;
    print_naive_results(runs, full_ms, &full, errors_ms, &errors);
    Ok(())
}

fn print_naive_results(
    runs: usize,
    full_ms: f64,
    full: &naive::NaiveScanResult,
    errors_ms: f64,
    errors: &naive::NaiveScanResult,
) {
    println!("Naive JSONL scan (median of {runs} runs):");
    println!(
        "  Full scan:       {:>12.1} ms  rows={}",
        full_ms, full.rows_scanned
    );
    println!(
        "  Selective (5xx): {:>12.1} ms  matched={} / {}",
        errors_ms, errors.rows_matched, errors.rows_scanned
    );
}

fn cmd_step1(
    parquet: &PathBuf,
    jsonl: &PathBuf,
    hive: &PathBuf,
    runs: usize,
    skip_export: bool,
) -> anyhow::Result<()> {
    if !skip_export || !jsonl.exists() {
        cmd_export(parquet, jsonl)?;
    } else {
        println!("Skipping export — {} exists", jsonl.display());
    }

    println!();
    let report = compression::SizeReport::collect(jsonl, parquet, hive)?;
    print_compression_table(&report);

    println!();
    let (full, full_ms) = naive::run_median(runs, || naive::scan_count_all(jsonl))?;
    let (errors, errors_ms) = naive::run_median(runs, || naive::scan_count_errors(jsonl))?;
    print_naive_results(runs, full_ms, &full, errors_ms, &errors);

    let markdown = format_step1_markdown(&report, runs, full_ms, &full, errors_ms, &errors)?;
    let out_path = PathBuf::from("results/benchmarks.md");
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out_path, &markdown)
        .with_context(|| format!("write {}", out_path.display()))?;
    println!();
    println!("Saved summary → {}", out_path.display());

    Ok(())
}

fn print_compression_table(report: &compression::SizeReport) {
    println!("Compression (same 5M rows):");
    println!(
        "  JSON Lines:     {}",
        compression::format_bytes(report.jsonl_bytes)
    );
    println!(
        "  Parquet flat:   {}",
        compression::format_bytes(report.parquet_flat_bytes)
    );
    println!(
        "  Parquet hive:   {}",
        compression::format_bytes(report.parquet_hive_bytes)
    );
    println!("  Ratio JSON/flat: {:.2}x", report.json_vs_flat_ratio());
}

async fn cmd_step2(
    endpoint: String,
    access_key: String,
    secret_key: String,
    bucket: String,
    runs: usize,
) -> anyhow::Result<()> {
    println!("DataFusion benchmarks via MinIO ({endpoint}, bucket={bucket})");
    println!("Median of {runs} runs:\n");

    let config = datafusion_bench::MinioConfig {
        endpoint,
        access_key,
        secret_key,
        bucket,
    };
    let results = datafusion_bench::run_all(&config, runs).await?;

    let step2_md = datafusion_bench::format_step2_markdown(&results, runs);
    let out_path = PathBuf::from("results/benchmarks.md");
    let existing = std::fs::read_to_string(&out_path).unwrap_or_else(|_| {
        "# Benchmark Results (Day 3)\n\n_Run `make bench-step1` first for compression + naive baseline._\n".to_string()
    });
    let combined = if existing.contains("## Step 3.2") {
        existing
            .split("## Step 3.2")
            .next()
            .unwrap_or(&existing)
            .to_string()
            + &step2_md
    } else {
        existing + &step2_md
    };
    std::fs::write(&out_path, combined).with_context(|| format!("write {}", out_path.display()))?;

    println!();
    println!("Appended results → {}", out_path.display());
    Ok(())
}

fn format_step1_markdown(
    report: &compression::SizeReport,
    runs: usize,
    full_ms: f64,
    full: &naive::NaiveScanResult,
    errors_ms: f64,
    errors: &naive::NaiveScanResult,
) -> anyhow::Result<String> {
    Ok(format!(
        r#"# Benchmark Results (Day 3)

## Step 3.1 — Compression + naive baseline

| Format | Size |
|--------|------|
| JSON Lines | {jsonl_size} |
| Parquet (flat) | {flat_size} |
| Parquet (hive) | {hive_size} |
| **JSON / flat ratio** | **{ratio:.2}x** |

## Naive JSONL scan (median of {runs} runs)

| Scenario | Latency (ms) | Rows |
|----------|-------------|------|
| Full scan | {full_ms:.1} | {full_rows} |
| Selective (`status_code >= 500`) | {errors_ms:.1} | {errors_matched} matched |

_Note: naive scan reads and parses every line even when filtering — no pushdown._
"#,
        jsonl_size = compression::format_bytes(report.jsonl_bytes),
        flat_size = compression::format_bytes(report.parquet_flat_bytes),
        hive_size = compression::format_bytes(report.parquet_hive_bytes),
        ratio = report.json_vs_flat_ratio(),
        full_ms = full_ms,
        full_rows = full.rows_scanned,
        errors_ms = errors_ms,
        errors_matched = errors.rows_matched,
    ))
}

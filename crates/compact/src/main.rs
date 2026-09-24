mod merge;

use std::path::PathBuf;

use clap::Parser;
use merge::{print_report, run, write_report_json, CompactConfig};

#[derive(Parser)]
#[command(name = "compact", about = "Merge raw micro-batch Parquet files into compacted partitions")]
struct Args {
    /// Raw ingest layout root (date=…/hour=…/service=…/)
    #[arg(long, default_value = "data/raw")]
    input: PathBuf,

    /// Compacted output root
    #[arg(long, default_value = "data/compacted")]
    output: PathBuf,

    /// Target max output file size in megabytes
    #[arg(long, default_value_t = 128)]
    target_size_mb: u64,

    /// Parquet row group size for output files
    #[arg(long, default_value_t = 100_000)]
    row_group_size: usize,

    /// Write JSON report to this path
    #[arg(long, default_value = "results/compaction.json")]
    report: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = CompactConfig {
        input: args.input,
        output: args.output,
        target_size_bytes: args.target_size_mb * 1_048_576,
        row_group_size: args.row_group_size,
    };

    let report = run(&config)?;
    print_report(&report);

    if let Some(parent) = args.report.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_report_json(&report, &args.report)?;
    println!("  Report: {}", args.report.display());

    Ok(())
}

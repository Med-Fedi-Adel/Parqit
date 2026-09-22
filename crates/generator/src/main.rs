mod generate;
mod schema;
mod write;

use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;

#[derive(Parser)]
#[command(name = "generator", about = "Generate synthetic observability logs")]
struct Args {
    /// Total number of log rows to generate
    #[arg(long, default_value_t = 5_000_000)]
    rows: usize,

    /// Output root directory
    #[arg(long, default_value = "data")]
    output: PathBuf,

    /// Parquet row group size
    #[arg(long, default_value_t = 100_000)]
    row_group_size: usize,

    /// RNG seed for reproducible datasets
    #[arg(long, default_value_t = 42)]
    seed: u64,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let layout_a = args.output.join("layout_a");
    let layout_b = args.output.join("layout_b");

    println!("Generating {} rows...", args.rows);
    println!("  Layout A (Hive): {}/date=…/hour=…/service=…/", layout_a.display());
    println!("  Layout B (flat): {}/", layout_b.display());

    let hive_stats =
        generate::generate_hive_layout(&layout_a, args.rows, args.row_group_size, args.seed)
            .context("failed to write hive layout")?;
    let flat_stats = generate::generate_flat_layout(
        &layout_b,
        args.rows,
        args.row_group_size,
        args.seed + 1,
    )
    .context("failed to write flat layout")?;

    println!();
    println!("Done.");
    println!(
        "  Layout A: {} partitions, {:.2} MB",
        hive_stats["hive_partitions"],
        hive_stats["hive_parquet_bytes"] as f64 / 1_048_576.0
    );
    println!(
        "  Layout B: {:.2} MB",
        flat_stats["flat_parquet_bytes"] as f64 / 1_048_576.0
    );

    Ok(())
}

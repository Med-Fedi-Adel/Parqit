mod config;
mod generate;
mod raw_layout;
mod schema;
mod write;

use anyhow::Context;
use clap::Parser;
use config::{Args, ResolvedConfig};

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = ResolvedConfig::from_args(args)?;

    if config.is_legacy() {
        run_legacy(&config)?;
    } else {
        raw_layout::generate_raw_layout(&config)?;
    }

    Ok(())
}

fn run_legacy(config: &ResolvedConfig) -> anyhow::Result<()> {
    let layout_a = config.output.join("layout_a");
    let layout_b = config.output.join("layout_b");

    println!("Generating {} rows (legacy v2)...", config.rows);
    println!("  Layout A (Hive): {}/date=…/hour=…/service=…/", layout_a.display());
    println!("  Layout B (flat): {}/", layout_b.display());

    let hive_stats = generate::generate_hive_layout(
        &layout_a,
        config.rows,
        config.row_group_size,
        config.seed,
    )
    .context("failed to write hive layout")?;
    let flat_stats = generate::generate_flat_layout(
        &layout_b,
        config.rows,
        config.row_group_size,
        config.seed + 1,
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

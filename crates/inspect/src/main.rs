use std::fs::File;
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Parser;
use parquet::file::metadata::ParquetMetaDataReader;
use parquet::file::statistics::Statistics;

#[derive(Parser)]
#[command(name = "inspect", about = "Print Parquet footer metadata and column statistics")]
struct Args {
    /// Parquet file or directory to inspect
    path: PathBuf,

    /// Maximum number of files to inspect when given a directory
    #[arg(long, default_value_t = 5)]
    limit: usize,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let files = collect_parquet_files(&args.path, args.limit)?;

    if files.is_empty() {
        anyhow::bail!("no parquet files found at {}", args.path.display());
    }

    for file in files {
        inspect_file(&file)?;
        println!();
    }

    Ok(())
}

fn collect_parquet_files(path: &Path, limit: usize) -> anyhow::Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }

    let mut files = parquet_files(path)?;
    files.sort();
    files.truncate(limit);
    Ok(files)
}

fn parquet_files(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    visit(root, &mut files)?;
    Ok(files)
}

fn visit(dir: &Path, files: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            visit(&path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "parquet") {
            files.push(path);
        }
    }
    Ok(())
}

fn inspect_file(path: &Path) -> anyhow::Result<()> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let metadata = ParquetMetaDataReader::new()
        .parse_and_finish(&file)
        .with_context(|| format!("read metadata from {}", path.display()))?;

    let file_size = std::fs::metadata(path)?.len();
    let compressed_data: i64 = metadata
        .row_groups()
        .iter()
        .map(|rg| rg.compressed_size())
        .sum();

    println!("File: {}", path.display());
    println!(
        "  Rows: {}, Row groups: {}, File size: {:.2} MB, Column data: {:.2} MB",
        metadata.file_metadata().num_rows(),
        metadata.row_groups().len(),
        file_size as f64 / 1_048_576.0,
        compressed_data as f64 / 1_048_576.0
    );

    for (idx, row_group) in metadata.row_groups().iter().enumerate() {
        println!("  Row group {idx}: {} rows", row_group.num_rows());
        for column in row_group.columns() {
            let column_name = column.column_path().string();
            let encoding = format!("{:?}", column.encodings());
            match column.statistics() {
                Some(stats) if has_min_max(stats) => {
                    let min = format_stat_value(stats, &column_name, true);
                    let max = format_stat_value(stats, &column_name, false);
                    println!(
                        "    {column_name} ({encoding}): min={min} max={max} compressed={} bytes",
                        column.compressed_size()
                    );
                }
                Some(_) => println!("    {column_name} ({encoding}): statistics present but no min/max"),
                None => println!("    {column_name} ({encoding}): no statistics"),
            }
        }
    }

    Ok(())
}

fn has_min_max(stats: &Statistics) -> bool {
    stats.min_bytes_opt().is_some() && stats.max_bytes_opt().is_some()
}

fn format_stat_value(stats: &Statistics, column_name: &str, is_min: bool) -> String {
    match stats {
        Statistics::Int32(s) => {
            let value = if is_min { s.min_opt() } else { s.max_opt() };
            value.map(|v| v.to_string()).unwrap_or_else(|| "?".into())
        }
        Statistics::Int64(s) => {
            let value = if is_min { s.min_opt() } else { s.max_opt() };
            match value {
                Some(v) if column_name == "timestamp" => format_timestamp_micros(*v),
                Some(v) => v.to_string(),
                None => "?".into(),
            }
        }
        Statistics::ByteArray(s) => {
            let value = if is_min { s.min_opt() } else { s.max_opt() };
            value
                .map(|v| String::from_utf8_lossy(v.data()).into_owned())
                .unwrap_or_else(|| "?".into())
        }
        _ => {
            let bytes = if is_min {
                stats.min_bytes_opt()
            } else {
                stats.max_bytes_opt()
            };
            bytes
                .map(|b| format!("0x{}", hex::encode(b)))
                .unwrap_or_else(|| "?".into())
        }
    }
}

fn format_timestamp_micros(micros: i64) -> String {
    chrono::DateTime::from_timestamp_micros(micros)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string())
        .unwrap_or_else(|| micros.to_string())
}

mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

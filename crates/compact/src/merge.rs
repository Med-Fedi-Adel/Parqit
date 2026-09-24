use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context;
use arrow::datatypes::SchemaRef;
use arrow::record_batch::RecordBatch;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use rayon::prelude::*;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CompactReport {
    pub input_dir: String,
    pub output_dir: String,
    pub target_size_mb: u64,
    pub partitions: u64,
    pub files_in: u64,
    pub files_out: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub rows_in: u64,
    pub rows_out: u64,
    pub duration_secs: f64,
}

#[derive(Default)]
struct PartitionResult {
    files_in: u64,
    files_out: u64,
    bytes_in: u64,
    bytes_out: u64,
    rows_in: u64,
    rows_out: u64,
}

pub struct CompactConfig {
    pub input: PathBuf,
    pub output: PathBuf,
    pub target_size_bytes: u64,
    pub row_group_size: usize,
}

pub fn run(config: &CompactConfig) -> anyhow::Result<CompactReport> {
    let started = Instant::now();
    std::fs::create_dir_all(&config.output)?;

    let partitions = find_partitions(&config.input)?;
    if partitions.is_empty() {
        anyhow::bail!("no partitions with parquet files under {}", config.input.display());
    }

    println!(
        "Compacting {} partitions (target {} MB per output file)...",
        partitions.len(),
        config.target_size_bytes / 1_048_576
    );

    let results: Vec<PartitionResult> = partitions
        .par_iter()
        .map(|partition| {
            compact_partition(partition, &config.input, &config.output, config)
                .unwrap_or_else(|err| {
                    panic!(
                        "failed to compact partition {}: {err:#}",
                        partition.display()
                    )
                })
        })
        .collect();

    let mut totals = PartitionResult::default();
    for result in results {
        totals.files_in += result.files_in;
        totals.files_out += result.files_out;
        totals.bytes_in += result.bytes_in;
        totals.bytes_out += result.bytes_out;
        totals.rows_in += result.rows_in;
        totals.rows_out += result.rows_out;
    }

    if totals.rows_in != totals.rows_out {
        anyhow::bail!(
            "row count mismatch: read {} rows, wrote {}",
            totals.rows_in,
            totals.rows_out
        );
    }

    let report = CompactReport {
        input_dir: config.input.display().to_string(),
        output_dir: config.output.display().to_string(),
        target_size_mb: config.target_size_bytes / 1_048_576,
        partitions: partitions.len() as u64,
        files_in: totals.files_in,
        files_out: totals.files_out,
        bytes_in: totals.bytes_in,
        bytes_out: totals.bytes_out,
        rows_in: totals.rows_in,
        rows_out: totals.rows_out,
        duration_secs: started.elapsed().as_secs_f64(),
    };

    Ok(report)
}

fn find_partitions(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut partitions = Vec::new();
    collect_partitions(root, &mut partitions)?;
    partitions.sort();
    Ok(partitions)
}

fn collect_partitions(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        entries.push(entry?);
    }

    let has_parquet = entries.iter().any(|entry| {
        entry.path().is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|ext| ext == "parquet")
    });

    if has_parquet {
        out.push(dir.to_path_buf());
        return Ok(());
    }

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_partitions(&path, out)?;
        }
    }

    Ok(())
}

fn compact_partition(
    partition: &Path,
    input_root: &Path,
    output_root: &Path,
    config: &CompactConfig,
) -> anyhow::Result<PartitionResult> {
    let rel = partition
        .strip_prefix(input_root)
        .context("partition outside input root")?;
    let out_dir = output_root.join(rel);
    std::fs::create_dir_all(&out_dir)?;

    let mut input_files: Vec<PathBuf> = std::fs::read_dir(partition)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext == "parquet")
        })
        .collect();
    input_files.sort();

    let mut merger = PartitionMerger::new(out_dir, config.target_size_bytes, config.row_group_size);
    let mut result = PartitionResult {
        files_in: input_files.len() as u64,
        ..PartitionResult::default()
    };

    for input_path in input_files {
        result.bytes_in += std::fs::metadata(&input_path)?.len();
        let file = File::open(&input_path)?;
        let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;
        for batch in reader {
            let batch = batch?;
            result.rows_in += batch.num_rows() as u64;
            merger.write_batch(&batch)?;
            result.rows_out = merger.rows_written();
        }
    }

    merger.finish()?;
    result.files_out = merger.files_written();
    result.bytes_out = merger.bytes_written();

    Ok(result)
}

struct PartitionMerger {
    out_dir: PathBuf,
    target_size_bytes: u64,
    row_group_size: usize,
    part_index: u32,
    writer: Option<ArrowWriter<File>>,
    current_path: PathBuf,
    schema: Option<SchemaRef>,
    rows_written: u64,
    bytes_written: u64,
    files_written: u64,
}

impl PartitionMerger {
    fn new(out_dir: PathBuf, target_size_bytes: u64, row_group_size: usize) -> Self {
        Self {
            out_dir,
            target_size_bytes,
            row_group_size,
            part_index: 0,
            writer: None,
            current_path: PathBuf::new(),
            schema: None,
            rows_written: 0,
            bytes_written: 0,
            files_written: 0,
        }
    }

    fn rows_written(&self) -> u64 {
        self.rows_written
    }

    fn files_written(&self) -> u64 {
        self.files_written
    }

    fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    fn write_batch(&mut self, batch: &RecordBatch) -> anyhow::Result<()> {
        if self.writer.is_none() {
            self.start_new_file(batch.schema())?;
        }
        let writer = self.writer.as_mut().expect("writer open");
        writer.write(batch)?;
        self.rows_written += batch.num_rows() as u64;

        let size = std::fs::metadata(&self.current_path)?.len();
        if size >= self.target_size_bytes {
            self.finish_current()?;
        }
        Ok(())
    }

    fn finish(&mut self) -> anyhow::Result<()> {
        if self.writer.is_some() {
            self.finish_current()?;
        }
        Ok(())
    }

    fn start_new_file(&mut self, schema: SchemaRef) -> anyhow::Result<()> {
        self.schema = Some(schema.clone());
        self.current_path = self
            .out_dir
            .join(format!("part-{:03}.parquet", self.part_index));
        self.part_index += 1;

        let file = File::create(&self.current_path)?;
        let props = WriterProperties::builder()
            .set_compression(Compression::SNAPPY)
            .set_max_row_group_size(self.row_group_size)
            .set_statistics_enabled(parquet::file::properties::EnabledStatistics::Chunk)
            .build();
        self.writer = Some(ArrowWriter::try_new(file, schema, Some(props))?);
        Ok(())
    }

    fn finish_current(&mut self) -> anyhow::Result<()> {
        if let Some(writer) = self.writer.take() {
            writer.close()?;
            let size = std::fs::metadata(&self.current_path)?.len();
            self.bytes_written += size;
            self.files_written += 1;
        }
        Ok(())
    }
}

pub fn print_report(report: &CompactReport) {
    println!();
    println!("Compaction complete.");
    println!("  Partitions: {}", report.partitions);
    println!(
        "  Files: {} → {} ({:.1}% reduction)",
        report.files_in,
        report.files_out,
        (1.0 - report.files_out as f64 / report.files_in as f64) * 100.0
    );
    println!(
        "  Size: {:.2} GB → {:.2} GB",
        report.bytes_in as f64 / 1_073_741_824.0,
        report.bytes_out as f64 / 1_073_741_824.0
    );
    println!("  Rows: {}", report.rows_in);
    println!("  Duration: {:.1}s", report.duration_secs);
}

pub fn write_report_json(report: &CompactReport, path: &Path) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(path, json)?;
    Ok(())
}

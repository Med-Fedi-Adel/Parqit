use std::path::Path;

use anyhow::Context;

pub struct SizeReport {
    pub jsonl_bytes: u64,
    pub parquet_flat_bytes: u64,
    pub parquet_hive_bytes: u64,
}

impl SizeReport {
    pub fn collect(jsonl: &Path, flat_parquet: &Path, hive_dir: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            jsonl_bytes: file_size(jsonl)?,
            parquet_flat_bytes: file_size(flat_parquet)?,
            parquet_hive_bytes: dir_parquet_bytes(hive_dir)?,
        })
    }

    pub fn json_vs_flat_ratio(&self) -> f64 {
        self.jsonl_bytes as f64 / self.parquet_flat_bytes as f64
    }
}

fn file_size(path: &Path) -> anyhow::Result<u64> {
    std::fs::metadata(path)
        .with_context(|| format!("stat {}", path.display()))
        .map(|m| m.len())
}

fn dir_parquet_bytes(root: &Path) -> anyhow::Result<u64> {
    let mut total = 0u64;
    if !root.exists() {
        return Ok(0);
    }
    visit_parquet(root, &mut total)?;
    Ok(total)
}

fn visit_parquet(dir: &Path, total: &mut u64) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            visit_parquet(&path, total)?;
        } else if path.extension().is_some_and(|e| e == "parquet") {
            *total += file_size(&path)?;
        }
    }
    Ok(())
}

pub fn format_bytes(bytes: u64) -> String {
    const MB: f64 = 1_048_576.0;
    if bytes >= 1_048_576 {
        format!("{:.2} MB", bytes as f64 / MB)
    } else if bytes >= 1024 {
        format!("{:.2} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

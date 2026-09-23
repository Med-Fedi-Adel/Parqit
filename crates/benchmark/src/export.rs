use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::Context;
use arrow::array::{
    Array, Int32Array, StringArray, TimestampMicrosecondArray,
};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

pub fn parquet_to_jsonl(parquet_path: &Path, jsonl_path: &Path) -> anyhow::Result<u64> {
    let file = File::open(parquet_path)
        .with_context(|| format!("open parquet {}", parquet_path.display()))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?
        .build()
        .context("build parquet batch reader")?;

    if let Some(parent) = jsonl_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let out = File::create(jsonl_path)
        .with_context(|| format!("create jsonl {}", jsonl_path.display()))?;
    let mut writer = BufWriter::new(out);
    let mut rows = 0u64;

    for batch in reader {
        let batch = batch.context("read parquet batch")?;
        rows += export_batch(&mut writer, &batch)? as u64;
    }

    writer.flush()?;
    Ok(rows)
}

fn export_batch(
    writer: &mut impl Write,
    batch: &arrow::record_batch::RecordBatch,
) -> anyhow::Result<usize> {
    let n = batch.num_rows();
    let ts = batch
        .column_by_name("timestamp")
        .context("timestamp column")?
        .as_any()
        .downcast_ref::<TimestampMicrosecondArray>()
        .context("timestamp type")?;
    let service = batch
        .column_by_name("service")
        .context("service column")?
        .as_any()
        .downcast_ref::<StringArray>()
        .context("service type")?;
    let status = batch
        .column_by_name("status_code")
        .context("status_code column")?
        .as_any()
        .downcast_ref::<Int32Array>()
        .context("status_code type")?;
    let latency = batch
        .column_by_name("latency_ms")
        .context("latency_ms column")?
        .as_any()
        .downcast_ref::<Int32Array>()
        .context("latency_ms type")?;
    let trace_id = batch
        .column_by_name("trace_id")
        .context("trace_id column")?
        .as_any()
        .downcast_ref::<StringArray>()
        .context("trace_id type")?;
    let method = batch
        .column_by_name("http_method")
        .context("http_method column")?
        .as_any()
        .downcast_ref::<StringArray>()
        .context("http_method type")?;
    let route = batch
        .column_by_name("route")
        .context("route column")?
        .as_any()
        .downcast_ref::<StringArray>()
        .context("route type")?;
    let region = batch
        .column_by_name("region")
        .context("region column")?
        .as_any()
        .downcast_ref::<StringArray>()
        .context("region type")?;

    for i in 0..n {
        let line = serde_json::json!({
            "timestamp": ts.value(i),
            "service": service.value(i),
            "status_code": status.value(i),
            "latency_ms": latency.value(i),
            "trace_id": trace_id.value(i),
            "http_method": method.value(i),
            "route": route.value(i),
            "region": region.value(i),
        });
        writeln!(writer, "{line}")?;
    }

    Ok(n)
}

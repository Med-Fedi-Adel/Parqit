use std::fs::File;

use arrow::datatypes::SchemaRef;
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

fn writer_properties(row_group_size: usize) -> WriterProperties {
    WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .set_max_row_group_size(row_group_size)
        .set_statistics_enabled(parquet::file::properties::EnabledStatistics::Chunk)
        .build()
}

pub fn write_parquet(
    path: &std::path::Path,
    batch: &RecordBatch,
    row_group_size: usize,
) -> anyhow::Result<u64> {
    write_parquet_batches(path, batch.schema(), std::slice::from_ref(batch), row_group_size)
}

pub fn write_parquet_batches(
    path: &std::path::Path,
    schema: SchemaRef,
    batches: &[RecordBatch],
    row_group_size: usize,
) -> anyhow::Result<u64> {
    let file = File::create(path)?;
    let mut writer = ArrowWriter::try_new(file, schema, Some(writer_properties(row_group_size)))?;
    for batch in batches {
        writer.write(batch)?;
    }
    writer.close()?;

    Ok(std::fs::metadata(path)?.len())
}

pub fn open_parquet_writer(
    path: &std::path::Path,
    schema: SchemaRef,
    row_group_size: usize,
) -> anyhow::Result<ArrowWriter<File>> {
    let file = File::create(path)?;
    ArrowWriter::try_new(file, schema, Some(writer_properties(row_group_size)))
        .map_err(anyhow::Error::from)
}

pub fn finish_parquet_writer(writer: ArrowWriter<File>, path: &std::path::Path) -> anyhow::Result<u64> {
    writer.close()?;
    Ok(std::fs::metadata(path)?.len())
}

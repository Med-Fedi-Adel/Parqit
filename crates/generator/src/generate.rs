use std::collections::HashMap;

use arrow::array::{
    Int32Array, RecordBatch, StringArray, TimestampMicrosecondArray, TimestampMicrosecondBuilder,
};
use chrono::NaiveDate;
use rand::prelude::*;
use uuid::Uuid;

use crate::schema::{HTTP_METHODS, REGIONS, ROUTES, SERVICES};

pub struct PartitionKey {
    pub date: String,
    pub hour: u32,
    pub service: String,
}

pub fn partition_keys() -> Vec<PartitionKey> {
    let date = "2026-09-01".to_string();
    let mut keys = Vec::with_capacity(SERVICES.len() * 24);
    for hour in 0..24 {
        for service in SERVICES {
            keys.push(PartitionKey {
                date: date.clone(),
                hour,
                service: (*service).to_string(),
            });
        }
    }
    keys
}

pub fn rows_per_partition(total_rows: usize, partition_count: usize) -> Vec<usize> {
    let base = total_rows / partition_count;
    let remainder = total_rows % partition_count;
    (0..partition_count)
        .map(|idx| base + usize::from(idx < remainder))
        .collect()
}

pub fn generate_hive_batch(key: &PartitionKey, row_count: usize, rng: &mut StdRng) -> RecordBatch {
    let schema = crate::schema::hive_file_schema();
    let base_ts = NaiveDate::parse_from_str(&key.date, "%Y-%m-%d")
        .expect("valid date")
        .and_hms_opt(key.hour, 0, 0)
        .expect("valid hour")
        .and_utc()
        .timestamp_micros();

    let hour_end_ts = base_ts + 3_600_000_000 - 1;

    let mut timestamps = TimestampMicrosecondBuilder::with_capacity(row_count);
    let mut status_codes = Vec::with_capacity(row_count);
    let mut latencies = Vec::with_capacity(row_count);
    let mut trace_ids = Vec::with_capacity(row_count);
    let mut http_methods = Vec::with_capacity(row_count);
    let mut routes = Vec::with_capacity(row_count);
    let mut regions = Vec::with_capacity(row_count);

    for _ in 0..row_count {
        timestamps.append_value(rng.gen_range(base_ts..=hour_end_ts));
        let status = sample_status_code(rng);
        status_codes.push(status);
        latencies.push(sample_latency_ms(rng, status));
        trace_ids.push(Uuid::new_v4().to_string());
        http_methods.push(HTTP_METHODS.choose(rng).unwrap().to_string());
        routes.push(ROUTES.choose(rng).unwrap().to_string());
        regions.push(REGIONS.choose(rng).unwrap().to_string());
    }

    RecordBatch::try_new(
        schema.into(),
        vec![
            std::sync::Arc::new(TimestampMicrosecondArray::from(timestamps.finish())),
            std::sync::Arc::new(Int32Array::from(status_codes)),
            std::sync::Arc::new(Int32Array::from(latencies)),
            std::sync::Arc::new(StringArray::from(trace_ids)),
            std::sync::Arc::new(StringArray::from(http_methods)),
            std::sync::Arc::new(StringArray::from(routes)),
            std::sync::Arc::new(StringArray::from(regions)),
        ],
    )
    .expect("valid hive batch")
}

pub fn generate_flat_batch(
    rows: &[(String, i64, i32, i32, String, String, String, String)],
) -> RecordBatch {
    let schema = crate::schema::flat_file_schema();
    let row_count = rows.len();

    let mut services = Vec::with_capacity(row_count);
    let mut timestamps = TimestampMicrosecondBuilder::with_capacity(row_count);
    let mut status_codes = Vec::with_capacity(row_count);
    let mut latencies = Vec::with_capacity(row_count);
    let mut trace_ids = Vec::with_capacity(row_count);
    let mut http_methods = Vec::with_capacity(row_count);
    let mut routes = Vec::with_capacity(row_count);
    let mut regions = Vec::with_capacity(row_count);

    for (service, ts, status, latency, trace_id, method, route, region) in rows {
        services.push(service.clone());
        timestamps.append_value(*ts);
        status_codes.push(*status);
        latencies.push(*latency);
        trace_ids.push(trace_id.clone());
        http_methods.push(method.clone());
        routes.push(route.clone());
        regions.push(region.clone());
    }

    RecordBatch::try_new(
        schema.into(),
        vec![
            std::sync::Arc::new(TimestampMicrosecondArray::from(timestamps.finish())),
            std::sync::Arc::new(StringArray::from(services)),
            std::sync::Arc::new(Int32Array::from(status_codes)),
            std::sync::Arc::new(Int32Array::from(latencies)),
            std::sync::Arc::new(StringArray::from(trace_ids)),
            std::sync::Arc::new(StringArray::from(http_methods)),
            std::sync::Arc::new(StringArray::from(routes)),
            std::sync::Arc::new(StringArray::from(regions)),
        ],
    )
    .expect("valid flat batch")
}

pub fn generate_flat_rows(
    total_rows: usize,
    rng: &mut StdRng,
) -> Vec<(String, i64, i32, i32, String, String, String, String)> {
    let base_date = NaiveDate::from_ymd_opt(2026, 9, 1).expect("valid date");
    let day_start = base_date.and_hms_opt(0, 0, 0).expect("valid time");
    let day_start_ts = day_start.and_utc().timestamp_micros();
    let day_end_ts = day_start_ts + 86_400_000_000 - 1;

    (0..total_rows)
        .map(|_| {
            let ts = rng.gen_range(day_start_ts..=day_end_ts);
            let service = SERVICES.choose(rng).unwrap().to_string();
            let status = sample_status_code(rng);
            (
                service,
                ts,
                status,
                sample_latency_ms(rng, status),
                Uuid::new_v4().to_string(),
                HTTP_METHODS.choose(rng).unwrap().to_string(),
                ROUTES.choose(rng).unwrap().to_string(),
                REGIONS.choose(rng).unwrap().to_string(),
            )
        })
        .collect()
}

pub fn generate_hive_layout(
    output_dir: &std::path::Path,
    total_rows: usize,
    row_group_size: usize,
    seed: u64,
) -> anyhow::Result<HashMap<String, u64>> {
    let mut rng = StdRng::seed_from_u64(seed);
    let keys = partition_keys(); // 120
    let counts = rows_per_partition(total_rows, keys.len());
    let mut stats = HashMap::new();

    for (key, row_count) in keys.iter().zip(counts) {
        let dir = output_dir
            .join(format!("date={}", key.date))
            .join(format!("hour={:02}", key.hour))
            .join(format!("service={}", key.service));
        std::fs::create_dir_all(&dir)?;

        let batch = generate_hive_batch(key, row_count, &mut rng);
        let path = dir.join("part-000.parquet");
        let bytes = crate::write::write_parquet(&path, &batch, row_group_size)?;
        *stats.entry("hive_parquet_bytes".to_string()).or_insert(0) += bytes;
    }

    stats.insert("hive_partitions".to_string(), keys.len() as u64);
    stats.insert("hive_rows".to_string(), total_rows as u64);
    Ok(stats)
}

pub fn generate_flat_layout(
    output_dir: &std::path::Path,
    total_rows: usize,
    row_group_size: usize,
    seed: u64,
) -> anyhow::Result<HashMap<String, u64>> {
    use std::sync::Arc;

    std::fs::create_dir_all(output_dir)?;
    let mut rng = StdRng::seed_from_u64(seed);
    let schema = Arc::new(crate::schema::flat_file_schema());

    // One file keeps the benchmark focused on row-group pruning, not file listing.
    // Write in row-group-sized chunks to avoid holding 5M rows in memory at once.
    let path = output_dir.join("part-000.parquet");
    let mut writer = crate::write::open_parquet_writer(&path, schema, row_group_size)?;
    let mut written = 0usize;
    while written < total_rows {
        let chunk = row_group_size.min(total_rows - written);
        let rows = generate_flat_rows(chunk, &mut rng);
        let batch = generate_flat_batch(&rows);
        writer.write(&batch)?;
        written += chunk;
    }
    let bytes = crate::write::finish_parquet_writer(writer, &path)?;

    let mut stats = HashMap::new();
    stats.insert("flat_parquet_bytes".to_string(), bytes);
    stats.insert("flat_rows".to_string(), total_rows as u64);
    Ok(stats)
}

fn sample_status_code(rng: &mut StdRng) -> i32 {
    match rng.gen_range(0..100) {
        0..=89 => rng.gen_range(200..300),
        90..=94 => rng.gen_range(400..500),
        _ => rng.gen_range(500..600),
    }
}

fn sample_latency_ms(rng: &mut StdRng, status_code: i32) -> i32 {
    if status_code >= 500 {
        rng.gen_range(500..3_000)
    } else if status_code >= 400 {
        rng.gen_range(100..800)
    } else {
        rng.gen_range(1..400)
    }
}

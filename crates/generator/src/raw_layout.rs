use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use arrow::array::{
    Array, Int32Array, RecordBatch, StringArray, TimestampMicrosecondArray,
    TimestampMicrosecondBuilder,
};
use chrono::{Duration, NaiveDate};
use rand::prelude::*;
use rayon::prelude::*;
use serde::Serialize;
use uuid::Uuid;

use crate::config::{IncidentConfig, ResolvedConfig};
use crate::schema::{self, ENVIRONMENTS, HTTP_METHODS, REGIONS, ROUTES};

#[derive(Clone)]
pub struct PartitionKey {
    pub day_index: u32,
    pub date: String,
    pub hour: u32,
    pub service: String,
    pub weight: f64,
}

#[derive(Clone, Serialize)]
pub struct TraceSample {
    pub date: String,
    pub trace_id: String,
    pub service: String,
}

#[derive(Serialize)]
pub struct Manifest {
    pub tier: String,
    pub rows: u64,
    pub days: u32,
    pub services: usize,
    pub ingest_interval_min: u32,
    pub service_skew: f64,
    pub files: u64,
    pub bytes: u64,
    pub partitions: u64,
    pub incident_rows: u64,
    pub trace_ids: Vec<TraceSample>,
    pub incident: Option<String>,
}

#[derive(Default)]
struct PartitionStats {
    files: u64,
    bytes: u64,
    rows: u64,
    incident_rows: u64,
}

pub fn generate_raw_layout(config: &ResolvedConfig) -> anyhow::Result<Manifest> {
    let output_dir = config.output.join("raw");
    std::fs::create_dir_all(&output_dir)?;

    let services = schema::services_for_count(config.service_count);
    let keys = partition_keys(config, &services);
    let row_counts = allocate_rows(config.rows, &keys);
    let files_per_hour = files_per_hour(config.ingest_interval_min);

    let trace_samples: Arc<Mutex<HashMap<String, TraceSample>>> = Arc::new(Mutex::new(HashMap::new()));
    let incident = config.incident.clone();

    println!(
        "v3 raw layout: {} rows, {} days, {} services, {} partitions, {} files/partition/hour",
        config.rows,
        config.days,
        services.len(),
        keys.len(),
        files_per_hour
    );

    let stats: Vec<PartitionStats> = keys
        .par_iter()
        .zip(row_counts.par_iter())
        .map(|(key, &row_count)| {
            let mut rng = StdRng::seed_from_u64(
                config.seed
                    ^ key.date.as_bytes().iter().fold(0u64, |acc, b| acc.wrapping_add(*b as u64))
                    ^ (key.hour as u64).wrapping_mul(997)
                    ^ key.service.as_bytes().iter().fold(0u64, |acc, b| acc.wrapping_add(*b as u64)),
            );
            write_partition(
                &output_dir,
                key,
                row_count,
                files_per_hour,
                config.row_group_size,
                config.ingest_interval_min,
                incident.as_ref(),
                &trace_samples,
                &mut rng,
            )
            .unwrap_or_else(|err| {
                panic!(
                    "failed partition {} hour={} service={}: {err}",
                    key.date, key.hour, key.service
                )
            })
        })
        .collect();

    let mut totals = PartitionStats::default();
    for stat in stats {
        totals.files += stat.files;
        totals.bytes += stat.bytes;
        totals.rows += stat.rows;
        totals.incident_rows += stat.incident_rows;
    }

    let mut trace_ids: Vec<TraceSample> = trace_samples
        .lock()
        .expect("trace sample map")
        .values()
        .cloned()
        .collect();
    trace_ids.sort_by(|a, b| a.date.cmp(&b.date));

    let manifest = Manifest {
        tier: match config.tier {
            crate::config::Tier::Legacy => "legacy".to_string(),
            crate::config::Tier::Smoke => "smoke".to_string(),
            crate::config::Tier::Standard => "standard".to_string(),
            crate::config::Tier::Stress => "stress".to_string(),
        },
        rows: totals.rows,
        days: config.days,
        services: services.len(),
        ingest_interval_min: config.ingest_interval_min,
        service_skew: config.service_skew,
        files: totals.files,
        bytes: totals.bytes,
        partitions: keys.len() as u64,
        incident_rows: totals.incident_rows,
        trace_ids,
        incident: config.incident.as_ref().map(|i| {
            format!(
                "day={},hour={}-{},service={},error-rate={}",
                i.day,
                i.hours.start(),
                i.hours.end(),
                i.service,
                i.error_rate
            )
        }),
    };

    let manifest_path = config.output.join("manifest.json");
    let json = serde_json::to_string_pretty(&manifest)?;
    std::fs::write(&manifest_path, json)?;

    println!();
    println!("Done (raw).");
    println!("  Partitions: {}", keys.len());
    println!("  Files: {}", totals.files);
    println!("  Rows: {}", totals.rows);
    println!("  Size: {:.2} GB", totals.bytes as f64 / 1_073_741_824.0);
    if totals.incident_rows > 0 {
        println!("  Incident rows: {}", totals.incident_rows);
    }
    println!("  Manifest: {}", manifest_path.display());

    Ok(manifest)
}

fn files_per_hour(ingest_interval_min: u32) -> u32 {
    if ingest_interval_min == 0 {
        1
    } else {
        60 / ingest_interval_min.max(1)
    }
}

pub fn partition_keys(config: &ResolvedConfig, services: &[String]) -> Vec<PartitionKey> {
    let start =
        NaiveDate::parse_from_str(&config.start_date, "%Y-%m-%d").expect("valid start_date");
    let weights = service_weights(services, config.service_skew);

    let mut keys = Vec::with_capacity(config.days as usize * 24 * services.len());
    for day_idx in 0..config.days {
        let date = start + Duration::days(day_idx as i64);
        let date_str = date.format("%Y-%m-%d").to_string();
        for hour in 0..24 {
            for (service, &weight) in services.iter().zip(weights.iter()) {
                keys.push(PartitionKey {
                    day_index: day_idx + 1,
                    date: date_str.clone(),
                    hour,
                    service: service.clone(),
                    weight,
                });
            }
        }
    }
    keys
}

fn service_weights(services: &[String], skew: f64) -> Vec<f64> {
    if skew <= 0.0 {
        return vec![1.0; services.len()];
    }
    services
        .iter()
        .enumerate()
        .map(|(idx, _)| 1.0 / ((idx + 1) as f64).powf(skew))
        .collect()
}

pub fn allocate_rows(total_rows: usize, keys: &[PartitionKey]) -> Vec<usize> {
    let total_weight: f64 = keys.iter().map(|k| k.weight).sum();
    let mut counts = vec![0usize; keys.len()];
    let mut assigned = 0usize;

    for (idx, key) in keys.iter().enumerate() {
        let share = (total_rows as f64 * key.weight / total_weight).floor() as usize;
        counts[idx] = share;
        assigned += share;
    }

    let mut remainder = total_rows.saturating_sub(assigned);
    let len = counts.len();
    let mut idx = 0;
    while remainder > 0 {
        counts[idx % len] += 1;
        remainder -= 1;
        idx += 1;
    }
    counts
}

fn write_partition(
    output_dir: &std::path::Path,
    key: &PartitionKey,
    row_count: usize,
    files_per_hour: u32,
    row_group_size: usize,
    ingest_interval_min: u32,
    incident: Option<&IncidentConfig>,
    trace_samples: &Arc<Mutex<HashMap<String, TraceSample>>>,
    rng: &mut StdRng,
) -> anyhow::Result<PartitionStats> {
    if row_count == 0 {
        return Ok(PartitionStats::default());
    }

    let dir = output_dir
        .join(format!("date={}", key.date))
        .join(format!("hour={:02}", key.hour))
        .join(format!("service={}", key.service));
    std::fs::create_dir_all(&dir)?;

    let in_incident = incident
        .map(|cfg| cfg.matches(key.day_index, key.hour, &key.service))
        .unwrap_or(false);
    let error_rate = incident.map(|cfg| cfg.error_rate).unwrap_or(0.05);

    let batch_rows = distribute_across_files(row_count, files_per_hour);
    let mut stats = PartitionStats::default();

    for (batch_idx, rows_in_file) in batch_rows.iter().enumerate() {
        if *rows_in_file == 0 {
            continue;
        }
        let (ts_start, ts_end) = batch_time_bounds(key, batch_idx as u32, ingest_interval_min);
        let batch = generate_batch(
            *rows_in_file,
            ts_start,
            ts_end,
            &key.service,
            in_incident,
            error_rate,
            rng,
        );

        if batch.num_rows() > 0 {
            if let Some(col) = batch.column_by_name("trace_id") {
                if let Some(arr) = col.as_any().downcast_ref::<StringArray>() {
                    let mut samples = trace_samples.lock().expect("trace samples");
                    samples.entry(key.date.clone()).or_insert_with(|| TraceSample {
                        date: key.date.clone(),
                        trace_id: arr.value(0).to_string(),
                        service: key.service.clone(),
                    });
                }
            }
        }

        let path = dir.join(format!("part-batch-{batch_idx:04}.parquet"));
        let bytes = crate::write::write_parquet(&path, &batch, row_group_size)?;
        stats.files += 1;
        stats.bytes += bytes;
        stats.rows += *rows_in_file as u64;
        if in_incident {
            stats.incident_rows += *rows_in_file as u64;
        }
    }

    Ok(stats)
}

fn distribute_across_files(row_count: usize, files_per_hour: u32) -> Vec<usize> {
    let n = files_per_hour as usize;
    let base = row_count / n;
    let rem = row_count % n;
    (0..n)
        .map(|idx| base + usize::from(idx < rem))
        .collect()
}

fn batch_time_bounds(
    key: &PartitionKey,
    batch_idx: u32,
    ingest_interval_min: u32,
) -> (i64, i64) {
    let base_date = NaiveDate::parse_from_str(&key.date, "%Y-%m-%d").expect("valid date");
    let hour_start = base_date
        .and_hms_opt(key.hour, 0, 0)
        .expect("valid hour")
        .and_utc()
        .timestamp_micros();

    if ingest_interval_min == 0 {
        return (hour_start, hour_start + 3_600_000_000 - 1);
    }

    let interval_micros = i64::from(ingest_interval_min) * 60 * 1_000_000;
    let start = hour_start + i64::from(batch_idx) * interval_micros;
    let end = start + interval_micros - 1;
    (start, end)
}

fn generate_batch(
    row_count: usize,
    ts_start: i64,
    ts_end: i64,
    service: &str,
    in_incident: bool,
    incident_error_rate: f64,
    rng: &mut StdRng,
) -> RecordBatch {
    let schema = schema::hive_v3_file_schema();
    let pod_count = 200u32;

    let mut timestamps = TimestampMicrosecondBuilder::with_capacity(row_count);
    let mut status_codes = Vec::with_capacity(row_count);
    let mut latencies = Vec::with_capacity(row_count);
    let mut trace_ids = Vec::with_capacity(row_count);
    let mut http_methods = Vec::with_capacity(row_count);
    let mut routes = Vec::with_capacity(row_count);
    let mut regions = Vec::with_capacity(row_count);
    let mut pods = Vec::with_capacity(row_count);
    let mut environments = Vec::with_capacity(row_count);

    for _ in 0..row_count {
        timestamps.append_value(rng.gen_range(ts_start..=ts_end));
        let status = sample_status_code(rng, in_incident, incident_error_rate);
        status_codes.push(status);
        latencies.push(sample_latency_ms(rng, status));
        trace_ids.push(Uuid::new_v4().to_string());
        http_methods.push(HTTP_METHODS.choose(rng).unwrap().to_string());
        routes.push(ROUTES.choose(rng).unwrap().to_string());
        regions.push(REGIONS.choose(rng).unwrap().to_string());
        pods.push(schema::pod_name(
            service,
            rng.gen_range(0..pod_count),
        ));
        environments.push(sample_environment(rng));
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
            std::sync::Arc::new(StringArray::from(pods)),
            std::sync::Arc::new(StringArray::from(environments)),
        ],
    )
    .expect("valid v3 batch")
}

fn sample_environment(rng: &mut StdRng) -> String {
    if rng.gen_bool(0.99) {
        ENVIRONMENTS[0].to_string()
    } else {
        ENVIRONMENTS[1].to_string()
    }
}

fn sample_status_code(rng: &mut StdRng, in_incident: bool, incident_error_rate: f64) -> i32 {
    if in_incident {
        if rng.gen_bool(incident_error_rate) {
            return rng.gen_range(500..600);
        }
        return rng.gen_range(200..300);
    }
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

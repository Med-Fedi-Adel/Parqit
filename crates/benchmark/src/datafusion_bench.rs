use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;
use arrow::datatypes::DataType;
use datafusion::datasource::file_format::parquet::ParquetFormat;
use datafusion::datasource::listing::{
    ListingOptions, ListingTable, ListingTableConfig, ListingTableUrl,
};
use datafusion::prelude::*;
use object_store::aws::AmazonS3Builder;
use url::Url;

use crate::naive;

pub struct MinioConfig {
    pub endpoint: String,
    pub access_key: String,
    pub secret_key: String,
    pub bucket: String,
}

pub struct BenchResult {
    pub name: String,
    pub median_ms: f64,
}

pub fn benchmark_cases() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "DataFusion full scan (Hive)",
            "SELECT COUNT(*) AS n FROM logs",
        ),
        (
            "DataFusion full scan (flat)",
            "SELECT COUNT(*) AS n FROM logs_flat",
        ),
        (
            "DataFusion selective 5xx (Hive)",
            "SELECT COUNT(*) AS n FROM logs WHERE status_code >= 500",
        ),
        (
            "DataFusion selective 5xx (flat)",
            "SELECT COUNT(*) AS n FROM logs_flat WHERE status_code >= 500",
        ),
        (
            "DataFusion tight partition (Hive)",
            "SELECT COUNT(*) AS n FROM logs WHERE date = '2026-09-01' AND hour = '14' AND service = 'payments' AND status_code >= 500",
        ),
        (
            "DataFusion payments 5xx all day (flat)",
            "SELECT COUNT(*) AS n FROM logs_flat WHERE service = 'payments' AND status_code >= 500",
        ),
        (
            "DataFusion projection narrow (Hive)",
            "SELECT AVG(latency_ms) AS avg_ms FROM logs WHERE status_code >= 500",
        ),
        (
            "DataFusion projection wide (Hive)",
            "SELECT AVG(latency_ms) AS avg_ms, MAX(trace_id) AS max_trace, MAX(route) AS max_route FROM logs WHERE status_code >= 500",
        ),
    ]
}

pub async fn run_all(
    config: &MinioConfig,
    runs: usize,
) -> anyhow::Result<Vec<BenchResult>> {
    let ctx = build_context(config).await?;
    let mut results = Vec::new();

    for (name, sql) in benchmark_cases() {
        let median = run_query_median(&ctx, sql, runs).await?;
        println!("  {name}: {median:.1} ms");
        results.push(BenchResult {
            name: name.to_string(),
            median_ms: median,
        });
    }

    Ok(results)
}

async fn build_context(config: &MinioConfig) -> anyhow::Result<SessionContext> {
    let ctx = SessionContext::new();

    let store = AmazonS3Builder::new()
        .with_endpoint(&config.endpoint)
        .with_bucket_name(&config.bucket)
        .with_access_key_id(&config.access_key)
        .with_secret_access_key(&config.secret_key)
        .with_region("us-east-1")
        .with_allow_http(true)
        .with_virtual_hosted_style_request(false)
        .build()
        .context("build MinIO object store")?;

    let base_url = Url::parse(&format!("s3://{}/", config.bucket))?;
    ctx.runtime_env()
        .register_object_store(&base_url, Arc::new(store));

    register_hive_table(&ctx, &config.bucket, "layout_a", "logs").await?;
    register_flat_table(&ctx, &config.bucket, "layout_b", "logs_flat").await?;

    Ok(ctx)
}

async fn register_hive_table(
    ctx: &SessionContext,
    bucket: &str,
    path: &str,
    table_name: &str,
) -> anyhow::Result<()> {
    register_table(ctx, bucket, path, true, table_name).await
}

async fn register_flat_table(
    ctx: &SessionContext,
    bucket: &str,
    path: &str,
    table_name: &str,
) -> anyhow::Result<()> {
    register_table(ctx, bucket, path, false, table_name).await
}

async fn register_table(
    ctx: &SessionContext,
    bucket: &str,
    path: &str,
    hive: bool,
    table_name: &str,
) -> anyhow::Result<()> {
    let table_url = ListingTableUrl::parse(format!("s3://{bucket}/{path}/"))?;
    let mut listing_options =
        ListingOptions::new(Arc::new(ParquetFormat::default())).with_file_extension(".parquet");

    if hive {
        listing_options = listing_options.with_table_partition_cols(vec![
            ("date".into(), DataType::Utf8),
            ("hour".into(), DataType::Utf8),
            ("service".into(), DataType::Utf8),
        ]);
    }

    let config = ListingTableConfig::new(table_url)
        .with_listing_options(listing_options)
        .infer_schema(&ctx.state())
        .await?;

    let table = ListingTable::try_new(config)?;
    ctx.register_table(table_name, Arc::new(table))?;
    Ok(())
}

async fn run_query_median(ctx: &SessionContext, sql: &str, runs: usize) -> anyhow::Result<f64> {
    let mut times = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        let df = ctx.sql(sql).await.context("execute SQL")?;
        let batches = df.collect().await.context("collect results")?;
        let _ = batches.len();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    Ok(naive::median_ms(times))
}

pub fn format_step2_markdown(results: &[BenchResult], runs: usize) -> String {
    let mut table = String::from(format!(
        "\n## Step 3.2 — DataFusion via MinIO (median of {runs} runs)\n\n\
         | Scenario | Latency (ms) |\n\
         |----------|-------------:|\n"
    ));
    for r in results {
        table.push_str(&format!("| {} | {:.1} |\n", r.name, r.median_ms));
    }
    table.push_str(
        "\n_Source: Parquet in MinIO (`s3://logs/`). Compare to Step 3.1 naive JSONL baseline._\n",
    );
    table
}

use std::path::Path;
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
use workloads::{build_workloads, load_manifest};

use crate::datafusion_bench::{BenchResult, MinioConfig};
use crate::naive;

pub struct V3LayoutRun {
    pub label: String,
    pub minio_path: String,
    pub results: Vec<BenchResult>,
}

pub async fn run_layout(
    config: &MinioConfig,
    minio_path: &str,
    layout_label: &str,
    manifest_path: &Path,
    queries_dir: &Path,
    runs: usize,
) -> anyhow::Result<V3LayoutRun> {
    let manifest = load_manifest(manifest_path)?;
    let workloads = build_workloads(&manifest, queries_dir)?;

    println!(
        "v3 workloads on {layout_label} (s3://{}/{minio_path}/, {} rows, median of {runs} runs):\n",
        config.bucket, manifest.rows
    );

    let ctx = build_context(config, minio_path).await?;
    let mut results = Vec::with_capacity(workloads.len());

    for workload in &workloads {
        let median = run_query_median(&ctx, &workload.sql, runs).await?;
        println!("  {}: {:.1} ms", workload.name, median);
        results.push(BenchResult {
            name: workload.name.clone(),
            median_ms: median,
        });
    }

    Ok(V3LayoutRun {
        label: layout_label.to_string(),
        minio_path: minio_path.to_string(),
        results,
    })
}

async fn build_context(config: &MinioConfig, path: &str) -> anyhow::Result<SessionContext> {
    let ctx = SessionContext::new();

    let store = AmazonS3Builder::new()
        .with_endpoint(&config.endpoint)
        .with_access_key_id(&config.access_key)
        .with_secret_access_key(&config.secret_key)
        .with_bucket_name(&config.bucket)
        .with_region("us-east-1")
        .with_allow_http(true)
        .with_virtual_hosted_style_request(false)
        .build()
        .context("build MinIO object store")?;

    let base_url = Url::parse(&format!("s3://{}/", config.bucket))?;
    ctx.runtime_env()
        .register_object_store(&base_url, Arc::new(store));

    let table_url = ListingTableUrl::parse(format!(
        "s3://{}/{}/",
        config.bucket,
        path.trim_end_matches('/')
    ))?;
    let listing_options = ListingOptions::new(Arc::new(ParquetFormat::default()))
        .with_file_extension(".parquet")
        .with_table_partition_cols(vec![
            ("date".into(), DataType::Utf8),
            ("hour".into(), DataType::Utf8),
            ("service".into(), DataType::Utf8),
        ]);

    let table_config = ListingTableConfig::new(table_url)
        .with_listing_options(listing_options)
        .infer_schema(&ctx.state())
        .await
        .context("infer schema for v3 logs table")?;

    let table = ListingTable::try_new(table_config)?;
    ctx.register_table("logs", Arc::new(table))?;

    Ok(ctx)
}

async fn run_query_median(ctx: &SessionContext, sql: &str, runs: usize) -> anyhow::Result<f64> {
    let mut times = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        let df = ctx.sql(sql).await.context("execute SQL")?;
        df.collect().await.context("collect results")?;
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    Ok(naive::median_ms(times))
}

pub fn format_benchmarks_v3(
    manifest_path: &Path,
    runs: usize,
    raw: &V3LayoutRun,
    compacted: Option<&V3LayoutRun>,
) -> String {
    let manifest = load_manifest(manifest_path).ok();
    let header = manifest
        .as_ref()
        .map(|m| {
            format!(
                "_Dataset: {} tier, {} rows, {} days, {} services, {} raw files._",
                m.tier, m.rows, m.days, m.services, m.files
            )
        })
        .unwrap_or_default();

    let mut md = format!(
        "# v3 Workload Benchmarks\n\n{header}\n\n_Median of {runs} runs via MinIO._\n"
    );

    md.push_str(&format_layout_section(raw, runs));

    if let Some(comp) = compacted {
        md.push_str(&format_layout_section(comp, runs));
        md.push_str("\n## Raw vs compacted\n\n");
        md.push_str("| Workload | Raw (ms) | Compacted (ms) | Ratio |\n");
        md.push_str("|----------|----------:|---------------:|------:|\n");
        for (raw_r, comp_r) in raw.results.iter().zip(comp.results.iter()) {
            let ratio = if comp_r.median_ms > 0.0 {
                raw_r.median_ms / comp_r.median_ms
            } else {
                0.0
            };
            md.push_str(&format!(
                "| {} | {:.1} | {:.1} | {:.2}x |\n",
                raw_r.name, raw_r.median_ms, comp_r.median_ms, ratio
            ));
        }
        md.push('\n');
    }

    md
}

fn format_layout_section(run: &V3LayoutRun, runs: usize) -> String {
    let mut section = format!(
        "\n## {} (`s3://…/{}/`)\n\n| Workload | Latency (ms) |\n|----------|-------------:|\n",
        run.label, run.minio_path
    );
    for r in &run.results {
        section.push_str(&format!("| {} | {:.1} |\n", r.name, r.median_ms));
    }
    section.push_str(&format!("\n_Median of {runs} runs._\n"));
    section
}

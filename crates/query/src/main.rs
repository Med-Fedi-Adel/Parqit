use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use arrow::datatypes::DataType;
use clap::{Parser, ValueEnum};
use workloads::{build_workloads, load_manifest};
use datafusion::datasource::file_format::parquet::ParquetFormat;
use datafusion::datasource::listing::{
    ListingOptions, ListingTable, ListingTableConfig, ListingTableUrl,
};
use datafusion::prelude::*;
use object_store::aws::AmazonS3Builder;
use url::Url;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Layout {
    /// Single directory, all columns stored in Parquet files (Layout B)
    Flat,
    /// Hive-style paths: date=…/hour=…/service=…/ (Layout A)
    Hive,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Workload {
    FullScan,
    Dashboard,
    Incident,
    ScopedDay,
    TightPartition,
    Selective5xx,
    TraceLookup,
    CrossDay,
    ProjectionWide,
}

impl Workload {
    fn id(self) -> &'static str {
        match self {
            Workload::FullScan => "full_scan",
            Workload::Dashboard => "dashboard",
            Workload::Incident => "incident",
            Workload::ScopedDay => "scoped_day",
            Workload::TightPartition => "tight_partition",
            Workload::Selective5xx => "selective_5xx",
            Workload::TraceLookup => "trace_lookup",
            Workload::CrossDay => "cross_day",
            Workload::ProjectionWide => "projection_wide",
        }
    }
}

#[derive(Parser)]
#[command(
    name = "query",
    about = "Run SQL against Parquet in MinIO via DataFusion"
)]
struct Args {
    /// MinIO / S3 API endpoint
    #[arg(long, default_value = "http://127.0.0.1:9000")]
    endpoint: String,

    /// S3 access key
    #[arg(long, default_value = "minioadmin")]
    access_key: String,

    /// S3 secret key
    #[arg(long, default_value = "minioadmin")]
    secret_key: String,

    /// Bucket name
    #[arg(long, default_value = "logs")]
    bucket: String,

    /// Object prefix inside the bucket (ignored when --both)
    #[arg(long, default_value = "layout_a")]
    path: String,

    /// Table layout: flat (Layout B) or hive (Layout A)
    #[arg(long, value_enum, default_value_t = Layout::Hive)]
    layout: Layout,

    /// Register both logs (Hive / layout_a) and logs_flat (flat / layout_b)
    #[arg(long, default_value_t = false)]
    both: bool,

    /// SQL table name (ignored when --both for registration; use logs or logs_flat in SQL)
    #[arg(long, default_value = "logs")]
    table: String,

    /// SQL query to execute (ignored when --workload is set)
    #[arg(long, default_value = "SELECT COUNT(*) AS row_count FROM logs")]
    sql: String,

    /// Run a predefined v3 workload (reads queries/ + manifest.json)
    #[arg(long, value_enum)]
    workload: Option<Workload>,

    /// Manifest for workload date/trace substitution
    #[arg(long, default_value = "data/manifest.json")]
    manifest: PathBuf,

    /// Directory containing queries/*.sql templates
    #[arg(long, default_value = "queries")]
    queries_dir: PathBuf,

    /// Print EXPLAIN plan
    #[arg(long, default_value_t = false)]
    explain: bool,

    /// Print EXPLAIN ANALYZE plan (includes runtime metrics)
    #[arg(long, default_value_t = false)]
    explain_analyze: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let ctx = SessionContext::new();
    register_minio_store(&ctx, &args)?;

    if args.both {
        register_table(
            &ctx,
            &args.bucket,
            "layout_a",
            Layout::Hive,
            "logs",
        )
        .await?;
        register_table(
            &ctx,
            &args.bucket,
            "layout_b",
            Layout::Flat,
            "logs_flat",
        )
        .await?;
        println!("Registered tables: logs (Hive layout_a), logs_flat (flat layout_b)");
    } else {
        register_table(
            &ctx,
            &args.bucket,
            &args.path,
            args.layout,
            &args.table,
        )
        .await?;
    }

    let sql = resolve_sql(&args)?;
    println!("Running: {sql}");
    let df = ctx.sql(&sql).await.context("execute SQL")?;
    df.show().await.context("print results")?;

    Ok(())
}

fn resolve_sql(args: &Args) -> anyhow::Result<String> {
    let base_sql = if let Some(workload) = args.workload {
        let manifest = load_manifest(&args.manifest)?;
        let workloads = build_workloads(&manifest, &args.queries_dir)?;
        let id = workload.id();
        workloads
            .into_iter()
            .find(|w| w.id == id)
            .map(|w| w.sql)
            .with_context(|| format!("workload {id} not found"))?
    } else {
        args.sql.clone()
    };

    Ok(if args.explain_analyze {
        format!("EXPLAIN ANALYZE {base_sql}")
    } else if args.explain {
        format!("EXPLAIN {base_sql}")
    } else {
        base_sql
    })
}

fn register_minio_store(ctx: &SessionContext, args: &Args) -> anyhow::Result<()> {
    let store = AmazonS3Builder::new()
        .with_endpoint(&args.endpoint)
        .with_bucket_name(&args.bucket)
        .with_access_key_id(&args.access_key)
        .with_secret_access_key(&args.secret_key)
        .with_region("us-east-1")
        .with_allow_http(true)
        .with_virtual_hosted_style_request(false)
        .build()
        .context("build MinIO object store")?;

    let base_url = Url::parse(&format!("s3://{}/", args.bucket))
        .context("parse s3 base URL for object store registration")?;

    ctx.runtime_env()
        .register_object_store(&base_url, Arc::new(store));

    Ok(())
}

async fn register_table(
    ctx: &SessionContext,
    bucket: &str,
    path: &str,
    layout: Layout,
    table_name: &str,
) -> anyhow::Result<()> {
    let table_url = ListingTableUrl::parse(format!("s3://{bucket}/{}/", path.trim_end_matches('/')))
        .with_context(|| format!("parse listing table URL for {table_name}"))?;

    let mut listing_options =
        ListingOptions::new(Arc::new(ParquetFormat::default())).with_file_extension(".parquet");

    if matches!(layout, Layout::Hive) {
        listing_options = listing_options.with_table_partition_cols(vec![
            ("date".into(), DataType::Utf8),
            ("hour".into(), DataType::Utf8),
            ("service".into(), DataType::Utf8),
        ]);
    }

    let config = ListingTableConfig::new(table_url)
        .with_listing_options(listing_options)
        .infer_schema(&ctx.state())
        .await
        .with_context(|| format!("infer schema for table {table_name}"))?;

    let table = ListingTable::try_new(config)
        .with_context(|| format!("create listing table {table_name}"))?;
    ctx.register_table(table_name, Arc::new(table))
        .with_context(|| format!("register table {table_name}"))?;

    Ok(())
}

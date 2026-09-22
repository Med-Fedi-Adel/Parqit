use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use datafusion::datasource::file_format::parquet::ParquetFormat;
use datafusion::datasource::listing::{
    ListingOptions, ListingTable, ListingTableConfig, ListingTableUrl,
};
use datafusion::prelude::*;
use object_store::aws::AmazonS3Builder;
use url::Url;

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

    /// Object prefix inside the bucket (Layout B by default)
    #[arg(long, default_value = "layout_b")]
    path: String,

    /// SQL table name
    #[arg(long, default_value = "logs")]
    table: String,

    /// SQL query to execute
    #[arg(long, default_value = "SELECT COUNT(*) AS row_count FROM logs")]
    sql: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let ctx = SessionContext::new();
    register_minio_store(&ctx, &args)?;
    register_parquet_table(&ctx, &args).await?;

    println!("Running: {}", args.sql);
    let df = ctx.sql(&args.sql).await.context("execute SQL")?;
    df.show().await.context("print results")?;

    Ok(())
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

async fn register_parquet_table(ctx: &SessionContext, args: &Args) -> anyhow::Result<()> {
    let table_url = ListingTableUrl::parse(format!(
        "s3://{}/{}/",
        args.bucket,
        args.path.trim_end_matches('/')
    ))
    .context("parse listing table URL")?;

    let listing_options =
        ListingOptions::new(Arc::new(ParquetFormat::default())).with_file_extension(".parquet");

    let config = ListingTableConfig::new(table_url)
        .with_listing_options(listing_options)
        .infer_schema(&ctx.state())
        .await
        .context("infer schema from Parquet footer")?;

    let table = ListingTable::try_new(config).context("create listing table")?;
    ctx.register_table(&args.table, Arc::new(table))
        .context("register table")?;

    Ok(())
}

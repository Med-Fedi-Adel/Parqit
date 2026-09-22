use arrow::datatypes::{DataType, Field, Schema, TimeUnit};

pub const SERVICES: &[&str] = &["api", "payments", "auth", "notifications", "search"];
pub const HTTP_METHODS: &[&str] = &["GET", "POST", "PUT", "DELETE", "PATCH"];
pub const REGIONS: &[&str] = &["us-east-1", "us-west-2", "eu-west-1", "ap-southeast-1"];
pub const ROUTES: &[&str] = &[
    "/api/v1/users",
    "/api/v1/users/{id}",
    "/api/v1/orders",
    "/api/v1/orders/{id}",
    "/api/v1/payments",
    "/api/v1/payments/charge",
    "/api/v1/auth/login",
    "/api/v1/auth/logout",
    "/api/v1/auth/refresh",
    "/api/v1/search",
    "/api/v1/search/suggest",
    "/api/v1/notifications",
    "/api/v1/notifications/send",
    "/api/v1/products",
    "/api/v1/products/{id}",
    "/api/v1/cart",
    "/api/v1/cart/checkout",
    "/api/v1/inventory",
    "/api/v1/inventory/reserve",
    "/api/v1/webhooks/stripe",
    "/api/v1/webhooks/github",
    "/api/v1/metrics",
    "/api/v1/health",
    "/api/v1/admin/users",
    "/api/v1/admin/settings",
    "/api/v2/users",
    "/api/v2/orders",
    "/api/v2/payments/refund",
    "/internal/debug",
    "/internal/cache/purge",
];

/// Hive-partitioned files: partition keys live in the directory path.
pub fn hive_file_schema() -> Schema {
    Schema::new(vec![
        Field::new(
            "timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, None),
            false,
        ),
        Field::new("status_code", DataType::Int32, false),
        Field::new("latency_ms", DataType::Int32, false),
        Field::new("trace_id", DataType::Utf8, false),
        Field::new("http_method", DataType::Utf8, false),
        Field::new("route", DataType::Utf8, false),
        Field::new("region", DataType::Utf8, false),
    ])
}

/// Flat layout: every column, including `service`, is stored in the file.
pub fn flat_file_schema() -> Schema {
    let mut fields = hive_file_schema().fields().to_vec();
    fields.insert(1, Field::new("service", DataType::Utf8, false).into());
    Schema::new(fields)
}

use std::path::PathBuf;

use clap::Parser;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Legacy,
    Smoke,
    Standard,
    Stress,
}

impl Tier {
    pub fn rows(self) -> usize {
        match self {
            Tier::Legacy => 5_000_000,
            Tier::Smoke => 10_000_000,
            Tier::Standard => 200_000_000,
            Tier::Stress => 1_000_000_000,
        }
    }

    pub fn days(self) -> u32 {
        match self {
            Tier::Legacy => 1,
            Tier::Smoke => 1,
            Tier::Standard => 7,
            Tier::Stress => 14,
        }
    }

    pub fn service_count(self) -> usize {
        match self {
            Tier::Legacy => 5,
            Tier::Smoke => 5,
            Tier::Standard => 20,
            Tier::Stress => 50,
        }
    }

    pub fn ingest_interval_min(self) -> u32 {
        match self {
            Tier::Legacy => 0,
            _ => 15,
        }
    }

    pub fn service_skew(self) -> f64 {
        match self {
            Tier::Legacy => 0.0,
            Tier::Smoke => 1.0,
            Tier::Standard => 1.2,
            Tier::Stress => 1.4,
        }
    }
}

#[derive(Clone, Debug)]
pub struct IncidentConfig {
    /// 1-based day index within the dataset (day 1 = start date).
    pub day: u32,
    pub hours: std::ops::RangeInclusive<u32>,
    pub service: String,
    pub error_rate: f64,
}

impl IncidentConfig {
    pub fn parse(raw: &str) -> anyhow::Result<Self> {
        let mut day = None;
        let mut hours = None;
        let mut service = None;
        let mut error_rate = None;

        for part in raw.split(',') {
            let (key, value) = part
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("invalid incident segment: {part}"))?;
            match key.trim() {
                "day" => day = Some(value.trim().parse()?),
                "hour" => {
                    hours = Some(parse_hour_range(value.trim())?);
                }
                "service" => service = Some(value.trim().to_string()),
                "error-rate" => error_rate = Some(value.trim().parse()?),
                other => anyhow::bail!("unknown incident key: {other}"),
            }
        }

        Ok(Self {
            day: day.ok_or_else(|| anyhow::anyhow!("incident missing day="))?,
            hours: hours.ok_or_else(|| anyhow::anyhow!("incident missing hour="))?,
            service: service.ok_or_else(|| anyhow::anyhow!("incident missing service="))?,
            error_rate: error_rate.unwrap_or(0.25),
        })
    }

    pub fn matches(&self, day_index: u32, hour: u32, service: &str) -> bool {
        self.day == day_index && self.hours.contains(&hour) && self.service == service
    }
}

fn parse_hour_range(raw: &str) -> anyhow::Result<std::ops::RangeInclusive<u32>> {
    if let Some((start, end)) = raw.split_once('-') {
        let start: u32 = start.parse()?;
        let end: u32 = end.parse()?;
        if start > end || end > 23 {
            anyhow::bail!("invalid hour range: {raw}");
        }
        return Ok(start..=end);
    }
    let hour: u32 = raw.parse()?;
    Ok(hour..=hour)
}

#[derive(Parser)]
#[command(name = "generator", about = "Generate synthetic observability logs")]
pub struct Args {
    /// Preset: legacy (v2), smoke, standard, stress
    #[arg(long, value_enum, default_value_t = CliTier::Legacy)]
    pub tier: CliTier,

    /// Override row count (default comes from tier)
    #[arg(long)]
    pub rows: Option<usize>,

    /// Override retention days (default comes from tier)
    #[arg(long)]
    pub days: Option<u32>,

    /// Output root directory
    #[arg(long, default_value = "data")]
    pub output: PathBuf,

    /// Parquet row group size
    #[arg(long, default_value_t = 100_000)]
    pub row_group_size: usize,

    /// RNG seed for reproducible datasets
    #[arg(long, default_value_t = 42)]
    pub seed: u64,

    /// First date in the dataset (YYYY-MM-DD)
    #[arg(long, default_value = "2026-09-01")]
    pub start_date: String,

    /// Micro-batch flush interval in minutes (0 = one file per partition)
    #[arg(long)]
    pub ingest_interval_min: Option<u32>,

    /// Zipf skew for service popularity (0 = uniform)
    #[arg(long)]
    pub service_skew: Option<f64>,

    /// Incident window, e.g. day=3,hour=14-16,service=payments,error-rate=0.25
    #[arg(long)]
    pub incident: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum CliTier {
    #[default]
    Legacy,
    Smoke,
    Standard,
    Stress,
}

impl From<CliTier> for Tier {
    fn from(value: CliTier) -> Self {
        match value {
            CliTier::Legacy => Tier::Legacy,
            CliTier::Smoke => Tier::Smoke,
            CliTier::Standard => Tier::Standard,
            CliTier::Stress => Tier::Stress,
        }
    }
}

pub struct ResolvedConfig {
    pub tier: Tier,
    pub rows: usize,
    pub days: u32,
    pub output: PathBuf,
    pub row_group_size: usize,
    pub seed: u64,
    pub start_date: String,
    pub ingest_interval_min: u32,
    pub service_skew: f64,
    pub incident: Option<IncidentConfig>,
    pub service_count: usize,
}

impl ResolvedConfig {
    pub fn from_args(args: Args) -> anyhow::Result<Self> {
        let tier: Tier = args.tier.into();
        let incident = match args.incident {
            Some(raw) => Some(IncidentConfig::parse(&raw)?),
            None => None,
        };

        Ok(Self {
            tier,
            rows: args.rows.unwrap_or_else(|| tier.rows()),
            days: args.days.unwrap_or_else(|| tier.days()),
            output: args.output,
            row_group_size: args.row_group_size,
            seed: args.seed,
            start_date: args.start_date,
            ingest_interval_min: args
                .ingest_interval_min
                .unwrap_or_else(|| tier.ingest_interval_min()),
            service_skew: args.service_skew.unwrap_or_else(|| tier.service_skew()),
            incident,
            service_count: tier.service_count(),
        })
    }

    pub fn is_legacy(&self) -> bool {
        self.tier == Tier::Legacy
    }
}

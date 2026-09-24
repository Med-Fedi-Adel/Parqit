use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::{Duration, NaiveDate};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct TraceSample {
    pub date: String,
    pub trace_id: String,
    pub service: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub tier: String,
    pub rows: u64,
    pub days: u32,
    pub services: usize,
    pub files: u64,
    pub bytes: u64,
    pub trace_ids: Vec<TraceSample>,
    pub incident: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Workload {
    pub id: &'static str,
    pub name: String,
    pub sql: String,
}

pub fn load_manifest(path: &Path) -> anyhow::Result<Manifest> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read manifest {}", path.display()))?;
    Ok(serde_json::from_str(&text)?)
}

pub fn build_workloads(manifest: &Manifest, queries_dir: &Path) -> anyhow::Result<Vec<Workload>> {
    let vars = template_vars(manifest)?;
    let defs = workload_defs();
    let mut out = Vec::with_capacity(defs.len());

    for (id, name, file) in defs {
        let path = queries_dir.join(file);
        let template = std::fs::read_to_string(&path)
            .with_context(|| format!("read query {}", path.display()))?;
        let sql = substitute(&template, &vars);
        out.push(Workload {
            id,
            name: name.to_string(),
            sql,
        });
    }

    Ok(out)
}

fn workload_defs() -> &'static [(&'static str, &'static str, &'static str)] {
    &[
        ("full_scan", "Full scan", "full_scan.sql"),
        ("dashboard", "Dashboard (1h errors by service)", "dashboard.sql"),
        ("incident", "Incident (15m 5xx window)", "incident.sql"),
        ("scoped_day", "Scoped day (payments 5xx)", "scoped_day.sql"),
        ("tight_partition", "Tight partition (1 file)", "tight_partition.sql"),
        ("selective_5xx", "Selective 5xx (all data)", "selective_5xx.sql"),
        ("trace_lookup", "Trace lookup", "trace_lookup.sql"),
        ("cross_day", "Cross-day report (api routes)", "cross_day.sql"),
        ("projection_wide", "Projection wide", "projection_wide.sql"),
    ]
}

fn template_vars(manifest: &Manifest) -> anyhow::Result<HashMap<String, String>> {
    let mut trace_ids = manifest.trace_ids.clone();
    trace_ids.sort_by(|a, b| a.date.cmp(&b.date));
    let start_date = trace_ids
        .first()
        .map(|t| t.date.clone())
        .ok_or_else(|| anyhow::anyhow!("manifest has no trace_ids"))?;
    let last_date = trace_ids.last().expect("trace_ids non-empty").date.clone();
    let trace_id = trace_ids
        .iter()
        .find(|t| t.date == last_date)
        .map(|t| t.trace_id.clone())
        .expect("trace_id for last day");

    let (incident_date, incident_hour) = incident_window(manifest, &start_date)?;

    let mut vars = HashMap::new();
    vars.insert("start_date".into(), start_date.clone());
    vars.insert("last_date".into(), last_date.clone());
    vars.insert("last_hour".into(), "23".into());
    vars.insert("trace_id".into(), trace_id);
    vars.insert(
        "incident_ts_start".into(),
        format!("{incident_date}T14:30:00"),
    );
    vars.insert(
        "incident_ts_end".into(),
        format!("{incident_date}T14:45:00"),
    );
    vars.insert("incident_date".into(), incident_date);
    vars.insert("incident_hour".into(), incident_hour);
    Ok(vars)
}

fn incident_window(manifest: &Manifest, start_date: &str) -> anyhow::Result<(String, String)> {
    if let Some(raw) = &manifest.incident {
        let day: u32 = parse_incident_field(raw, "day")?
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        let hour = parse_incident_field(raw, "hour")?
            .and_then(|h| h.split('-').next().map(str::to_string))
            .unwrap_or_else(|| "14".to_string());
        let start =
            NaiveDate::parse_from_str(start_date, "%Y-%m-%d").context("parse start_date")?;
        let date = start + Duration::days(i64::from(day.saturating_sub(1)));
        return Ok((date.format("%Y-%m-%d").to_string(), hour));
    }
    Ok((start_date.to_string(), "14".to_string()))
}

fn parse_incident_field(raw: &str, key: &str) -> anyhow::Result<Option<String>> {
    for part in raw.split(',') {
        if let Some((k, v)) = part.split_once('=') {
            if k.trim() == key {
                return Ok(Some(v.trim().to_string()));
            }
        }
    }
    Ok(None)
}

fn substitute(template: &str, vars: &HashMap<String, String>) -> String {
    let mut out = template.to_string();
    for (key, value) in vars {
        out = out.replace(&format!("{{{{{key}}}}}"), value);
    }
    out
}

pub fn default_queries_dir() -> PathBuf {
    PathBuf::from("queries")
}

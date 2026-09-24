# v3 Workload Benchmarks

_Dataset: standard tier, 200000000 rows, 7 days, 20 services, 13440 raw files._

_Median of 3 runs via MinIO._

## Raw (`s3://…/raw/`)

| Workload | Latency (ms) |
|----------|-------------:|
| Full scan | 2251.9 |
| Dashboard (1h errors by service) | 347.1 |
| Incident (15m 5xx window) | 245.8 |
| Scoped day (payments 5xx) | 443.1 |
| Tight partition (1 file) | 71.0 |
| Selective 5xx (all data) | 26010.5 |
| Trace lookup | 94320.7 |
| Cross-day report (api routes) | 16682.2 |
| Projection wide | 76684.7 |

_Median of 3 runs._

## Compacted (`s3://…/compacted/`)

| Workload | Latency (ms) |
|----------|-------------:|
| Full scan | 1784.5 |
| Dashboard (1h errors by service) | 295.6 |
| Incident (15m 5xx window) | 218.5 |
| Scoped day (payments 5xx) | 380.1 |
| Tight partition (1 file) | 49.4 |
| Selective 5xx (all data) | 12813.1 |
| Trace lookup | 81411.7 |
| Cross-day report (api routes) | 16564.9 |
| Projection wide | 78444.7 |

_Median of 3 runs._

## Raw vs compacted

| Workload | Raw (ms) | Compacted (ms) | Ratio |
|----------|----------:|---------------:|------:|
| Full scan | 2251.9 | 1784.5 | 1.26x |
| Dashboard (1h errors by service) | 347.1 | 295.6 | 1.17x |
| Incident (15m 5xx window) | 245.8 | 218.5 | 1.12x |
| Scoped day (payments 5xx) | 443.1 | 380.1 | 1.17x |
| Tight partition (1 file) | 71.0 | 49.4 | 1.44x |
| Selective 5xx (all data) | 26010.5 | 12813.1 | 2.03x |
| Trace lookup | 94320.7 | 81411.7 | 1.16x |
| Cross-day report (api routes) | 16682.2 | 16564.9 | 1.01x |
| Projection wide | 76684.7 | 78444.7 | 0.98x |


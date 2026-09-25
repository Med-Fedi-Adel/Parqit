# parqit — common commands
#
# Usage:
#   make help              show all targets
#   make generate          full 5M-row dataset
#   make inspect-flat      inspect Layout B

.DEFAULT_GOAL := help

CARGO         ?= cargo
ROWS          ?= 5000000
OUTPUT        ?= data
ROW_GROUP_SIZE ?= 100000
SEED          ?= 42
INSPECT_LIMIT ?= 5
SMOKE_ROWS    ?= 10000
SMOKE_OUTPUT  ?= data/test

LAYOUT_A      := $(OUTPUT)/layout_a
LAYOUT_B      := $(OUTPUT)/layout_b
FLAT_FILE     := $(LAYOUT_B)/part-000.parquet
JSONL_FILE    := $(OUTPUT)/logs.jsonl
HIVE_SAMPLE   := $(LAYOUT_A)/date=2026-09-01/hour=14/service=payments/part-000.parquet
RESULTS_DIR   := results
METADATA_FILE := $(RESULTS_DIR)/metadata.txt

.PHONY: help build check test clean demo \
        generate generate-smoke \
        inspect inspect-flat inspect-hive inspect-partition inspect-save \
        data-stats clean-data clean-results clean-all \
        query minio-up minio-down minio-logs \
        bench-step1 bench-step2 export-jsonl bench-compression bench-naive \
        compact compact-dev upload-v3 clean-minio-v3 \
        bench-v3-raw bench-v3-compacted bench-v3-all \
        bench-v3-concurrent bench-v3-step4 plots

##@ Help

help: ## Show this help
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage: make \033[36m<target>\033[0m\n\n"} \
		/^[a-zA-Z0-9_-]+:.*##/ {printf "  \033[36m%-22s\033[0m %s\n", $$1, $$2} \
		/^##@/ {printf "\n\033[1m%s\033[0m\n", substr($$0, 5)}' $(MAKEFILE_LIST)
	@echo ""
	@echo "Variables (override on the command line):"
	@echo "  ROWS=$(ROWS)  OUTPUT=$(OUTPUT)  ROW_GROUP_SIZE=$(ROW_GROUP_SIZE)  SEED=$(SEED)"
	@echo "  SMOKE_ROWS=$(SMOKE_ROWS)  INSPECT_LIMIT=$(INSPECT_LIMIT)"
	@echo ""
	@echo "Examples:"
	@echo "  make generate ROWS=100000 OUTPUT=data/small"
	@echo "  make inspect-hive INSPECT_LIMIT=10"

##@ Build

build: ## Build all workspace crates (release)
	$(CARGO) build --release

check: ## Fast compile check (debug)
	$(CARGO) check

test: ## Run workspace tests
	$(CARGO) test

clean: ## Remove Rust build artifacts (target/)
	$(CARGO) clean

##@ Day 1 — Generate

generate: ## Generate full dataset (Layout A + B) → $(OUTPUT)
	$(CARGO) run -p generator -- \
		--rows $(ROWS) \
		--output $(OUTPUT) \
		--row-group-size $(ROW_GROUP_SIZE) \
		--seed $(SEED)

generate-smoke: ## Quick legacy smoke test ($(SMOKE_ROWS) rows) → $(SMOKE_OUTPUT)
	$(CARGO) run -p generator -- \
		--rows $(SMOKE_ROWS) \
		--output $(SMOKE_OUTPUT) \
		--row-group-size $(ROW_GROUP_SIZE) \
		--seed $(SEED)

generate-v3-smoke: ## v3 smoke tier (10M rows, 1 day, micro-batches) → data/raw
	$(CARGO) run -p generator -- --tier smoke --output $(OUTPUT)

generate-v3-standard: ## v3 standard tier (200M rows, 7 days) → data/raw
	$(CARGO) run -p generator -- --tier standard --output $(OUTPUT)

generate-v3-stress: ## v3 stress tier (1B rows, 14 days) → data/raw
	$(CARGO) run -p generator -- --tier stress --output $(OUTPUT)

generate-v3-dev: ## v3 tiny dev run (100k rows) → data/test
	$(CARGO) run -p generator -- --tier smoke --rows 100000 --output data/test \
		--incident "day=1,hour=14-14,service=payments,error-rate=0.25"

RAW_DIR       ?= $(OUTPUT)/raw
COMPACTED_DIR ?= $(OUTPUT)/compacted

compact: ## Merge raw micro-batches → $(COMPACTED_DIR)
	$(CARGO) run -p compact -- --input $(RAW_DIR) --output $(COMPACTED_DIR)

compact-dev: ## Compact data/test/raw → data/test/compacted
	$(CARGO) run -p compact -- --input data/test/raw --output data/test/compacted \
		--report results/compaction-dev.json

upload-v3: ## Upload data/raw + data/compacted to MinIO
	@chmod +x scripts/upload-v3.sh
	@./scripts/upload-v3.sh both

upload-v3-raw: ## Upload data/raw only
	@chmod +x scripts/upload-v3.sh
	@./scripts/upload-v3.sh raw

upload-v3-compacted: ## Upload data/compacted only
	@chmod +x scripts/upload-v3.sh
	@./scripts/upload-v3.sh compacted

clean-minio-v3: ## Remove v3 raw/compacted prefixes from MinIO (fix stale counts)
	@chmod +x scripts/clean-minio-v3.sh
	@./scripts/clean-minio-v3.sh both

verify-minio: ## Compare local vs MinIO parquet file counts
	@chmod +x scripts/verify-minio.sh
	@./scripts/verify-minio.sh

##@ Day 1 — Inspect

inspect: inspect-flat inspect-hive ## Inspect both layouts

inspect-flat: ## Inspect Layout B flat file
	$(CARGO) run -p inspect -- $(FLAT_FILE)

inspect-hive: ## Inspect first $(INSPECT_LIMIT) Hive partition files
	$(CARGO) run -p inspect -- $(LAYOUT_A) --limit $(INSPECT_LIMIT)

inspect-partition: ## Inspect one Hive partition (payments, hour 14)
	$(CARGO) run -p inspect -- "$(HIVE_SAMPLE)"

inspect-save: ## Save inspect output → $(METADATA_FILE)
	@mkdir -p $(RESULTS_DIR)
	$(CARGO) run -q -p inspect -- $(FLAT_FILE) > $(METADATA_FILE)
	$(CARGO) run -q -p inspect -- $(LAYOUT_A) --limit $(INSPECT_LIMIT) >> $(METADATA_FILE)
	@echo "Wrote $(METADATA_FILE)"

##@ Day 1 — Data utilities

data-stats: ## Print parquet file counts and disk usage
	@echo "Layout A (Hive):"
	@find $(LAYOUT_A) -name '*.parquet' 2>/dev/null | wc -l | xargs -I{} echo "  files: {}"
	@du -sh $(LAYOUT_A) 2>/dev/null || echo "  (not generated yet)"
	@echo "Layout B (flat):"
	@ls -lh $(FLAT_FILE) 2>/dev/null || echo "  (not generated yet)"
	@du -sh $(LAYOUT_B) 2>/dev/null || true
	@echo "Raw (v3):"
	@find $(OUTPUT)/raw -name '*.parquet' 2>/dev/null | wc -l | xargs -I{} echo "  files: {}"
	@du -sh $(OUTPUT)/raw 2>/dev/null || echo "  (not generated yet)"
	@echo "Compacted (v3):"
	@find $(OUTPUT)/compacted -name '*.parquet' 2>/dev/null | wc -l | xargs -I{} echo "  files: {}"
	@du -sh $(OUTPUT)/compacted 2>/dev/null || echo "  (not compacted yet)"

clean-data: ## Remove generated parquet files
	rm -rf $(OUTPUT)

clean-results: ## Remove saved inspect output
	rm -rf $(RESULTS_DIR)

clean-all: clean-data clean-results clean ## Remove data, results, and target/

##@ Day 2 — Query

query: ## COUNT(*) on Layout A (Hive) in MinIO
	$(CARGO) run -p query

query-flat: ## COUNT(*) on Layout B (flat)
	$(CARGO) run -p query -- --layout flat --path layout_b

query-hive-errors: ## Error count by service (Layout A, partition + filter)
	$(CARGO) run -p query -- --sql "SELECT service, COUNT(*) AS errors FROM logs WHERE status_code >= 500 AND date = '2026-09-01' GROUP BY service ORDER BY service"

query-hive-partition: ## Tight partition filter (hour 14, payments)
	$(CARGO) run -p query -- --sql "SELECT COUNT(*) AS n FROM logs WHERE date = '2026-09-01' AND hour = '14' AND service = 'payments'"

query-explain-hive: ## EXPLAIN selective Hive query
	$(CARGO) run -p query -- --explain --sql "SELECT service, COUNT(*) AS errors FROM logs WHERE status_code >= 500 AND date = '2026-09-01' AND hour = '14' AND service = 'payments' GROUP BY service"

query-explain-flat: ## EXPLAIN same filter on Layout B (compare plans)
	$(CARGO) run -p query -- --layout flat --path layout_b --explain --sql "SELECT COUNT(*) AS n FROM logs WHERE status_code >= 500 AND service = 'payments'"

query-sql: ## Run custom SQL: make query-sql SQL="SELECT ..."
	@test -n "$(SQL)" || (echo 'Usage: make query-sql SQL="SELECT ..."' && exit 1)
	$(CARGO) run -p query -- --sql "$(SQL)"

query-both: ## Register logs + logs_flat, compare row counts
	$(CARGO) run -p query -- --both --sql "SELECT 'hive' AS layout, COUNT(*) AS n FROM logs UNION ALL SELECT 'flat', COUNT(*) FROM logs_flat"

query-latency: ## Top slow routes for api service (Hive)
	$(CARGO) run -p query -- --sql "SELECT route, AVG(latency_ms) AS avg_ms FROM logs WHERE service = 'api' AND date = '2026-09-01' GROUP BY route ORDER BY avg_ms DESC LIMIT 10"

query-explain-analyze-hive: ## EXPLAIN ANALYZE tight Hive filter
	$(CARGO) run -p query -- --explain-analyze --sql "SELECT COUNT(*) AS n FROM logs WHERE date = '2026-09-01' AND hour = '14' AND service = 'payments' AND status_code >= 500"

query-save-explains: ## Save EXPLAIN output → results/explain/
	@chmod +x scripts/save-explain.sh
	@./scripts/save-explain.sh

##@ Day 3 — Benchmarks

export-jsonl: ## Export Layout B Parquet → JSON Lines
	$(CARGO) run -p benchmark -- export-jsonl --input $(FLAT_FILE) --output $(JSONL_FILE)

bench-compression: ## JSON vs Parquet size table
	$(CARGO) run -p benchmark -- compression --jsonl $(JSONL_FILE) --flat $(FLAT_FILE) --hive $(LAYOUT_A)

bench-naive: ## Naive JSONL scan (3 runs, median)
	$(CARGO) run -p benchmark -- naive --file $(JSONL_FILE)

bench-step1: ## Day 3.1: export + compression + naive → results/benchmarks.md
	$(CARGO) run -p benchmark -- step1 --parquet $(FLAT_FILE) --jsonl $(JSONL_FILE) --hive $(LAYOUT_A)

bench-step2: ## Day 3.2: DataFusion benchmarks via MinIO
	$(CARGO) run -p benchmark -- step2

bench-v3-raw: ## v3 workloads on raw layout in MinIO
	$(CARGO) run -p benchmark -- step3-raw

bench-v3-compacted: ## v3 workloads on compacted layout in MinIO
	$(CARGO) run -p benchmark -- step3-compacted

bench-v3-all: ## v3 raw + compacted comparison → results/benchmarks_v3.md
	$(CARGO) run -p benchmark -- step3-all

bench-v3-concurrent: ## v3 concurrent mix on compacted (8 workers × 3 rounds)
	$(CARGO) run -p benchmark -- step4 --path compacted

bench-v3-step4: ## v3 concurrent raw + compacted → append Step 4 to benchmarks_v3.md
	$(CARGO) run -p benchmark -- step4-all

bench-all: bench-step1 bench-step2 ## Run full Day 3 benchmark suite

plots: ## Regenerate README charts from results/benchmarks_v3.md
	@command -v python3 >/dev/null || { echo "python3 required; pip install -r requirements-plots.txt"; exit 1; }
	@python3 -m pip install -q -r requirements-plots.txt
	@python3 scripts/plot_results.py

demo: ## End-to-end demo (v3 compacted+incident if data exists, else v2)
	@chmod +x scripts/demo.sh
	@./scripts/demo.sh

minio-up: ## Start MinIO via docker compose (Day 2)
	@test -f docker-compose.yml || (echo "docker-compose.yml not found — add it on Day 2" && exit 1)
	docker compose up -d

minio-down: ## Stop MinIO
	@test -f docker-compose.yml || (echo "docker-compose.yml not found" && exit 1)
	docker compose down

minio-logs: ## Tail MinIO container logs
	@test -f docker-compose.yml || (echo "docker-compose.yml not found" && exit 1)
	docker compose logs -f minio

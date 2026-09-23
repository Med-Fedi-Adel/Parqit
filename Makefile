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
HIVE_SAMPLE   := $(LAYOUT_A)/date=2026-09-01/hour=14/service=payments/part-000.parquet
RESULTS_DIR   := results
METADATA_FILE := $(RESULTS_DIR)/metadata.txt

.PHONY: help build check test clean \
        generate generate-smoke \
        inspect inspect-flat inspect-hive inspect-partition inspect-save \
        data-stats clean-data clean-results clean-all \
        query minio-up minio-down minio-logs

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

generate-smoke: ## Quick smoke test ($(SMOKE_ROWS) rows) → $(SMOKE_OUTPUT)
	$(CARGO) run -p generator -- \
		--rows $(SMOKE_ROWS) \
		--output $(SMOKE_OUTPUT) \
		--row-group-size $(ROW_GROUP_SIZE) \
		--seed $(SEED)

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

minio-up: ## Start MinIO via docker compose (Day 2)
	@test -f docker-compose.yml || (echo "docker-compose.yml not found — add it on Day 2" && exit 1)
	docker compose up -d

minio-down: ## Stop MinIO
	@test -f docker-compose.yml || (echo "docker-compose.yml not found" && exit 1)
	docker compose down

minio-logs: ## Tail MinIO container logs
	@test -f docker-compose.yml || (echo "docker-compose.yml not found" && exit 1)
	docker compose logs -f minio

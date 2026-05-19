.PHONY: install dev tauri-dev typecheck rust-check rust-test check build tauri-build sample-data clean

install:
	pnpm install

dev: tauri-dev

tauri-dev:
	pnpm tauri:dev

typecheck:
	pnpm typecheck

rust-check:
	cd src-tauri && cargo check

rust-test:
	cd src-tauri && cargo test

check: typecheck rust-check rust-test

build:
	pnpm build

tauri-build:
	pnpm tauri:build

sample-data:
	@case "$$(uname -s)" in \
		Darwin) DATA_DIR="$$HOME/Library/Application Support/lilimit" ;; \
		Linux) DATA_DIR="$$HOME/.config/lilimit" ;; \
		*) DATA_DIR="$$HOME/.config/lilimit" ;; \
	esac; \
	mkdir -p "$$DATA_DIR"; \
	UPDATED_AT="$$(date -u +"%Y-%m-%dT%H:%M:%SZ")"; \
	printf '%s\n' \
		'{' \
		"  \"updatedAt\": \"$$UPDATED_AT\"," \
		'  "providers": [' \
		'    { "name": "Codex", "sessionLeftPercent": 74, "weeklyLeftPercent": 61, "resetText": "2h 11m" },' \
		'    { "name": "Claude", "sessionLeftPercent": 42, "weeklyLeftPercent": 80, "resetText": "4h 03m" }' \
		'  ]' \
		'}' > "$$DATA_DIR/usage_snapshot.json"; \
	echo "Wrote $$DATA_DIR/usage_snapshot.json"

clean:
	rm -rf dist src-tauri/target

.PHONY: install dev tauri-dev typecheck rust-check rust-test secret-scan check build tauri-build clean

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

secret-scan:
	@if ! command -v gitleaks >/dev/null 2>&1; then \
		echo "gitleaks is not installed. Install it from https://github.com/gitleaks/gitleaks."; \
		exit 127; \
	fi
	gitleaks git --config .gitleaks.toml --redact .

check: typecheck rust-check rust-test

build:
	pnpm build

tauri-build:
	pnpm tauri:build

clean:
	rm -rf dist src-tauri/target

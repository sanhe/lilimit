.PHONY: install dev tauri-dev typecheck rust-fmt-check rust-clippy rust-check rust-build rust-test secret-scan check ci analyze build tauri-build clean

install:
	pnpm install

dev: tauri-dev

tauri-dev:
	pnpm tauri:dev

typecheck:
	pnpm typecheck

rust-fmt-check:
	cd src-tauri && cargo fmt --check

rust-clippy:
	cd src-tauri && cargo clippy --locked --all-targets -- -D warnings

rust-check:
	cd src-tauri && cargo check --locked

rust-build:
	cd src-tauri && cargo build --locked

rust-test:
	cd src-tauri && cargo test --locked

secret-scan:
	@if ! command -v gitleaks >/dev/null 2>&1; then \
		echo "gitleaks is not installed. Install it from https://github.com/gitleaks/gitleaks."; \
		exit 127; \
	fi
	gitleaks git --config .gitleaks.toml --redact --log-opts="--all" .

check: typecheck rust-check rust-test

ci: build rust-fmt-check rust-clippy rust-check rust-build rust-test

analyze: ci secret-scan

build:
	pnpm build

tauri-build:
	pnpm tauri:build

clean:
	rm -rf dist src-tauri/target

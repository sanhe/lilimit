.PHONY: install dev tauri-dev typecheck rust-check rust-test check build tauri-build clean

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

clean:
	rm -rf dist src-tauri/target

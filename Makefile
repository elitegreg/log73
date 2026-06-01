.PHONY: all release \
	backend launcher frontend \
	backend-build backend-release backend-test backend-fmt backend-lint \
	launcher-build launcher-test launcher-fmt launcher-lint launcher-run \
	frontend-build frontend-release frontend-test frontend-fmt frontend-lint

all: backend-build launcher-build frontend-build

release: backend-release launcher-build frontend-release

backend: backend-fmt backend-lint backend-test backend-build

launcher: launcher-fmt launcher-lint launcher-test launcher-build

frontend: frontend-fmt frontend-lint frontend-test frontend-build

backend-build:
	cargo build -p log73-backend

backend-release: frontend-release
	cargo build --release -p log73-backend

backend-test:
	cargo test -p log73-backend

backend-fmt:
	cargo fmt -p log73-backend

backend-lint:
	cargo clippy -p log73-backend --all-targets --all-features

launcher-build:
	cargo build -p launcher

launcher-test:
	cargo test -p launcher

launcher-fmt:
	cargo fmt -p launcher

launcher-lint:
	cargo clippy -p launcher --all-targets --all-features

launcher-run:
	cargo run -p launcher

frontend-build:
	pnpm run build

frontend-release:
	pnpm run build

frontend-test:
	pnpm run test:frontend

frontend-fmt:
	pnpm run format

frontend-lint:
	pnpm run lint

.PHONY: all release \
	backend frontend \
	backend-build backend-release backend-test backend-fmt backend-lint \
	frontend-build frontend-release frontend-test frontend-fmt frontend-lint

all: backend-build frontend-build

release: backend-release frontend-release

backend: backend-fmt backend-lint backend-test backend-build

frontend: frontend-fmt frontend-lint frontend-test frontend-build

backend-build:
	cd backend && cargo build

backend-release: frontend-release
	cd backend && cargo build --release

backend-test:
	cd backend && cargo test

backend-fmt:
	cd backend && cargo fmt

backend-lint:
	cd backend && cargo clippy --all-targets --all-features

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

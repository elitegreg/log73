DEB_TARGET ?= x86_64-unknown-linux-gnu
VERSION ?= $(shell awk -F '"' '/^version =/ { print $$2; exit }' backend/Cargo.toml)
DIST ?= $(shell command -v dist 2>/dev/null || printf '%s' "$$HOME/.cargo/bin/dist")
NFPM ?= $(shell command -v nfpm 2>/dev/null || { command -v go >/dev/null 2>&1 && printf '%s' "$$(go env GOPATH)/bin/nfpm"; } || printf '%s' nfpm)

.PHONY: all release deb help \
	backend launcher frontend \
	backend-build backend-release backend-test backend-fmt backend-lint \
	launcher-build launcher-test launcher-fmt launcher-lint launcher-run \
	frontend-build frontend-release frontend-test frontend-fmt frontend-lint

all: backend-build launcher-build frontend-build

release: backend-release launcher-build frontend-release

deb:
	@if ! command -v "$(DIST)" >/dev/null 2>&1 && [ ! -x "$(DIST)" ]; then \
		echo "dist not found; install with: cargo install cargo-dist --version 0.32.0 --locked"; \
		exit 1; \
	fi
	pnpm run build:production
	"$(DIST)" build --artifacts=local --target="$(DEB_TARGET)" --allow-dirty
	NFPM="$(NFPM)" scripts/build_native_linux_packages.sh "$(DEB_TARGET)" "$(VERSION)"

help:
	mkdir -p docs/help
	pandoc docs/index.md -s -c docs/help.css --lua-filter=docs/replace-links.lua -o docs/help/index.html
	pandoc docs/keyboard-shortcuts.md -s -c docs/help.css --lua-filter=docs/replace-links.lua -o docs/help/keyboard-shortcuts.html
	pandoc docs/manual.md -s -c docs/help.css --lua-filter=docs/replace-links.lua -o docs/help/manual.html

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
	cargo clippy -p log73-backend --all-targets --all-features -- -D dead_code

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

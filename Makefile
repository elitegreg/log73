DEB_TARGET ?= x86_64-unknown-linux-gnu
VERSION ?= $(shell awk -F '"' '/^version =/ { print $$2; exit }' backend/Cargo.toml)
DIST ?= $(shell command -v dist 2>/dev/null || printf '%s' "$$HOME/.cargo/bin/dist")
NFPM ?= $(shell command -v nfpm 2>/dev/null || { command -v go >/dev/null 2>&1 && printf '%s' "$$(go env GOPATH)/bin/nfpm"; } || printf '%s' nfpm)

.PHONY: all release deb package-smoke help \
	backend launcher frontend \
	radio-client \
	backend-build backend-release backend-test backend-fmt backend-lint \
	launcher-build launcher-test launcher-fmt launcher-lint launcher-run \
	frontend-build frontend-release frontend-test frontend-fmt frontend-lint \
	radio-client-build radio-client-test radio-client-fmt radio-client-lint radio-client-run

all: backend-build launcher-build frontend-build radio-client-build

ci: backend-lint frontend-lint backend-test frontend-test frontend-build radio-client-test radio-client-build

release: backend-release launcher-build frontend-release radio-client-build

deb:
	@if ! command -v "$(DIST)" >/dev/null 2>&1 && [ ! -x "$(DIST)" ]; then \
		echo "dist not found; install with: cargo install cargo-dist --version 0.32.0 --locked"; \
		exit 1; \
	fi
	pnpm run build:production
	"$(DIST)" build --artifacts=local --target="$(DEB_TARGET)" --allow-dirty
	NFPM="$(NFPM)" scripts/build_native_linux_packages.sh "$(DEB_TARGET)" "$(VERSION)"

package-smoke:
	@tmp_dir="$$(mktemp -d)"; \
	trap 'rm -rf "$$tmp_dir"' EXIT; \
	install -d "$$tmp_dir/opt/log73/bin" "$$tmp_dir/usr/share/applications" "$$tmp_dir/usr/share/icons/hicolor/512x512/apps"; \
	install -m 0755 target/debug/log73-backend "$$tmp_dir/opt/log73/bin/log73-backend"; \
	install -m 0755 target/debug/log73-launcher "$$tmp_dir/opt/log73/bin/log73-launcher"; \
	install -m 0755 target/debug/log73-radio-client "$$tmp_dir/opt/log73/bin/log73-radio-client"; \
	install -m 0644 static/log73-icon-512.png "$$tmp_dir/usr/share/icons/hicolor/512x512/apps/log73.png"; \
	install -m 0644 static/log73-icon-512.png "$$tmp_dir/usr/share/icons/hicolor/512x512/apps/log73-radio-client.png"; \
	printf '%s\n' '[Desktop Entry]' 'Name=Log73' 'Exec=/opt/log73/bin/log73-launcher' > "$$tmp_dir/usr/share/applications/log73.desktop"; \
	printf '%s\n' '[Desktop Entry]' 'Name=Log73 Radio Client' 'Exec=/opt/log73/bin/log73-radio-client' > "$$tmp_dir/usr/share/applications/log73-radio-client.desktop"; \
	scripts/check_native_package_contents.sh "$$tmp_dir"

help:
	mkdir -p docs/help
	pandoc docs/index.md -s -c docs/help.css --lua-filter=docs/replace-links.lua -o docs/help/index.html
	pandoc docs/keyboard-shortcuts.md -s -c docs/help.css --lua-filter=docs/replace-links.lua -o docs/help/keyboard-shortcuts.html
	pandoc docs/manual.md -s -c docs/help.css --lua-filter=docs/replace-links.lua -o docs/help/manual.html

backend: backend-fmt backend-lint backend-test backend-build

launcher: launcher-fmt launcher-lint launcher-test launcher-build

frontend: frontend-fmt frontend-lint frontend-test frontend-build

radio-client: radio-client-fmt radio-client-lint radio-client-test radio-client-build

backend-build:
	cargo build -p log73-backend

backend-release: frontend-release
	cargo build --release -p log73-backend

backend-test:
	cargo test -p log73-backend -p radio-io

backend-fmt:
	cargo fmt -p log73-backend

backend-lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --all-features -- -D warnings

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

radio-client-build:
	cargo build -p log73-radio-client

radio-client-test:
	cargo test -p log73-radio-client

radio-client-fmt:
	cargo fmt -p log73-radio-client

radio-client-lint:
	cargo clippy -p log73-radio-client --all-targets --all-features -- -D warnings

radio-client-run:
	cargo run -p log73-radio-client

# Common tasks. `make` builds the release binary; `make help` lists them all. Everything uses
# --locked, like CI.
#
#   make                the optimised binary: target/release/putio-adder
#   make check          what CI runs on every change: format, clippy, tests
#   make run            the app, in a debug build
#   make demo           the app with sample data (no account, no network, no Keychain)
#   make app            macOS: target/put.io adder.app
#
# Needs: Rust (see rust-toolchain.toml) and, for a few targets, python3. macOS-only targets say so.

CARGO   ?= cargo
PACKAGE := putio-adder
VERSION := $(shell sed -n 's/^version = "\(.*\)"$$/\1/p' Cargo.toml | head -1)
UNAME   := $(shell uname -s)

# Demo mode: `make demo VIEW=prefs THEME=dark` (see docs/development.md for the views).
VIEW  ?= list
THEME ?= system

.DEFAULT_GOAL := build-release
.PHONY: help version build build-release run run-release demo \
        fmt fmt-check lint lint-release test check ci tools-test spec-check \
        app open dmg zip sbom deny audit \
        release-dry-run release-tag clean

help: ## Show this list (plain `make` builds the release binary)
	@awk 'BEGIN {FS = ":.*## "} /^[a-zA-Z_-]+:.*## / {printf "  \033[1m%-16s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

version: ## Print the app version (from Cargo.toml)
	@echo $(VERSION)

# ---- build and run ------------------------------------------------------------------------------

build: ## Debug build of every crate
	$(CARGO) build --workspace --locked

build-release: ## Optimised binary: target/release/putio-adder (the default: plain `make`)
	$(CARGO) build --release --locked -p $(PACKAGE)

run: ## Run the app (debug build, your real account and Keychain)
	$(CARGO) run --locked -p $(PACKAGE)

run-release: ## Run the optimised build
	$(CARGO) run --release --locked -p $(PACKAGE)

demo: ## Run with sample data: make demo [VIEW=list|empty|prefs|menu|...] [THEME=light|dark|system]
	PUTIO_ADDER_DEMO=$(VIEW) PUTIO_ADDER_THEME=$(THEME) $(CARGO) run --locked -p $(PACKAGE)

# ---- checks -------------------------------------------------------------------------------------

fmt: ## Format the code
	$(CARGO) fmt --all

fmt-check: ## Fail if the code is not formatted
	$(CARGO) fmt --all --check

lint: ## Clippy, warnings are errors (debug profile)
	$(CARGO) clippy --workspace --all-targets --locked -- -D warnings

lint-release: ## Clippy in the release profile (catches release-only warnings)
	$(CARGO) clippy --release --workspace --all-targets --locked -- -D warnings

test: ## Run every test
	$(CARGO) test --workspace --locked

check: fmt-check lint test ## Format, clippy and tests: what CI runs for Rust

tools-test: ## Python tests for the tools, and a syntax check of the shell scripts
	@for t in tools/test_*.py; do python3 -I "$$t" || exit 1; done
	@for f in tools/*.sh packaging/linux/*.sh; do sh -n "$$f" || exit 1; done
	@echo "tools ok"

spec-check: ## Check the vendored put.io API spec against its digest
	cd crates/putio-api/tests/spec && shasum -a 256 -c putio-2.8.14.json.sha256

ci: check lint-release tools-test spec-check ## Everything above, in one go

deny: ## Dependency policy (needs cargo-deny: cargo install cargo-deny)
	@command -v cargo-deny >/dev/null || { echo "cargo-deny is not installed: cargo install cargo-deny --locked"; exit 1; }
	$(CARGO) deny check

audit: ## Known vulnerabilities in dependencies (needs cargo-audit)
	@command -v cargo-audit >/dev/null || { echo "cargo-audit is not installed: cargo install cargo-audit --locked"; exit 1; }
	$(CARGO) audit

# ---- macOS app ----------------------------------------------------------------------------------

define need_macos
	@[ "$(UNAME)" = "Darwin" ] || { echo "$@ only works on macOS"; exit 1; }
endef

app: ## macOS: build target/put.io adder.app (release build, ad-hoc signed)
	$(need_macos)
	tools/bundle-macos.sh

open: app ## macOS: build the app and open it (quits a running copy first)
	-@pkill -f "put.io adder.app/Contents/MacOS" 2>/dev/null; sleep 0.5
	open "target/put.io adder.app"

dmg: app ## macOS: disk image target/putio-adder-vX.Y.Z.dmg
	tools/make-dmg.sh "target/put.io adder.app" "target/putio-adder-v$(VERSION).dmg"

zip: app ## macOS: zip of the app (the shape the in-app updater downloads)
	ditto -c -k --keepParent "target/put.io adder.app" "target/putio-adder-v$(VERSION)-macos.zip"
	@echo "wrote target/putio-adder-v$(VERSION)-macos.zip"

# ---- release ------------------------------------------------------------------------------------

sbom: ## Software bill of materials and third-party licences into target/
	python3 tools/sbom.py --out target/sbom.cdx.json --licenses target/THIRD_PARTY.txt

release-dry-run: ## Check a release changes nothing yet: make release-dry-run VERSION=0.2.0
	@[ "$(origin VERSION)" = "command line" ] || { echo "usage: make release-dry-run VERSION=x.y.z"; exit 1; }
	tools/release.sh $(VERSION) --dry-run

release-tag: ## Bump the version and make the signed tag (never pushes): make release-tag VERSION=0.2.0
	@[ "$(origin VERSION)" = "command line" ] || { echo "usage: make release-tag VERSION=x.y.z"; exit 1; }
	tools/release.sh $(VERSION)

clean: ## Remove build output
	$(CARGO) clean

# ─── Development ──────────────────────────────────────────────

# Run all checks (fmt, clippy, test, build)
check:
    cargo fmt --all -- --check
    cargo clippy --workspace -- -D warnings
    cargo test --workspace
    cargo build --workspace

# Run tests
test *ARGS:
    cargo test --workspace {{ARGS}}

# Run clippy
lint:
    cargo clippy --workspace -- -D warnings

# Format code
fmt:
    cargo fmt --all

# Build release binary
build:
    cargo build --release

# Run the CLI
run *ARGS:
    cargo run -p modde-cli -- {{ARGS}}

# Run the GUI
gui:
    cargo run -p modde-ui

# ─── Coverage ─────────────────────────────────────────────────

# Run workspace coverage and print a summary table
coverage:
    cargo llvm-cov --workspace --all-features --summary-only

# Generate HTML coverage report at target/llvm-cov/html/index.html
coverage-html:
    cargo llvm-cov --workspace --all-features --html

# Generate lcov.info at target/llvm-cov/lcov.info
coverage-lcov:
    cargo llvm-cov --workspace --all-features --lcov --output-path target/llvm-cov/lcov.info

# Coverage run for CI: gathers once, emits lcov + summary, fails under threshold (override with FAIL_UNDER=N)
coverage-ci FAIL_UNDER='0':
    mkdir -p target/llvm-cov
    cargo llvm-cov --workspace --all-features --no-report
    cargo llvm-cov report --lcov --output-path target/llvm-cov/lcov.info
    cargo llvm-cov report --summary-only --fail-under-lines {{FAIL_UNDER}}

# ─── Publishing ───────────────────────────────────────────────

# Dry-run a cargo-release workspace release (usage: just release-dry 0.2.0)
release-dry VERSION:
    cargo release {{VERSION}} --workspace --no-confirm

# Release all crates with cargo-release, then push the tag (usage: just release 0.2.0)
release VERSION:
    cargo release {{VERSION}} --workspace --execute --no-confirm

# ─── COPR ─────────────────────────────────────────────────────

# Generate vendored tarball for COPR
copr-vendor:
    cargo vendor
    tar czf vendor.tar.gz vendor
    rm -rf vendor

# Build SRPM locally (for testing)
copr-srpm VERSION:
    curl -Lo rs-modde-v{{VERSION}}.tar.gz \
        "https://codeberg.org/caniko/rs-modde/archive/v{{VERSION}}.tar.gz"
    @just copr-vendor
    rpmbuild -bs modde.spec \
        --define "_sourcedir $(pwd)" \
        --define "_srcrpmdir $(pwd)/srpms"

# ─── Nix ──────────────────────────────────────────────────────

# Build with nix
nix-build:
    nix build .#modde

# Build docs site
nix-docs:
    nix build .#site

# Enter dev shell
nix-dev:
    nix develop

# ─── Docs ─────────────────────────────────────────────────────

# Serve docs locally
docs-serve:
    cd docs/site && zola serve

# Serve website locally
site-serve:
    cd website && zola serve

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

# ─── Publishing ───────────────────────────────────────────────

# Publish all crates to crates.io (dependency order)
publish:
    cargo publish -p modde-core
    cargo publish -p modde-sources
    cargo publish -p modde-games
    cargo publish -p modde-ui --no-verify
    cargo publish -p modde-cli --no-verify

# Dry-run publish (no upload)
publish-dry:
    cargo publish -p modde-core --dry-run
    cargo publish -p modde-sources --dry-run
    cargo publish -p modde-games --dry-run
    cargo publish -p modde-ui --dry-run --no-verify
    cargo publish -p modde-cli --dry-run --no-verify

# Tag and push a release (usage: just release 0.2.0)
release VERSION:
    sed -i 's/^Version:.*/Version:        {{VERSION}}/' modde.spec
    git add modde.spec
    git commit -m "chore: bump spec to v{{VERSION}}"
    git tag -a "v{{VERSION}}" -m "v{{VERSION}}"
    git push origin trunk "v{{VERSION}}"

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

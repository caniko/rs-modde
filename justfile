export-tool-schema:
    cargo run -p modde-cli --quiet -- dev export-tool-schema

check-tool-schema-fresh:
    cargo run -p modde-cli --quiet -- dev export-tool-schema --out /tmp/modde-tool-schema.nix
    diff -u nix/tool-schema.nix /tmp/modde-tool-schema.nix

website-serve:
    mkdir -p website/data
    cp docs/capability-matrix.toml website/data/capability-matrix.toml
    cd website && zola serve

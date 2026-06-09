export-tool-schema:
    cargo run -p modde-cli --quiet -- dev export-tool-schema

check-tool-schema-fresh:
    cargo run -p modde-cli --quiet -- dev export-tool-schema --out /tmp/modde-tool-schema.nix
    diff -u nix/tool-schema.nix /tmp/modde-tool-schema.nix

website-serve:
    plinth-project serve --config website/plinth-project.toml --out website/public

website-audit *ARGS:
    plinth-project audit install --config website/plinth-project.toml --out website/public --rubric-bin "${VISUAL_RUBRIC_BIN:-visual-rubric}" {{ARGS}}
    just website-assert-person-links

website-assert-person-links:
    test -f website/public/index.html
    rg -F -q 'application/ld+json' website/public/index.html
    rg -F -q 'hero-byline' website/public/index.html
    rg -F -q 'person-attribution' website/public/index.html
    rg -F -q 'Caniko' website/public/index.html
    rg -F -q 'person-mention' website/public/index.html

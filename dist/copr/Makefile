.PHONY: srpm

VERSION = $(shell grep '^Version:' dist/rpm/modde.spec | awk '{print $$2}')
ARTIFACT_ROOT ?= target/modde-release/root-artifacts
SRPM_DIR ?= $(ARTIFACT_ROOT)/srpms
WORK_DIR ?= $(ARTIFACT_ROOT)/copr-work

srpm:
	mkdir -p "$(ARTIFACT_ROOT)" "$(SRPM_DIR)" "$(WORK_DIR)"
	rm -rf "$(WORK_DIR)/rs-modde"
	curl -Lo "$(ARTIFACT_ROOT)/rs-modde-v$(VERSION).tar.gz" \
		"https://codeberg.org/caniko/rs-modde/archive/v$(VERSION).tar.gz"
	tar xf "$(ARTIFACT_ROOT)/rs-modde-v$(VERSION).tar.gz" -C "$(WORK_DIR)"
	cd "$(WORK_DIR)/rs-modde" && cargo vendor vendor > "$(CURDIR)/$(ARTIFACT_ROOT)/cargo-vendor-config.toml"
	tar czf "$(ARTIFACT_ROOT)/vendor.tar.gz" -C "$(WORK_DIR)/rs-modde" vendor
	rpmbuild -bs dist/rpm/modde.spec \
		--define "_sourcedir $(CURDIR)/$(ARTIFACT_ROOT)" \
		--define "_srcrpmdir $(CURDIR)/$(SRPM_DIR)"

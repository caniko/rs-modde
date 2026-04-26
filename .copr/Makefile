.PHONY: srpm

VERSION = $(shell grep '^Version:' modde.spec | awk '{print $$2}')

srpm:
	curl -Lo rs-modde-v$(VERSION).tar.gz \
		"https://codeberg.org/caniko/rs-modde/archive/v$(VERSION).tar.gz"
	tar xf rs-modde-v$(VERSION).tar.gz
	cd rs-modde && cargo vendor
	tar czf vendor.tar.gz -C rs-modde vendor
	rpmbuild -bs modde.spec \
		--define "_sourcedir $(CURDIR)" \
		--define "_srcrpmdir $(outdir)"

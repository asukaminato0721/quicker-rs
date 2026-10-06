PREFIX ?= $(HOME)/.local
DESTDIR ?=

.PHONY: build install uninstall test
build:
	cargo build --release --locked

install: build
	install -Dm755 target/release/quicker-rs "$(DESTDIR)$(PREFIX)/bin/quicker-rs"
	install -Dm644 assets/net.getquicker.QuickerRS.desktop "$(DESTDIR)$(PREFIX)/share/applications/net.getquicker.QuickerRS.desktop"
	install -Dm644 assets/net.getquicker.QuickerRS.svg "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/net.getquicker.QuickerRS.svg"

uninstall:
	rm -f "$(DESTDIR)$(PREFIX)/bin/quicker-rs"
	rm -f "$(DESTDIR)$(PREFIX)/share/applications/net.getquicker.QuickerRS.desktop"
	rm -f "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/net.getquicker.QuickerRS.svg"

test:
	cargo test --all-targets --locked

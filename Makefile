# Makefile for lksystem.
#
# There is no native C code in this tree: every installed program is built
# by Cargo from either the root crate (the public lksys* tools and the PID 1
# binary) or the etc/ crate (the boot-stage programs).

PREFIX    ?= /usr
DESTDIR   ?=
CARGO     ?= cargo
CARGOFLAGS ?= --release
TARGET_DIR     := target/x86_64-unknown-linux-musl/release
ETC_TARGET_DIR := etc/target/x86_64-unknown-linux-musl/release
SBINDIR      := $(DESTDIR)$(PREFIX)/sbin
LKSYSTEM_DIR := $(DESTDIR)/etc/lksystem
SERVICES_DIR := $(LKSYSTEM_DIR)/services
LICENSE_DIR  := $(DESTDIR)$(PREFIX)/share/licenses/lksystem

# Public commands built from the root crate.
BIN_TOOLS := lksystem lksys lksysdir lksysctl lksyschdir lksyslogd chpst

# Fixed-path boot stages built from the etc/ crate. Each entry is
# cargo-binary-name:installed-name, since the stage files are installed
# without their lksystem-stage prefix at fixed paths lksystem expects.
STAGE_MAP := lksystem-stage1:1 lksystem-stage2:2 lksystem-stage3:3 \
             lksystem-ctrlaltdel:ctrlaltdel
.PHONY: all build build-tools build-stages check test install \
        install-tools install-stages install-services clean
all: build
build: build-tools build-stages
build-tools:
	$(CARGO) build $(CARGOFLAGS)
build-stages:
	$(CARGO) build $(CARGOFLAGS) --manifest-path etc/Cargo.toml
check: test
test: build
	$(CARGO) test $(CARGOFLAGS) --all-targets
	$(CARGO) test $(CARGOFLAGS) --all-targets --manifest-path etc/Cargo.toml
install: install-tools install-stages install-services
install-tools: build-tools
	install -d "$(SBINDIR)"
	for tool in $(BIN_TOOLS); do \
		install -Dm0755 "$(TARGET_DIR)/$$tool" "$(SBINDIR)/$$tool"; \
	done
install-stages: build-stages
	install -d "$(LKSYSTEM_DIR)"
	for entry in $(STAGE_MAP); do \
		cargo_name=$${entry%%:*}; \
		installed_name=$${entry##*:}; \
		install -Dm0755 "$(ETC_TARGET_DIR)/$$cargo_name" \
			"$(LKSYSTEM_DIR)/$$installed_name"; \
	done
install-services:
	install -d "$(SERVICES_DIR)"
	cp -a etc/services/. "$(SERVICES_DIR)/"
	find "$(SERVICES_DIR)" -name run -exec chmod 0755 {} +
clean:
	$(CARGO) clean
	$(CARGO) clean --manifest-path etc/Cargo.toml

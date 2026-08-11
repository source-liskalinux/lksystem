PACKAGE=lksystem-1.0.0
DIRS=etc src

PREFIX?=/usr/local
SBINDIR?=$(PREFIX)/sbin
SYSCONFDIR?=/etc
DESTDIR?=
INSTALL?=install
C_BINARIES=chpst lksystem lksystem-init lksys lksyschdir lksysdir lksysctl lksyslogd
RUST_BINARIES=lksystem-stage1 lksystem-stage2 lksystem-stage3 lksystem-ctrlaltdel
GETTY_SERVICES=getty-tty1 getty-tty2 getty-tty3 getty-tty4 getty-tty5 getty-tty6 getty-tty7 getty-tty8

all: rust c

c:
	$(MAKE) -C src

rust:
	cargo build --manifest-path etc/Cargo.toml --release

check: c rust
	$(MAKE) -C src check IT="$(C_BINARIES)"
	cargo test --manifest-path etc/Cargo.toml

install: all
	$(INSTALL) -d $(DESTDIR)$(SBINDIR) $(DESTDIR)$(SYSCONFDIR)/lksystem/service
	$(INSTALL) -m 0755 $(addprefix src/,$(C_BINARIES)) $(DESTDIR)$(SBINDIR)/
	$(INSTALL) -m 0755 $(addprefix etc/target/release/,$(RUST_BINARIES)) $(DESTDIR)$(SBINDIR)/
	$(INSTALL) -m 0755 etc/target/release/lksystem-stage1 $(DESTDIR)$(SYSCONFDIR)/lksystem/1
	$(INSTALL) -m 0755 etc/target/release/lksystem-stage2 $(DESTDIR)$(SYSCONFDIR)/lksystem/2
	$(INSTALL) -m 0755 etc/target/release/lksystem-stage3 $(DESTDIR)$(SYSCONFDIR)/lksystem/3
	$(INSTALL) -m 0755 etc/target/release/lksystem-ctrlaltdel $(DESTDIR)$(SYSCONFDIR)/lksystem/ctrlaltdel
	$(INSTALL) -d $(DESTDIR)$(SYSCONFDIR)/lksystem/service/dbus $(DESTDIR)$(SYSCONFDIR)/lksystem/service/elogind $(DESTDIR)$(SYSCONFDIR)/lksystem/service/networkmanager $(addprefix $(DESTDIR)$(SYSCONFDIR)/lksystem/service/,$(GETTY_SERVICES))
	$(INSTALL) -m 0755 etc/service/dbus/run $(DESTDIR)$(SYSCONFDIR)/lksystem/service/dbus/run
	$(INSTALL) -m 0755 etc/service/elogind/run $(DESTDIR)$(SYSCONFDIR)/lksystem/service/elogind/run
	$(INSTALL) -m 0755 etc/service/networkmanager/run $(DESTDIR)$(SYSCONFDIR)/lksystem/service/networkmanager/run
	for service in $(GETTY_SERVICES); do $(INSTALL) -m 0755 etc/service/$$service/run $(DESTDIR)$(SYSCONFDIR)/lksystem/service/$$service/run; done

clean:
	find . -name \*~ -exec rm -f {} \;
	find . -name .??*~ -exec rm -f {} \;
	find . -name \#?* -exec rm -f {} \;

cleaner: clean
	rm -f $(PACKAGE).tar.gz
	rm -f doc/*.html man/*.[0-9] .doc .man

.PHONY: help build install clean clippy fmt tauri-dev tauri-build desktop-dev desktop-build linux-build linux-package linux-deploy server-dev

INSTALL_DIR ?= /opt/arguswing
SERVICE_USER ?= arguswing
SERVICE_GROUP ?= $(SERVICE_USER)
SERVICE_NAME ?= arguswing
DEPLOY_MODE ?= server
ARGUS_SERVER_ADDR ?= 0.0.0.0:3010
DATABASE_URL ?= postgres://argus:argus_dev@127.0.0.1:5432/argus_dev
ETC_DIR ?= /etc/arguswing
SYSTEMD_DIR ?= /etc/systemd/system
NGINX_CONF_DIR ?= /etc/nginx/conf.d
LINUX_STAGE_DIR ?= target/linux/arguswing
SERVER_ADDR_SERVER ?= $(ARGUS_SERVER_ADDR)
SERVER_ADDR_NGINX ?= 127.0.0.1:3010

help:
	@printf '%s\n' \
		'Available targets:' \
		'  build            Build the workspace' \
		'  install          Install required tools (sqlx-cli, prek)' \
		'  clean            Clean build artifacts' \
		'  clippy           Run clippy linter' \
		'  fmt              Format code' \
		'  linux-build      Build argus-server and apps/web for Linux deployment' \
		'  server-dev       Run argus-server locally with server-hosted web assets' \
		'  linux-package    Stage Linux deployment files under target/linux/arguswing' \
		'  linux-deploy     Install staged Linux service and restart systemd service' \
		'  tauri-dev        Run Tauri desktop app in dev mode' \
		'  tauri-build      Build Tauri desktop app for production' \
		'  desktop-dev      Alias for tauri-dev' \
		'  desktop-build    Alias for tauri-build'

build:
	cargo build --workspace

clean:
	cargo clean

install-tools:
	cargo install sqlx-cli --no-default-features --features sqlite
	cargo install prek
	cargo install --locked cargo-deny && cargo deny init && cargo deny check
	prek install

install: install-tools

# Run clippy linter
clippy:
	cargo clippy --workspace --all-targets

# Format code
fmt:
	cargo fmt --all
	cargo fmt --check --all

# Run Tauri desktop app in dev mode
tauri-dev:
	cd crates/desktop && pnpm install && pnpm tauri dev

# Build Tauri desktop app for production
tauri-build:
	cd crates/desktop && pnpm install && CI=true pnpm tauri build

# Build the Linux server binary and web admin assets.
linux-build:
	cargo build --release -p argus-server
	cd apps/web && pnpm install --frozen-lockfile && pnpm build

# Build and run the server-hosted release build locally for manual testing.
server-dev: linux-build
	install -d "$(CURDIR)/.tmp/arguswing-dev/data" "$(CURDIR)/.tmp/arguswing-dev/traces"
	printf '%s\n' \
		'[server]' \
		'bind_addr = "$(ARGUS_SERVER_ADDR)"' \
		'web_dist_dir = "$(CURDIR)/apps/web/dist"' \
		'' \
		'[database]' \
		'url = "$(DATABASE_URL)"' \
		'' \
		'[trace]' \
		'dir = "$(CURDIR)/.tmp/arguswing-dev/traces"' \
		'' \
		'[crypto]' \
		'master_key_path = "$(CURDIR)/.tmp/arguswing-dev/master.key"' \
		'' \
		'[auth]' \
		'dev_enabled = true' \
		'' \
		'[auth.oauth]' \
		'enabled = false' \
		'' \
		'[logging]' \
		'level = "info"' > "$(CURDIR)/.tmp/arguswing-dev/arguswing.toml"
	./target/release/argus-server --config "$(CURDIR)/.tmp/arguswing-dev/arguswing.toml"

# Stage Linux deployment files without touching system directories.
linux-package: linux-build
	rm -rf "$(LINUX_STAGE_DIR)"
	install -d "$(LINUX_STAGE_DIR)/bin" "$(LINUX_STAGE_DIR)/web" "$(LINUX_STAGE_DIR)/deploy/systemd" "$(LINUX_STAGE_DIR)/deploy/nginx"
	install -m 0755 target/release/argus-server "$(LINUX_STAGE_DIR)/bin/argus-server"
	cp -R apps/web/dist/. "$(LINUX_STAGE_DIR)/web/"
	cp deploy/systemd/arguswing.service deploy/systemd/arguswing.toml.example "$(LINUX_STAGE_DIR)/deploy/systemd/"
	cp deploy/nginx/arguswing.conf "$(LINUX_STAGE_DIR)/deploy/nginx/"

# Install and restart the Linux service. Run as root, e.g.:
# sudo make linux-deploy DEPLOY_MODE=server
# sudo make linux-deploy DEPLOY_MODE=nginx
linux-deploy:
	@if [ "$(DEPLOY_MODE)" != "server" ] && [ "$(DEPLOY_MODE)" != "nginx" ]; then \
		printf 'DEPLOY_MODE must be server or nginx, got %s\n' "$(DEPLOY_MODE)" >&2; \
		exit 2; \
	fi
	@if [ "$$(id -u)" -ne 0 ]; then \
		printf 'linux-deploy must run as root. Try: sudo make linux-deploy DEPLOY_MODE=%s\n' "$(DEPLOY_MODE)" >&2; \
		exit 2; \
	fi
	$(MAKE) linux-package
	@if ! getent group "$(SERVICE_GROUP)" >/dev/null; then groupadd --system "$(SERVICE_GROUP)"; fi
	@if ! id -u "$(SERVICE_USER)" >/dev/null 2>&1; then useradd --system --gid "$(SERVICE_GROUP)" --home-dir "$(INSTALL_DIR)" --shell /usr/sbin/nologin "$(SERVICE_USER)"; fi
	install -d -o "$(SERVICE_USER)" -g "$(SERVICE_GROUP)" "$(INSTALL_DIR)" "$(INSTALL_DIR)/bin" "$(INSTALL_DIR)/web" "$(INSTALL_DIR)/data" "$(INSTALL_DIR)/traces"
	install -d -m 0750 -o "$(SERVICE_USER)" -g "$(SERVICE_GROUP)" "$(ETC_DIR)"
	install -m 0755 "$(LINUX_STAGE_DIR)/bin/argus-server" "$(INSTALL_DIR)/bin/argus-server"
	rm -rf "$(INSTALL_DIR)/web"
	install -d -o "$(SERVICE_USER)" -g "$(SERVICE_GROUP)" "$(INSTALL_DIR)/web"
	cp -R "$(LINUX_STAGE_DIR)/web/." "$(INSTALL_DIR)/web/"
	chown -R "$(SERVICE_USER):$(SERVICE_GROUP)" "$(INSTALL_DIR)"
	@if [ "$(DEPLOY_MODE)" = "server" ]; then \
		printf '%s\n' \
			'[server]' \
			'bind_addr = "$(SERVER_ADDR_SERVER)"' \
			'web_dist_dir = "$(INSTALL_DIR)/web"' \
			'' \
			'[database]' \
			'url = "$(DATABASE_URL)"' \
			'' \
			'[trace]' \
			'dir = "$(INSTALL_DIR)/traces"' \
			'' \
			'[crypto]' \
			'master_key_path = "$(ETC_DIR)/master.key"' \
			'' \
			'[auth]' \
			'dev_enabled = false' \
			'' \
			'[auth.oauth]' \
			'enabled = false' \
			'' \
			'[logging]' \
			'level = "info"' > "$(ETC_DIR)/arguswing.toml"; \
	else \
		printf '%s\n' \
			'[server]' \
			'bind_addr = "$(SERVER_ADDR_NGINX)"' \
			'' \
			'[database]' \
			'url = "$(DATABASE_URL)"' \
			'' \
			'[trace]' \
			'dir = "$(INSTALL_DIR)/traces"' \
			'' \
			'[crypto]' \
			'master_key_path = "$(ETC_DIR)/master.key"' \
			'' \
			'[auth]' \
			'dev_enabled = false' \
			'' \
			'[auth.oauth]' \
			'enabled = false' \
			'' \
			'[logging]' \
			'level = "info"' > "$(ETC_DIR)/arguswing.toml"; \
	fi
	chown "$(SERVICE_USER):$(SERVICE_GROUP)" "$(ETC_DIR)/arguswing.toml"
	chmod 0640 "$(ETC_DIR)/arguswing.toml"
	sed -e 's#/opt/arguswing#$(INSTALL_DIR)#g' -e 's#/etc/arguswing#$(ETC_DIR)#g' -e 's#User=arguswing#User=$(SERVICE_USER)#g' -e 's#Group=arguswing#Group=$(SERVICE_GROUP)#g' deploy/systemd/arguswing.service > "$(SYSTEMD_DIR)/$(SERVICE_NAME).service"
	@if [ "$(DEPLOY_MODE)" = "nginx" ]; then \
		install -d "$(INSTALL_DIR)/deploy/nginx"; \
		sed -e 's#/opt/arguswing#$(INSTALL_DIR)#g' -e 's#127.0.0.1:3010#$(SERVER_ADDR_NGINX)#g' deploy/nginx/arguswing.conf > "$(INSTALL_DIR)/deploy/nginx/arguswing.conf"; \
		if [ -d "$(NGINX_CONF_DIR)" ]; then \
			cp "$(INSTALL_DIR)/deploy/nginx/arguswing.conf" "$(NGINX_CONF_DIR)/arguswing.conf"; \
			printf 'Installed nginx config to %s/arguswing.conf\n' "$(NGINX_CONF_DIR)"; \
		else \
			printf 'Nginx config staged at %s/deploy/nginx/arguswing.conf\n' "$(INSTALL_DIR)"; \
		fi; \
	fi
	systemctl daemon-reload
	systemctl enable "$(SERVICE_NAME).service"
	systemctl restart "$(SERVICE_NAME).service"

# Aliases for desktop development
desktop-dev: tauri-dev
desktop-build: tauri-build

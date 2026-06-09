SHELL := /bin/bash
.DEFAULT_GOAL := help

.PHONY: help smoke quick vm ssh setup-ssh build-env deploy test validate validate-all provision-edge validate-edge edge-all validate-gui gui-all validate-tray tray-all validate-gui-flows gui-flows-all validate-archive-smoke archive-smoke-all validate-autostart-login autostart-login-all validate-vss-spike vss-spike-all validate-vss down status logs

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-15s\033[0m %s\n", $$1, $$2}'

smoke: ## Full run: boot VM → wait SSH → setup env → deploy → test → teardown
	./run.sh all

quick: ## Re-run tests on already-running VM (skip boot)
	./run.sh quick

vm: ## Start Windows VM only
	./run.sh vm

ssh: ## Wait for SSH to become available
	./run.sh ssh

setup-ssh: ## Install OpenSSH via QEMU monitor keystrokes (first boot)
	./run.sh setup-ssh

build-env: ## Install Rust + MinGW + WebView2 in VM
	./run.sh build-env

deploy: ## Upload source code to VM
	./run.sh deploy

test: ## Run smoke tests against VM
	./run.sh test

validate: ## Run Windows runtime validation (borg/reg/schtasks) on a running VM
	./run.sh validate

validate-all: ## Boot VM → wait SSH → run runtime validation (no toolchain/source needed)
	./run.sh validate-all

provision-edge: ## Ensure standard user borgstd + D: drive on a running VM (idempotent)
	./run.sh provision-edge

validate-edge: ## Run edge validation (multi-drive + non-admin) on a provisioned VM
	./run.sh validate-edge

edge-all: ## Boot VM → wait SSH → provision (borgstd + D:) → edge validation
	./run.sh edge-all

validate-gui: ## Run GUI validation (keychain + scheduled-firing + Tier-C signals) on a running VM
	./run.sh validate-gui

gui-all: ## Boot VM → wait SSH → GUI validation (needs borg-ui.exe + toolchain; see README)
	./run.sh gui-all

validate-tray: ## Run tray-menu validation (#34: right-click menu + Show/Quit via UIA) on a running VM
	./run.sh validate-tray

tray-all: ## Boot VM → wait SSH → tray-menu validation (needs borg-ui.exe + desktop; see README)
	./run.sh tray-all

validate-gui-flows: ## Run interactive GUI-flow validation (tray-nav, settings, restore round-trip, cancel) on a running VM
	./run.sh validate-gui-flows

gui-flows-all: ## Boot VM → wait SSH → GUI-flow validation (needs a PRODUCTION tauri-build exe + desktop; see README)
	./run.sh gui-flows-all

validate-archive-smoke: ## Run the large-archive (#35) GUI smoke (100k-entry stream + virtualization) on a running VM
	./run.sh validate-archive-smoke

archive-smoke-all: ## Boot VM → wait SSH → large-archive (#35) GUI smoke (needs a PRODUCTION tauri-build exe + desktop; see README)
	./run.sh archive-smoke-all

validate-autostart-login: ## Run the autostart login-cycle check (set Run key → reboot → verify) on a running VM
	./run.sh validate-autostart-login

autostart-login-all: ## Boot VM → wait SSH → autostart login-cycle check (reboots the guest; needs a PRODUCTION tauri-build exe)
	./run.sh autostart-login-all

validate-vss-spike: ## Run the VSS feasibility spike (snapshot → junction → clean-path → restore) on a running VM (admin session; no toolchain)
	./run.sh validate-vss-spike

vss-spike-all: ## Boot VM → wait SSH → VSS feasibility spike (de-risks the junction approach before any implementation)
	./run.sh vss-spike-all

validate-vss: ## Run VSS validation (--scheduled-backup + locked file → snapshot) on a running VM (needs a current borg-ui.exe build + desktop)
	./run.sh validate-vss

down: ## Tear down Windows VM
	./run.sh down

status: ## Show VM status
	./run.sh status

logs: ## Show Windows container logs
	docker logs borgui-smoke-win --tail 50

shell: ## SSH into the Windows VM
	sshpass -p '$${SSH_PASS:-Password1!}' ssh -o StrictHostKeyChecking=no -p $${SSH_PORT:-2222} $${SSH_USER:-borgtest}@localhost

keep: ## Full run but keep VM alive after
	KEEP_VM=1 ./run.sh all

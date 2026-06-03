SHELL := /bin/bash
.DEFAULT_GOAL := help

.PHONY: help smoke quick vm ssh setup-ssh build-env deploy test validate validate-all provision-edge validate-edge edge-all down status logs

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

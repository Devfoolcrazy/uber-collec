# Uber Collec — raccourcis de développement et de build.
#
# Le PATH est reconstruit ici car le lazy-loading nvm du shell ne fonctionne
# pas hors session interactive (node/npm/npx sont des fonctions cassées).

NODE_BIN := $(HOME)/.nvm/versions/node/v24.13.0/bin
export PATH := $(NODE_BIN):$(HOME)/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$(PATH)

APP_BUNDLE := src-tauri/target/release/bundle/macos/Uber Collec.app

.PHONY: help dev build install test front ios ios-init doctor clean

help: ## Liste les commandes disponibles
	@grep -E '^[a-z-]+:.*##' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  make %-10s %s\n", $$1, $$2}'

node_modules: package.json
	npm install
	@touch node_modules

dev: node_modules ## Lance l'app desktop en mode développement (Ctrl-C pour quitter)
	npm run tauri dev

build: node_modules ## Compile l'app desktop optimisée (.app dans src-tauri/target/release/bundle)
	npm run tauri build

install: build ## Compile puis installe Uber Collec.app dans /Applications
	rm -rf "/Applications/Uber Collec.app"
	cp -R "$(APP_BUNDLE)" /Applications/
	@echo "✓ Installé : /Applications/Uber Collec.app"

test: ## Tests backend Rust + typecheck du front
	cd src-tauri && cargo test
	npm run build

front: node_modules ## Typecheck + build du front uniquement
	npm run build

ios: node_modules ## Lance l'app dans le simulateur iPhone
	npm run tauri ios dev "iPhone 17"

ios-init: node_modules ## (Re)génère le projet Xcode iOS
	npm run tauri ios init

doctor: ## Vérifie l'outillage (node, cargo, xcode, gh)
	@node --version && cargo --version && xcodebuild -version | head -1 && gh auth status 2>&1 | head -2

clean: ## Supprime les artefacts de build
	cd src-tauri && cargo clean
	rm -rf dist

.PHONY: build web install dev test clean

# Build the web UI, then the single binary with the UI embedded.
build: web
	cargo build --release

web:
	npm --prefix web ci --silent || npm --prefix web install --silent
	npm --prefix web run build

# Install `bankan` into ~/.cargo/bin (make sure it's on your PATH).
install: web
	cargo install --path . --locked --force

# Run the daemon and the Vite dev server with hot reload (UI at http://localhost:5173).
dev:
	cargo run -- serve & npm --prefix web run dev

test:
	cargo test
	npm --prefix web run typecheck

clean:
	cargo clean
	rm -rf web/dist web/node_modules

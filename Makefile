all: build

http:
	cd frontend && pnpm run build \
        && cargo build --features http --release

build:
	cargo build --release


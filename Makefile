all: build

http:
	cd frontend && pnpm i && pnpm run build \
        && cargo build --features http --release

build:
	cargo build --release


musl:
	cargo build --release --target=x86_64-unknown-linux-musl

http-musl:
	cd frontend && pnpm i && pnpm run build \
        && cargo build --features http --release --target=x86_64-unknown-linux-musl

clean:
	cargo clean

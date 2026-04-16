# ─── Stage 1: cargo-chef base ────────────────────────────────────────────────
FROM rust:1.88-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

# ─── Stage 2: planner ────────────────────────────────────────────────────────
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ─── Stage 3: build frontend (WASM) ──────────────────────────────────────────
FROM chef AS frontend-builder

# Install Node.js (for Tailwind CSS build)
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y nodejs

# Install trunk and wasm-bindgen-cli
RUN rustup target add wasm32-unknown-unknown \
    && cargo install trunk --locked \
    && cargo install wasm-bindgen-cli --version 0.2.114 --locked

# Cache Rust dependencies (frontend + shared only)
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --target wasm32-unknown-unknown --recipe-path recipe.json || true

# Copy source and build
COPY . .
RUN npm --prefix frontend ci
RUN trunk build --config frontend/Trunk.toml --release --dist frontend/dist
RUN tar -cf /frontend.bundle.tar -C frontend/dist .

# ─── Stage 4: build server ────────────────────────────────────────────────────
FROM chef AS server-builder

# Cache Rust dependencies (server + all native crates)
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Copy source, inject frontend bundle, build server
COPY . .
COPY --from=frontend-builder /frontend.bundle.tar /tmp/frontend.bundle.tar
RUN DEVICE_HUB_FRONTEND_BUNDLE=/tmp/frontend.bundle.tar \
    cargo build --release -p device-hub-server

# ─── Stage 5: runtime image ───────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=server-builder /app/target/release/device-hub-server /usr/local/bin/device-hub-server

EXPOSE 3000
ENTRYPOINT ["device-hub-server"]

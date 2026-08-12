# Build the TPT Lattice frontend (with the wasm engine) in a reproducible container.
# Usage:
#   docker build -t tpt-lattice-frontend -f Dockerfile .
#   docker run --rm -p 8080:80 tpt-lattice-frontend
# Then open http://localhost:8080

FROM rust:1.84-slim AS wasm-builder
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
RUN rustup target add wasm32-unknown-unknown \
    && cargo install wasm-pack --locked
WORKDIR /src
COPY . .
RUN cd crates/tpt-lattice-wasm && wasm-pack build --target web --out-dir pkg

FROM node:20-slim AS frontend-builder
WORKDIR /app
COPY frontend/package.json frontend/package-lock.json* ./
RUN npm install
COPY frontend/ ./
# Bring in the wasm package built above.
COPY --from=wasm-builder /src/crates/tpt-lattice-wasm/pkg ./crates/tpt-lattice-wasm/pkg
RUN npm run build

FROM nginx:alpine
COPY --from=frontend-builder /app/dist /usr/share/nginx/html
EXPOSE 80

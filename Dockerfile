# --- Build stage ---
FROM rust:1.86-slim AS builder

# Move files
WORKDIR /app
COPY ./Cargo.toml ./Cargo.lock ./
COPY ./src ./src

# Set up for target
ARG target=x86_64-unknown-linux-musl
RUN apt-get update && apt-get install -y musl-tools
RUN rustup target add $target

# Build
RUN cargo build --target=$target --release
RUN cp /app/target/$target/release/veloxid /app/veloxid

# --- Final stage ---
FROM scratch
COPY --from=builder /app/veloxid /veloxid
ENTRYPOINT ["/veloxid"]

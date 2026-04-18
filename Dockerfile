FROM rust:1-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/egras /usr/local/bin/egras
ENV EGRAS_BIND_ADDRESS=0.0.0.0:8080
EXPOSE 8080
USER 1000:1000
ENTRYPOINT ["egras"]
CMD ["serve"]

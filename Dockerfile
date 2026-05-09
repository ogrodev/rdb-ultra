# syntax=docker/dockerfile:1

FROM --platform=$BUILDPLATFORM rust:1.85-slim-bookworm AS builder
WORKDIR /src

COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked


FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN useradd --system --uid 10001 --home-dir /app rinha
COPY --from=builder /src/target/release/rinha-api /usr/local/bin/rinha-api
COPY model/hour /app/model/hour

ENV API_ADDR=0.0.0.0:8080
ENV SUPPORT_INDEX_PATH=/app/model/hour

USER rinha
EXPOSE 8080
CMD ["/usr/local/bin/rinha-api"]

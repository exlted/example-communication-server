FROM rust:latest AS builder

COPY . .

RUN cargo install --path .

# second stage.
FROM debian:bookworm-slim
COPY --from=builder /usr/local/cargo/bin/* /

EXPOSE 8080
CMD ["/server"]
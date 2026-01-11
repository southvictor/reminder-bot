## Build stage: compile reminderBot as a static musl binary
FROM messense/rust-musl-cross:x86_64-musl AS builder

WORKDIR /workspace

# NOTE: This Dockerfile assumes the build context is the parent
# directory that contains both `reminderBot/` and `memory_db/`.
# Example:
#   docker build -f reminderBot/Dockerfile -t reminderbot ..

COPY reminderBot ./reminderBot
COPY memory_db ./memory_db

WORKDIR /workspace/reminderBot

# Build for musl target
RUN cargo build --release --target x86_64-unknown-linux-musl

## Runtime stage: minimal image with the compiled binary
FROM alpine:3.20

RUN apk add --no-cache ca-certificates tzdata

WORKDIR /app

COPY --from=builder /workspace/reminderBot/target/x86_64-unknown-linux-musl/release/reminderBot /usr/local/bin/reminderBot

# Default command just prints help; override as needed.
CMD ["reminderBot", "--help"]

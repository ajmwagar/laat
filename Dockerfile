# Build
FROM clux/muslrust:latest as cargo-build 
WORKDIR /usr/src/

# Setup dummy project to seperate build into 2-steps
RUN rustup component add rustfmt
RUN USER=root cargo new --bin laat
WORKDIR /usr/src/laat

# Copy over dependencies and build script
COPY ./Cargo.toml ./Cargo.toml 
COPY ./Cargo.lock ./Cargo.lock 

COPY ./armake2 ./armake2

# Build without binary's source code
RUN cargo build --release 

# Cleanup resulting binary artifacts
RUN rm -f target/x86_64-unknown-linux-musl/release/deps/laat* 
RUN rm src/*.rs 

# Build binary for real
COPY ./src ./src 
COPY ./templates ./templates

RUN cargo build --release 

# Run in bare-metal environment
FROM steamcmd/steamcmd:latest

WORKDIR /

COPY --from=cargo-build /usr/src/laat/target/x86_64-unknown-linux-musl/release/laat /usr/bin/

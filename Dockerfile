# VOIDWRAP RUN - the whole game (server + web client) as one container.
# Used for free hosting (render.yaml). Build locally with:  docker build -t voidwrap-run .

FROM rust:1-bookworm AS rust
RUN rustup target add wasm32-unknown-unknown
WORKDIR /src
COPY server server
COPY protocol protocol
COPY design design
RUN cd server \
 && cargo build --release -p server \
 && cargo build --release -p sim-wasm --target wasm32-unknown-unknown

FROM node:26-bookworm-slim AS web
WORKDIR /src
RUN npm install -g pnpm@12.18.0
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY web/package.json web/
RUN pnpm install --frozen-lockfile
COPY web web
COPY design design
COPY --from=rust /src/server/target/wasm32-unknown-unknown/release/sim_wasm.wasm web/public/sim.wasm
RUN NODE_ENV=production pnpm --dir web build

FROM debian:bookworm-slim
RUN useradd --create-home --uid 10001 voidwrap
WORKDIR /app
COPY --from=rust /src/server/target/release/voidwrap-server /app/voidwrap-server
COPY --from=web /src/web/out /app/web
ENV VOIDWRAP_WEB_DIR=/app/web \
    VOIDWRAP_HOST=0.0.0.0
USER voidwrap
EXPOSE 7878
CMD ["/app/voidwrap-server"]

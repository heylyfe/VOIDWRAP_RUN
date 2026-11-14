# VOIDWRAP MATCHMAKER - the lobby registry service (render.yaml: voidwrap-matchmaker).
# Build from the repository root:
#   docker build -f server/crates/matchmaker/Dockerfile -t voidwrap-matchmaker .

FROM rust:1-bookworm AS build
WORKDIR /src
# The whole workspace is needed for cargo to resolve members; `protocol` is copied too
# because the workspace crates reference it from the repository root.
COPY server server
COPY protocol protocol
RUN cd server && cargo build --release -p matchmaker

FROM debian:bookworm-slim
RUN useradd --create-home --uid 10001 matchmaker
WORKDIR /app
COPY --from=build /src/server/target/release/matchmaker /app/matchmaker
ENV PORT=8080
USER matchmaker
EXPOSE 8080
CMD ["/app/matchmaker"]

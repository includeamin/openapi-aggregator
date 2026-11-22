# Runtime image for openapi-aggregator. It only packages a prebuilt static (musl) binary,
# laid out by the release workflow as dist/linux/<amd64|arm64>/openapi-aggregator.
FROM scratch
ARG TARGETARCH
COPY dist/linux/${TARGETARCH}/openapi-aggregator /openapi-aggregator
LABEL org.opencontainers.image.title="openapi-aggregator" \
      org.opencontainers.image.description="Aggregate and merge OpenAPI specifications from multiple sources" \
      org.opencontainers.image.source="https://github.com/includeamin/openapi-aggregator" \
      org.opencontainers.image.licenses="MIT"
USER 65532:65532
WORKDIR /work
ENTRYPOINT ["/openapi-aggregator"]

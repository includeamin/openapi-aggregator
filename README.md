# openapi-aggregator

Aggregate and merge OpenAPI 3.x specifications from multiple sources into a single spec.

Available as both a **Rust library** and a **CLI tool**.

## Features

- **Multiple source types** – local YAML files, local JSON files, and HTTP endpoints (with custom headers)
- **Config-file driven** – define sources and merge options in a single YAML config
- **Conflict resolution** – choose between `error`, `overwrite`, or `rename` strategies for duplicate paths and component names
- **`$ref` rewriting** – when using the `rename` strategy, `$ref` pointers are automatically updated
- **Path prefixing** – optionally prefix every path with the source name to guarantee uniqueness
- **Info override** – set a custom `title`, `version`, and `description` in the merged output
- **Per-source custom blocks** – deep-merge arbitrary OpenAPI blocks/extensions from config (provider-agnostic)

## Installation

### Quick install (Linux / macOS)

```sh
curl -sSfL https://raw.githubusercontent.com/includeamin/openapi-aggregator/main/install.sh | sh
```

Install a specific version or to a custom directory:

```sh
VERSION=v0.1.0 curl -sSfL https://raw.githubusercontent.com/includeamin/openapi-aggregator/main/install.sh | sh
INSTALL_DIR=/usr/local/bin curl -sSfL https://raw.githubusercontent.com/includeamin/openapi-aggregator/main/install.sh | sh
```

Uninstall:

```sh
curl -sSfL https://raw.githubusercontent.com/includeamin/openapi-aggregator/main/install.sh | sh -s -- --uninstall
```

### From source

```sh
cargo install --path crates/openapi-aggregator
```

### Pre-built binaries

Download from [GitHub Releases](../../releases). Binaries are available for Linux (x86_64, aarch64), macOS (x86_64, aarch64), and Windows (x86_64).

## CLI Usage

```sh
# Merge using a config file (defaults to openapi-aggregator.yaml)
openapi-aggregator

# Specify a config file and output location
openapi-aggregator -c my-config.yaml -o merged.yaml

# Output as JSON
openapi-aggregator -c my-config.yaml -f json

# Print help
openapi-aggregator --help
```

## Configuration

Create an `openapi-aggregator.yaml` (see [config.example.yaml](config.example.yaml)):

```yaml
sources:
  - name: petstore
    path: ./specs/petstore.yaml
    additional_blocks:
      x-custom-root:
        enabled: true
      paths:
        /pets:
          get:
            x-custom-operation:
              rate_limit: 100

  - name: users
    path: ./specs/users.json

  - name: billing
    url: https://billing.example.com/openapi.json
    headers:
      Authorization: "Bearer token"

output:
  format: yaml  # yaml | json

merge:
  conflict_strategy: error  # error | overwrite | rename
  prefix_paths: false
  info:
    title: "My Aggregated API"
    version: "1.0.0"
```

### Source detection

Sources are detected automatically by their fields:

- If `url` is present → HTTP source
- If `path` is present → file source (YAML or JSON auto-detected from content)

Exactly one of `url` / `path` must be set, and unknown keys anywhere in the config are rejected, so typos like `conflict_stratgy` fail loudly instead of being ignored.

An HTTP source without a `name` is named after its URL host (e.g. `billing.example.com`).

### Environment variables

`${VAR}` placeholders in an HTTP source's `url` and `headers` values are replaced with environment variables at load time, so secrets don't have to live in the config file:

```yaml
  - name: billing
    url: https://billing.example.com/openapi.json
    headers:
      Authorization: "Bearer ${BILLING_TOKEN}"
```

A referenced variable that is not set is an error.

### Additional source blocks

Each source can define `additional_blocks` as any YAML/JSON object. It is deep-merged into that source document before merge, so you can inject vendor extensions (for example API gateway related `x-...` blocks) or other custom OpenAPI fragments without adding provider-specific fields.

## Library Usage

```rust
use openapi_aggregator::{aggregate, Config, Source, MergeConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config {
        sources: vec![
            Source::File {
                name: Some("petstore".into()),
                path: "./specs/petstore.yaml".into(),
              additional_blocks: None,
              tag_prefix: None,
            },
            Source::Http {
                name: Some("billing".into()),
                url: "https://billing.example.com/openapi.json".into(),
                headers: [("Authorization".into(), "Bearer token".into())]
                    .into_iter()
                    .collect(),
              additional_blocks: None,
              tag_prefix: None,
            },
        ],
        output: Default::default(),
        merge: MergeConfig::default(),
    };

    let merged = aggregate(&config).await?;
    println!("{}", serde_json::to_string_pretty(&merged)?);
    Ok(())
}
```

Or load directly from a config file:

```rust
use openapi_aggregator::aggregate_from_file;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let merged = aggregate_from_file(Path::new("openapi-aggregator.yaml")).await?;
    println!("{}", serde_yaml::to_string(&merged)?);
    Ok(())
}
```

## Merge Behaviour

Only real conflicts trigger the conflict strategy:

- **Paths are merged per operation.** `GET /users` from one source and `POST /users` from another end up under the same path. A conflict is the same operation (or path-level field) defined differently.
- **Identical definitions are shared.** Two sources defining the same `bearerAuth` scheme or `Error` schema produce one copy, not a conflict.

| Strategy    | Conflicting paths / webhooks             | Conflicting components                   |
|-------------|------------------------------------------|------------------------------------------|
| `error`     | Fail immediately                         | Fail immediately                         |
| `overwrite` | Last source wins (per operation)         | Last source wins                         |
| `rename`    | Move the source's path to `/{source_name}{path}` | Rename to `{source_name}_{component}` |

When `prefix_paths: true`, **all** paths are prefixed regardless of conflicts.

When using `rename`, any `$ref` pointing to a renamed component is rewritten automatically, and renamed names never overwrite an existing one (`b_2_Pet` is used if `b_Pet` is taken). Source names are sanitised (`my api` → `my_api`) wherever they become part of a path or component name.

Other top-level fields:

- `webhooks` (OpenAPI 3.1) are merged like paths.
- Top-level `security` requirements are combined and de-duplicated.
- The merged `openapi` version comes from the first source. The CLI prints a warning when sources use different `major.minor` versions (also available via `aggregate_with_report`).
- Component types are always emitted in the same order, so the output is stable across runs.

## Development

```sh
# Run tests
cargo test

# Lint
cargo clippy --all-targets -- -D warnings

# Format
cargo fmt
```

## License

[MIT](LICENSE)

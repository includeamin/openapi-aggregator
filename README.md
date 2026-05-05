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
cargo install --path .
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

| Strategy    | Duplicate paths                        | Duplicate components                     |
|-------------|----------------------------------------|------------------------------------------|
| `error`     | Fail immediately                       | Fail immediately                         |
| `overwrite` | Last source wins                       | Last source wins                         |
| `rename`    | Prefix path with `/{source_name}`      | Rename to `{source_name}_{component}`    |

When `prefix_paths: true`, **all** paths are prefixed regardless of conflicts.

When using `rename`, any `$ref` pointing to a renamed component is rewritten automatically.

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

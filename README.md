# openapi-aggregator

Aggregate and merge OpenAPI 3.x (and Swagger 2.0) specifications from multiple sources into a single spec.

Available as both a **Rust library** and a **CLI tool**.

**Try it in your browser:** the [playground](https://includeamin.github.io/openapi-aggregator/) runs the same merge engine compiled to WebAssembly. Add specs, edit the config, and share the result as a link.

## Features

- **Swagger 2.0 support** – Swagger 2.0 sources are converted to OpenAPI 3.0.3 automatically; anything that had to be approximated is reported as a warning
- **External `$ref`s** – specs split across files or URLs are bundled per source before merging (CLI sandboxed to the config directory)
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

### External `$ref`s

A source may point at other files or URLs (`$ref: ./schemas/pet.yaml#/Pet`, `$ref: https://example.com/defs.yaml#/Error`). Before merging, each source is **bundled**: every external target is copied into that source's own `components` and the `$ref` is rewritten to `#/components/...`. Circular references work, and everything downstream (conflict strategies, renaming) sees a normal self-contained spec. Specs without external refs are unchanged.

- **Where targets land:** a pointer like `#/components/schemas/Pet` keeps its type and name; `#/definitions/Pet` and `#/$defs/Pet` become schemas; otherwise the type comes from where the `$ref` sits (a `parameters` entry becomes a parameter, a response becomes a response, ...) and the name is the last pointer segment (or the file name for a whole-file ref). If the name is taken, the file name is added (`pet_Pet`) and a warning says so. A path item that is a `$ref` is inlined.
- **Relative refs** resolve against the referencing file or URL.
- **Sandbox (CLI and library):** external file refs may only read inside the config directory and the directory of each file source. Anything else, including absolute paths and symlinks pointing elsewhere, is an error. Pass `--allow-outside-config-dir` (or `AggregateOptions { allow_outside_config_dir: true, .. }` with `aggregate_with_options`) to lift it.
- **URLs:** a URL source's `headers` are only sent to that URL's origin; refs to other hosts are fetched without them. A document fetched over HTTP can never cause a local file read, and `file:` URLs are not supported.
- **Limit:** at most 100 external documents per source.
- **Playground:** referenced files are looked up in the workspace by name, so add each one under exactly the name the `$ref` resolves to (the error message tells you which). Remote refs follow the same "Allow fetching" rule as remote sources.

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
- Top-level `security` stays top-level only when every source declares the same requirements. Otherwise each source's requirements are copied onto its own operations, so no operation gains or loses authentication. Renamed security schemes and `discriminator.mapping` entries are updated along with `$ref`s.
- Swagger 2.0 sources are converted to OpenAPI 3.0.3 before merging (`host`/`basePath`/`schemes` become `servers`, `in: body` and `in: formData` parameters become `requestBody`, `definitions` move to `components`, and so on). Conversion is lenient and each approximation or dropped field is reported as a warning prefixed with the source name. Swagger 1.x is not supported.
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

### Web playground

```sh
cd web
npm ci
npm run wasm     # build the WASM engine (needs wasm-pack + wasm32-unknown-unknown target)
npm run dev      # http://localhost:5173/openapi-aggregator/
npm test         # unit tests
npm run e2e      # Playwright smoke test
```

## License

[MIT](LICENSE)

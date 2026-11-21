# openapi-aggregator

Aggregate and merge OpenAPI 3.x (and Swagger 2.0) specifications from multiple sources into a single spec.

Available as both a **Rust library** and a **CLI tool**.

**Try it in your browser:** the [playground](https://includeamin.github.io/openapi-aggregator/) runs the same merge engine compiled to WebAssembly. Add specs, edit the config, and share the result as a link.

## Features

- **Swagger 2.0 support** – Swagger 2.0 sources are converted to OpenAPI 3.0.3 automatically; anything that had to be approximated is reported as a warning
- **External `$ref`s** – specs split across files or URLs are bundled per source before merging (CLI sandboxed to the config directory)
- **Validation and CI check** – `--check` validates the merged spec and fails the build when it is invalid or when a committed output file is stale
- **Operation filters** – hide internal endpoints (by path glob, tag, `operationId` or an `x-` flag), globally or per source, with unused components and tags pruned
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

# Validate the merged spec and verify that merged.yaml is up to date (writes nothing)
openapi-aggregator -c my-config.yaml -o merged.yaml --check

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

## Validation and CI

`--check` merges the sources, validates the result, and exits non-zero on problems, so it can gate a CI job. It never writes anything and prints nothing to stdout; findings go to stderr.

```sh
openapi-aggregator --check -c openapi-aggregator.yaml -o openapi.yaml
```

- **Validation** always runs. It checks what an aggregation can break (it is not a full OpenAPI schema validation).
- **Output comparison** runs when `-o` is given: the file is parsed and compared with the freshly merged spec *as data*, so formatting, YAML vs JSON and key order never count as a difference. On a mismatch a unified diff is printed. A missing or unparseable file counts as out of date.
- **`--deny-warnings`** also fails on warnings (merge warnings, Swagger 2.0 conversion notes and validation warnings).

| Exit code | Meaning |
|-----------|---------|
| `0` | Passed |
| `1` | Validation errors, denied warnings, or the merge/config itself failed (if the file is also out of date, the diff is still printed) |
| `2` | The output file is missing or out of date. Note that command-line usage errors also exit `2`. |

Findings look like `error[dangling-ref] /paths/~1pets/get/...: $ref '#/components/schemas/Ghost' does not resolve`, where the location is a JSON pointer into the merged spec.

| Code | Severity | Meaning |
|------|----------|---------|
| `invalid-openapi-version` | error | `openapi` is missing or not a 3.x version |
| `missing-field` | error | `info`, `info.title`, `info.version` missing; `paths` missing (3.0), or none of `paths`/`webhooks`/`components` (3.1+) |
| `dangling-ref` | error | an internal `$ref` that does not resolve |
| `external-ref` | error | a `$ref` outside the document that was not bundled |
| `duplicate-operation-id` | error | the same `operationId` on two operations |
| `missing-responses` | error | an operation without `responses` |
| `undeclared-path-param` | error | `{name}` in a path with no matching `in: path` parameter |
| `unused-path-param` | warning | a path parameter that is not in the path |
| `duplicate-parameter` | error | the same `name` and `in` declared twice |
| `ambiguous-paths` | error | two paths that differ only in placeholder names |
| `unknown-security-scheme` | error | a `security` requirement naming an undefined scheme |
| `undeclared-tag` | warning | an operation tag missing from the top-level `tags` (only checked when `tags` is non-empty) |
| `invalid-component-name` | error | a component name outside `[a-zA-Z0-9._-]+` |

Example GitHub Actions step that keeps a committed spec in sync:

```yaml
- name: Check the aggregated spec
  run: openapi-aggregator --check --deny-warnings -c openapi-aggregator.yaml -o openapi.yaml
```

The same validator is available as `openapi_aggregator::validate(&spec)`, and the playground shows its findings in the problems panel.

## Hiding internal endpoints

Build a public spec from sources that also describe internal endpoints by filtering operations out. Filters run **per source, before the merge**, so a hidden operation can never cause a path or component conflict (two services can both expose `/internal/metrics`).

```yaml
merge:
  filter:                        # applies to every source
    exclude:
      paths: ["/internal/**", "/admin/*"]
      tags: [internal]
      operation_ids: [debugDump]
      extensions: [x-internal]   # truthy on the operation or its path item
sources:
  - name: billing
    path: billing.yaml
    filter:                      # this source only
      include:
        tags: [public]           # allow-list: keep only these operations
```

- **Rules:** an operation matches a rule if *any* of its criteria match (`paths`, `tags`, `operation_ids`, `extensions`). Nothing is hidden by default; list `x-internal` explicitly if you use it.
- **Combining:** `exclude` rules from the global and source level all apply, and `exclude` wins over `include`. `include` is an allow-list; when both levels define one, an operation must satisfy both.
- **Globs:** `*` matches within one path segment (`/admin/*` does not match `/admin/a/b`), `**` matches across segments (`/internal/**`). Everything else, including `{id}`, is literal. Globs apply to `paths` only, not `webhooks`.
- **Cleanup:** a path left with no operations is removed. Components (schemas, parameters, responses, ...), security schemes and tags that were used before filtering and are unused afterwards are pruned. Things that were never referenced to begin with are left alone.
- **Not scrubbed:** the filter does not look inside descriptions, examples, `servers`, `info` or vendor extensions, so check those for internal details yourself. External documents referenced only by hidden operations are still loaded.

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
                filter: None,
                tag_prefix: None,
            },
            Source::Http {
                name: Some("billing".into()),
                url: "https://billing.example.com/openapi.json".into(),
                headers: [("Authorization".into(), "Bearer token".into())]
                    .into_iter()
                    .collect(),
                additional_blocks: None,
                filter: None,
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

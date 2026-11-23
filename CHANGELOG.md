# Changelog

All notable changes to this project will be documented in this file.

## [0.16.1] - 2026-11-23

### Miscellaneous

- *(deps)* Bump the rust-minor-and-patch group with 3 updates

## [0.16.0] - 2026-11-23

### Documentation

- Document per-source path prefixes

### Features

- *(core)* Add a per-source path_prefix to the configuration
- *(core)* Merge sources with a per-source path prefix
- Apply per-source path prefixes when aggregating
- *(web)* Add a path prefixes example

## [0.15.3] - 2026-11-22

### Bug Fixes

- *(ci)* Run the crates.io publish script from the workflow's own commit

## [0.15.2] - 2026-11-22

### CI

- Add a tested decision script for feature releases
- *(release)* Expose the bump type and a publish decision
- Add an idempotent crates.io publish script
- *(release)* Publish feature releases to crates.io

### Documentation

- Document crates.io publishing and the release policy

## [0.15.1] - 2026-11-22

### Miscellaneous

- Configure dependabot version updates

## [0.15.0] - 2026-11-22

### Documentation

- Document glob sources

### Features

- *(core)* Add glob path helpers and validate glob sources
- *(core)* Expand glob sources into one source per file
- Expand glob sources from the file system
- *(web)* Expand glob sources from workspace files

## [0.14.3] - 2026-11-22

### Documentation

- Point the Action examples at v0.14.2

## [0.14.2] - 2026-11-22

### Bug Fixes

- *(action)* Escape annotation data portably on bash 3.2
- *(action)* Exit 0 after a successful install
- *(action)* Verify checksums when sha256sum prefixes the hash with a backslash

### CI

- Exercise the Action's real install path on Linux, macOS and Windows

### Documentation

- Point the Action examples at v0.14.1 and note the v0.14.0 installer bug

## [0.14.0] - 2026-11-22

### CI

- Publish checksums and a multi-arch Docker image on release

### Documentation

- Document the GitHub Action and the Docker image

### Features

- *(action)* Add a checksum-verifying installer script
- *(action)* Add the check runner script
- *(action)* Add the composite GitHub Action and its CI self-test

## [0.13.0] - 2026-11-21

### Documentation

- Document operation filtering

### Features

- *(core)* Add filter configuration
- *(core)* Add glob and filter rule matching
- *(core)* Remove operations matched by filter rules
- *(core)* Prune components, schemes and tags orphaned by filtering
- Apply operation filters before merging
- *(web)* Add a "hide internal endpoints" example

## [0.12.0] - 2026-11-21

### Bug Fixes

- *(core)* Skip path-parameter checks for path items that are refs

### Documentation

- Document validation and check mode

### Features

- *(core)* Add a semantic validator for merged specs
- *(core)* Validate that every $ref resolves
- *(core)* Validate operations, path parameters and ambiguous paths
- *(core)* Validate security requirements and tags
- Report validation findings in the playground
- *(cli)* Add --check and --deny-warnings
- *(cli)* Verify the output file is up to date in check mode

## [0.11.0] - 2026-11-21

### Bug Fixes

- *(core)* Keep absolute file bases absolute when resolving refs

### Documentation

- Document external $ref resolution

### Features

- *(core)* Add external ref location helpers
- *(core)* Find documents referenced by external refs
- *(core)* Bundle external refs into components
- *(core)* Bundle_source with a per-source document cap
- Resolve external refs in the CLI and library with a directory sandbox
- *(wasm)* Add pendingRefs and external docs to aggregate
- *(web)* Resolve external refs from workspace files and add a multi-file example

## [0.10.0] - 2026-11-21

### Bug Fixes

- *(core)* Satisfy clippy in swagger2 converter

### Features

- *(core)* Convert Swagger 2.0 servers, definitions and refs to OpenAPI 3.0
- *(core)* Convert Swagger 2.0 parameters and request bodies
- *(core)* Convert Swagger 2.0 responses and security definitions
- Accept Swagger 2.0 sources and report conversion notes
- *(web)* Add Swagger 2.0 playground example and document conversion

### Testing

- *(core)* Cover Swagger Petstore 2.0 conversion

## [0.9.0] - 2026-10-17

### Features

- Prepare crates for publishing to crates.io

## [0.8.0] - 2026-10-01

### Features

- *(web)* Redesign playground as a three-pane workbench
- *(web)* Add SEO metadata, social preview image and sitemap

## [0.7.0] - 2026-10-01

### Bug Fixes

- Merge correctness, strict config and CLI output format
- *(merge)* Keep per-source security semantics and rewrite scheme and discriminator renames
- *(web)* Gate remote fetches from shared configs and harden workspace edits

### CI

- *(release)* Commit Cargo.lock and pass release notes via env
- Build, test and deploy the web playground to GitHub Pages

### Features

- *(wasm)* Add wasm-bindgen wrapper crate
- *(web)* Scaffold playground with wasm engine wrapper
- *(web)* Resolve workspace and URL sources with CORS-aware errors
- *(web)* Compressed share links in the URL hash
- *(web)* Add playground examples
- *(web)* Merge pipeline that turns failures into problems
- *(web)* Playground UI with editors, examples, share links and API reference

### Miscellaneous

- Sync Cargo.lock crate versions to 0.6.1

### Refactor

- Move crate into a cargo workspace
- Extract pure openapi-aggregator-core crate

## [0.6.1] - 2026-09-09

### Miscellaneous

- *(http)* Improve http request handling
- *(lint)* Fix formatting

## [0.6.0] - 2026-07-13

### Features

- Add spec processor

## [0.5.2] - 2026-07-13

### Miscellaneous

- Update message of install.sh

## [0.5.1] - 2026-07-13

### Miscellaneous

- Update deps and fix install script

## [0.5.0] - 2026-05-05

### Features

- Support custom blocks

## [0.4.0] - 2026-05-05

### Features

- Support servers tags in the merged openapi spec

## [0.3.0] - 2026-05-04

### Features

- Forward tags from original sources with optional prefix

## [0.2.3] - 2026-05-04

### Bug Fixes

- *(script)* Update install.sh to avoid sudo permission

## [0.2.2] - 2026-05-04

### Bug Fixes

- *(readme)* Update README

## [0.2.1] - 2026-05-04

### Bug Fixes

- *(action)* Fix release workflow

### Miscellaneous

- Add uninstall option to the install script
- Fix release workflow

## [0.2.0] - 2026-05-04

### Documentation

- Add blank line for better readability in source detection section

### Features

- Add OpenAPI aggregator library and CLI tool
- Add tag and release workflow with version bumping and changelog generation
- *(docs)* Add a command to simply install the binary



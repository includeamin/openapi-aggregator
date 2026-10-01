# Changelog

All notable changes to this project will be documented in this file.

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



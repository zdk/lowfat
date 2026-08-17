# Contributing

Thanks for your interest in lowfat.

## Build & test

```sh
cargo build
cargo test --workspace
```

To verify Linux from a Mac, run the suite in Docker (CI runs both):

```sh
docker run --rm -v "$PWD":/work -w /work -e CARGO_TARGET_DIR=/tmp/target rust:1 cargo test --workspace
```

The workspace has five crates: `lowfat-core`, `lowfat-compress`, `lowfat-plugin`, `lowfat-runner`, and the `lowfat` CLI.

## Pull requests

- Keep PRs small and focused on one change.
- Add a test for new behaviour; unit tests live alongside the code.

## Writing a plugin

See [docs/PLUGINS.md](docs/PLUGINS.md). Bundled plugins live under
`crates/lowfat-plugin/embedded/` (they ship in the binary); community plugins
live under `plugins/`.

## Releases (maintainers)

Bump `version` in `Cargo.toml`, tag `vX.Y.Z`, push — the release workflow handles the rest.

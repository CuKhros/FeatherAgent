# Contributing

Thanks for contributing to FeatherAgent. The project targets **small-model context engineering plus a very lightweight, portable kernel**. Please keep kernel size, startup, and RPC gates from regressing.

## Development environment

- Rust 1.80+, Linux x64 or Windows x64.
- Cross-compiling the Windows package on Linux requires `mingw-w64` and the `x86_64-pc-windows-gnu` target.
- Ollama is optional for real-model integration; `provider-mock` covers the offline tool loop.

## Local checks

```bash
cargo fmt --check
cargo test --all-targets
scripts/version-check.sh
scripts/build-linux.sh
```

Windows cross-build and packaging:

```bash
scripts/build-windows.sh
scripts/package-windows.sh --skip-build
```

One `cargo test` case needs public network and is marked ignored. Run it separately before release:

```bash
cargo test --test integration local_web_search_tool_via_kernel -- --ignored
```

## Commit conventions

- One topic per commit; explain **symptom → root cause → fix → verification**.
- Never commit `target/`, `dist/`, `runtime/`, `data/`, `workspace/`, logs, or any secret.
- When changing the version, update `Cargo.toml` and all `plugins/*/plugin.json`, then run `scripts/version-check.sh`.
- When changing Windows resources or icons, confirm all five exes contain `.rsrc` and do not depend on MinGW runtime DLLs.
- New platform-specific code must pass both Linux and Windows cross-compilation.

## Branches and review

`main` is maintained by the repository owner. External contributors should fork the repository, create a branch, and open a Pull Request. While branch protection is unavailable on a private repository, the owner applies the same rules manually: at least one review, CI green, and all conversations resolved before merge.

## License

By submitting a contribution you agree to license it under the [MIT License](LICENSE).

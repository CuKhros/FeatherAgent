## What changed

<!-- symptom / root cause / fix / verification -->

## Checklist

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets --locked -- -D warnings`
- [ ] `cargo test --locked --all-targets`
- [ ] `scripts/version-check.sh`
- [ ] Windows-related: `scripts/build-windows.sh`; all five exes have `.rsrc` and do not depend on MinGW runtime DLLs
- [ ] No secrets, personal paths, `dist/`, `target/`, `runtime/`, or logs committed

## Related issue

<!-- "None" if not applicable -->

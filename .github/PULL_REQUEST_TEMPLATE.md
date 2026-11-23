## 改动说明

<!-- 现象 / 根因 / 修复 / 验证 -->

## 检查项

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets --locked -- -D warnings`
- [ ] `cargo test --locked --all-targets`
- [ ] `scripts/version-check.sh`
- [ ] 涉及 Windows 时：`scripts/build-windows.sh`，5 个 exe 均有 `.rsrc` 且不依赖 MinGW 运行时 DLL
- [ ] 未提交密钥、个人路径、`dist/`、`target/`、`runtime/`、日志

## 关联 Issue

<!-- 无则写“无” -->

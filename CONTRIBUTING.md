# 参与贡献

感谢参与 FeatherAgent。项目以「小模型上下文工程 + 极轻量可移植内核」为目标，提交时请优先保持内核体积、启动与 RPC 门禁不退化。

## 开发环境

- Rust 1.80+，Linux x64 或 Windows x64；
- Linux 交叉编译 Windows 包需要 `mingw-w64` 与 `x86_64-pc-windows-gnu` target；
- 真实模型联调可选 Ollama；`provider-mock` 覆盖离线工具闭环。

## 本地检查

```bash
cargo fmt --check
cargo test --all-targets
scripts/version-check.sh
scripts/build-linux.sh
```

Windows 交叉编译与打包：

```bash
scripts/build-windows.sh
scripts/package-windows.sh --skip-build
```

`cargo test` 中有 1 个需要公网的 ignored 用例；发布前单独运行：

```bash
cargo test --test integration local_web_search_tool_via_kernel -- --ignored
```

## 提交规范

- 一个提交解决一个主题，说明「现象 → 根因 → 修复 → 验证」；
- 不提交 `target/`、`dist/`、`runtime/`、`data/`、`workspace/`、日志或任何密钥；
- 修改版本时同时更新 `Cargo.toml` 与全部 `plugins/*/plugin.json`，再跑 `scripts/version-check.sh`；
- 修改 Windows 资源或图标时，确认 5 个 exe 均有 `.rsrc` 且不依赖 MinGW 运行时 DLL；
- 新增平台相关代码必须同时通过 Linux 与 Windows 交叉编译。

## 分支与审阅

`main` 由仓库拥有者维护。外部贡献者请从自己的 fork 开分支并通过 Pull Request 提交；在分支保护不可用的私有仓库阶段，由拥有者手工执行同等规则：至少一次审阅、CI 通过、对话解决后再合并。

## 许可证

提交即表示同意以 [MIT License](LICENSE) 授权本项目。

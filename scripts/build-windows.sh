#!/usr/bin/env bash
# Windows x64 交叉编译（mingw-w64 + Rust windows-gnu target）。
# 用法: scripts/build-windows.sh
# 产物: target/x86_64-pc-windows-gnu/release/*.exe
set -euo pipefail
cd "$(dirname "$0")/.."

export PATH="${HOME}/.cargo/bin:${PATH}"
TARGET=x86_64-pc-windows-gnu

command -v x86_64-w64-mingw32-gcc >/dev/null || {
  echo "缺少 mingw-w64，请先: sudo apt-get install -y mingw-w64" >&2
  exit 1
}
rustup target list --installed | grep -qx "$TARGET" || rustup target add "$TARGET"

# C 依赖（rusqlite bundled / zstd）必须用 mingw 交叉编译器，
# 否则 cc crate 会调用宿主 gcc，产出 ELF 目标文件，链接期全是 undefined reference。
export CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc
export AR_x86_64_pc_windows_gnu=x86_64-w64-mingw32-ar
# 静态 CRT，最终 exe 只依赖 Windows 系统 DLL（已验证无 libgcc/libwinpthread）。
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="${CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS:--C target-feature=+crt-static}"

cargo build --release --target "$TARGET" --bins
echo "built:"
ls -lh target/$TARGET/release/*.exe

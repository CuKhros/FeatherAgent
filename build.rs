fn main() {
    println!("cargo:rerun-if-changed=assets/featheragent.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winres::WindowsResource::new();
    res.set_icon("assets/featheragent.ico");
    res.set("ProductName", "FeatherAgent");
    res.set("FileDescription", "FeatherAgent");
    res.set("CompanyName", "FeatherAgent");
    res.set("LegalCopyright", "FeatherAgent");
    // Linux 上交叉编译到 windows-gnu 时，MinGW 工具带目标前缀。
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu") {
        let host = std::env::var("HOST").unwrap_or_default();
        let target = std::env::var("TARGET").unwrap_or_default();
        let arch = if target.starts_with("x86_64") {
            "x86_64"
        } else {
            "i686"
        };
        if host.contains("linux") || host.contains("darwin") {
            res.set_windres_path(&format!("{arch}-w64-mingw32-windres"));
            res.set_ar_path(&format!("{arch}-w64-mingw32-ar"));
        }
    }
    if let Err(e) = res.compile() {
        panic!("Windows 资源（图标/版本信息）编译失败: {e}");
    }
    // winres 在 GNU 工具链下只生成仅含 .rsrc 的静态库；该成员没有符号，
    // GNU ld 不会从 .a 中拉取它，必须把 resource.o 直接交给链接器。
    if let Ok(out) = std::env::var("OUT_DIR") {
        println!("cargo:rustc-link-arg={out}/resource.o");
    }
}

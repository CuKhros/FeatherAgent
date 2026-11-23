//! 治理层（设计文档 §3.4 / §3.5 / §4.3 / §7.1）：
//! 路径解析、环境沙箱、卷类型守卫、协作式权限门。

pub mod paths {
    use std::path::{Path, PathBuf};

    /// 三类路径可独立外置（§7.1）。
    #[derive(Debug, Clone)]
    pub struct Paths {
        pub install_root: PathBuf,
        pub data: PathBuf,
        pub workspace: PathBuf,
        pub runtime: PathBuf,
    }

    #[derive(Debug, Clone, Default)]
    pub struct Overrides {
        pub install_root: Option<PathBuf>,
        pub data: Option<PathBuf>,
        pub workspace: Option<PathBuf>,
        pub runtime: Option<PathBuf>,
    }

    impl Paths {
        /// 以可执行文件位置为根，**绝不依赖 CWD / 环境变量 / 注册表**（§3.4）。
        pub fn resolve(exe: &Path, ov: &Overrides) -> Paths {
            let exe_dir = exe
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            let install_root = ov.install_root.clone().unwrap_or_else(|| exe_dir.clone());
            // 统一成绝对路径：Windows 命名管道名由 socket 路径哈希得到，
            // 若一端用 `.`、另一端用安装根的绝对路径，会算出不同的管道名。
            let install_root = std::path::absolute(&install_root).unwrap_or(install_root);
            let data = ov.data.clone().unwrap_or_else(|| install_root.join("data"));
            let workspace = ov
                .workspace
                .clone()
                .unwrap_or_else(|| install_root.join("workspace"));
            let runtime = ov
                .runtime
                .clone()
                .unwrap_or_else(|| install_root.join("runtime"));
            Paths {
                install_root,
                data,
                workspace,
                runtime,
            }
        }

        pub fn create_all(&self) -> std::io::Result<()> {
            std::fs::create_dir_all(&self.data)?;
            std::fs::create_dir_all(&self.workspace)?;
            std::fs::create_dir_all(self.runtime.join("tmp"))?;
            std::fs::create_dir_all(self.runtime.join("home"))?;
            std::fs::create_dir_all(self.runtime.join("ipc"))?;
            Ok(())
        }

        pub fn socket(&self) -> PathBuf {
            self.runtime.join("ipc").join("featheragent.sock")
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn rooted_at_exe_not_cwd() {
            let p = Paths::resolve(
                Path::new("/opt/feather/bin/featheragent"),
                &Overrides::default(),
            );
            assert_eq!(p.install_root, PathBuf::from("/opt/feather/bin"));
            assert_eq!(p.data, PathBuf::from("/opt/feather/bin/data"));
            assert_eq!(p.workspace, PathBuf::from("/opt/feather/bin/workspace"));
        }

        #[test]
        fn overrides_split_paths() {
            let ov = Overrides {
                data: Some(PathBuf::from("/mnt/d")),
                ..Default::default()
            };
            let p = Paths::resolve(Path::new("/opt/feather/bin/featheragent"), &ov);
            assert_eq!(p.data, PathBuf::from("/mnt/d"));
            assert_eq!(p.install_root, PathBuf::from("/opt/feather/bin"));
        }
    }
}

pub mod volume {
    use std::path::{Path, PathBuf};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum VolumeKind {
        Fixed,
        Network,
        Sync,
        Removable,
        Unknown,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Purpose {
        Data,
        Workspace,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Verdict {
        Ok,
        Warn,
        Reject,
    }

    /// 已知同步盘名单（随版本维护，可配置；§3.5）。
    pub const DEFAULT_SYNC_NAMES: &[&str] = &[
        "onedrive",
        "dropbox",
        "nutstore",
        "坚果云",
        "baidunetdisk",
        "百度网盘",
        "google drive",
        "googledrive",
        "icloud",
        "微云",
        "weiyun",
        "seafile",
    ];

    pub fn is_sync_path(path: &Path, extra: &[String]) -> bool {
        let mut names: Vec<String> = DEFAULT_SYNC_NAMES
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
        names.extend(extra.iter().map(|s| s.to_lowercase()));
        for comp in path.components() {
            let c = comp.as_os_str().to_string_lossy().to_lowercase();
            if names.iter().any(|n| !n.is_empty() && c.contains(n)) {
                return true;
            }
        }
        false
    }

    #[cfg(target_os = "linux")]
    fn statfs_type(path: &Path) -> Option<i64> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let c = CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut st: libc::statfs = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::statfs(c.as_ptr(), &mut st) };
        if rc != 0 {
            return None;
        }
        Some(st.f_type as i64)
    }

    #[cfg(not(target_os = "linux"))]
    fn statfs_type(_path: &Path) -> Option<i64> {
        None
    }

    /// Windows 盘类型：固定盘放行、网络盘拒绝、可移动盘告警。
    #[cfg(windows)]
    fn win_drive_kind(path: &Path) -> Option<VolumeKind> {
        use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
        let s = path.to_string_lossy();
        let s = s.strip_prefix(r"\?\").unwrap_or(&s);
        if s.starts_with(r"\\") {
            return Some(VolumeKind::Network); // UNC 共享
        }
        let bytes = s.as_bytes();
        if bytes.len() >= 2 && bytes[1] == b':' {
            let leaf = format!("{}:\\", &s[..1]);
            let w: Vec<u16> = leaf.encode_utf16().chain(std::iter::once(0)).collect();
            let t = unsafe { GetDriveTypeW(w.as_ptr()) };
            return Some(match t {
                2 => VolumeKind::Removable,
                3 | 6 => VolumeKind::Fixed,
                4 => VolumeKind::Network,
                _ => VolumeKind::Unknown,
            });
        }
        None
    }

    pub fn classify(path: &Path, extra_sync: &[String]) -> VolumeKind {
        if is_sync_path(path, extra_sync) {
            return VolumeKind::Sync;
        }
        #[cfg(windows)]
        if let Some(k) = win_drive_kind(path) {
            return k;
        }
        let Some(t) = statfs_type(path) else {
            return VolumeKind::Unknown;
        };
        // Linux magic：NFS 0x6969、CIFS 0xFF534D42、SMB2 0xFE534D42、
        // FUSE 0x65735546、OVERLAYFS 0x794C7630、9P 0x01021997
        const NETWORK: &[i64] = &[0x6969, 0xFF53_4D42, 0xFE53_4D42, 0x0102_1997];
        const FUSE: i64 = 0x6573_5546;
        const OVERLAY: i64 = 0x794C_7630;
        if NETWORK.contains(&t) {
            VolumeKind::Network
        } else if t == FUSE || t == OVERLAY {
            // FUSE 可能是 sshfs/rclone/同步盘挂载；按"未知/可移动"保守处理
            VolumeKind::Unknown
        } else {
            VolumeKind::Fixed
        }
    }

    /// 独占锁探测：同步盘客户端常持有目录内文件锁（§3.5）。
    pub fn probe_lock(dir: &Path) -> std::io::Result<bool> {
        std::fs::create_dir_all(dir)?;
        let probe = dir.join(".fa_probe");
        let f = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&probe)?;
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let rc = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if rc == 0 {
                unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_UN) };
                Ok(true)
            } else {
                Ok(false)
            }
        }
        #[cfg(not(unix))]
        {
            let _ = f;
            Ok(true)
        }
    }

    /// 判定矩阵（§3.5）。
    pub fn judge(kind: VolumeKind, purpose: Purpose) -> Verdict {
        match (kind, purpose) {
            (VolumeKind::Fixed, _) => Verdict::Ok,
            (VolumeKind::Network, Purpose::Data) => Verdict::Reject,
            (VolumeKind::Network, Purpose::Workspace) => Verdict::Warn,
            (VolumeKind::Sync, Purpose::Data) => Verdict::Reject,
            (VolumeKind::Sync, Purpose::Workspace) => Verdict::Warn,
            (VolumeKind::Removable, Purpose::Data) => Verdict::Warn,
            (VolumeKind::Removable, Purpose::Workspace) => Verdict::Ok,
            (VolumeKind::Unknown, _) => Verdict::Warn,
        }
    }

    pub fn label(kind: VolumeKind) -> &'static str {
        match kind {
            VolumeKind::Fixed => "local-fixed",
            VolumeKind::Network => "network",
            VolumeKind::Sync => "sync",
            VolumeKind::Removable => "removable",
            VolumeKind::Unknown => "unknown",
        }
    }

    pub fn paths_under(path: &Path) -> Vec<PathBuf> {
        path.ancestors().map(|p| p.to_path_buf()).collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn sync_names_detected() {
            assert!(is_sync_path(Path::new("/home/u/OneDrive/repo"), &[]));
            assert!(is_sync_path(Path::new("/Users/u/坚果云/x"), &[]));
            assert!(!is_sync_path(Path::new("/home/u/projects/x"), &[]));
            assert!(is_sync_path(
                Path::new("/mnt/custom/x"),
                &["custom".to_string()]
            ));
        }

        #[test]
        fn matrix_matches_doc() {
            assert_eq!(judge(VolumeKind::Network, Purpose::Data), Verdict::Reject);
            assert_eq!(
                judge(VolumeKind::Network, Purpose::Workspace),
                Verdict::Warn
            );
            assert_eq!(judge(VolumeKind::Sync, Purpose::Data), Verdict::Reject);
            assert_eq!(judge(VolumeKind::Fixed, Purpose::Data), Verdict::Ok);
            assert_eq!(
                judge(VolumeKind::Removable, Purpose::Workspace),
                Verdict::Ok
            );
        }

        #[test]
        fn tmp_is_not_network() {
            let d = tempfile::tempdir().unwrap();
            assert_ne!(classify(d.path(), &[]), VolumeKind::Network);
            assert!(probe_lock(d.path()).unwrap());
        }
    }
}

pub mod sandbox {
    use std::path::Path;
    use std::process::Command;

    /// 启动子进程前必须 unset / 覆盖的 HOME 依赖变量（随版本维护）。
    pub const SCRUB_VARS: &[&str] = &[
        "RIPGREP_CONFIG_PATH",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_RUNTIME_DIR",
        "XDG_STATE_HOME",
        "GIT_CONFIG_GLOBAL",
        "PYTHONPATH",
        "NODE_PATH",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "TMPDIR",
        "TEMP",
        "TMP",
    ];

    #[derive(Debug, Clone)]
    pub struct SandboxCfg {
        pub strict_temp: bool,
        pub runtime_dir: std::path::PathBuf,
    }

    impl Default for SandboxCfg {
        fn default() -> Self {
            Self {
                strict_temp: true,
                runtime_dir: std::path::PathBuf::from("runtime"),
            }
        }
    }

    /// 对将要启动的子进程应用环境沙箱：清掉已知 HOME 依赖变量，
    /// 把 HOME 指到 `runtime/home`，严格模式把临时目录指到 `runtime/tmp`。
    pub fn apply(cmd: &mut Command, cfg: &SandboxCfg) {
        let home = cfg.runtime_dir.join("home");
        let tmp = cfg.runtime_dir.join("tmp");
        let _ = std::fs::create_dir_all(&home);
        let _ = std::fs::create_dir_all(&tmp);
        for v in SCRUB_VARS {
            cmd.env_remove(v);
        }
        cmd.env("HOME", &home);
        if cfg.strict_temp {
            cmd.env("TMPDIR", &tmp);
            cmd.env("TEMP", &tmp);
            cmd.env("TMP", &tmp);
        }
        // 明确标记：子进程知道自己在沙箱里
        cmd.env("FEATHER_SANDBOX", "1");
    }

    /// 白名单例外：TLS/字体缓存等确实需要写用户目录的程序（§3.4）。
    pub fn allow_listed(program: &Path) -> bool {
        let p = program.to_string_lossy().to_lowercase();
        p.contains("chrome") || p.contains("edge") || p.contains("firefox")
    }
}

pub mod permission {
    use std::collections::HashMap;

    use parking_lot::Mutex;
    use serde_json::json;

    use crate::proto::types::Risk;
    use crate::util::random_token;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Decision {
        Allowed,
        ConfirmNeeded { token: String },
        Denied,
    }

    #[derive(Debug, Clone)]
    struct Pending {
        allowed: Option<bool>,
    }

    /// 协作式权限门：只拦"守规矩的调用者"，**不是沙箱**（§4.3）。
    pub struct Gate {
        default_risk: HashMap<String, Risk>,
        pending: Mutex<HashMap<String, Pending>>,
        pub auto_allow_safe: std::sync::atomic::AtomicBool,
    }

    impl Default for Gate {
        fn default() -> Self {
            Self::new(true)
        }
    }

    impl Gate {
        pub fn new(auto_allow_safe: bool) -> Self {
            Self {
                default_risk: HashMap::new(),
                pending: Mutex::new(HashMap::new()),
                auto_allow_safe: std::sync::atomic::AtomicBool::new(auto_allow_safe),
            }
        }

        pub fn register(&mut self, tool: &str, risk: Risk) {
            self.default_risk.insert(tool.to_string(), risk);
        }

        pub fn risk_of(&self, tool: &str) -> Risk {
            self.default_risk.get(tool).copied().unwrap_or(Risk::Safe)
        }

        pub fn check(&self, tool: &str, _session: &str) -> Decision {
            let risk = self.risk_of(tool);
            if risk == Risk::Safe
                && self
                    .auto_allow_safe
                    .load(std::sync::atomic::Ordering::Relaxed)
            {
                return Decision::Allowed;
            }
            let token = random_token(8);
            self.pending
                .lock()
                .insert(token.clone(), Pending { allowed: None });
            Decision::ConfirmNeeded { token }
        }

        pub fn set_auto_allow_safe(&self, on: bool) {
            self.auto_allow_safe
                .store(on, std::sync::atomic::Ordering::Relaxed);
        }

        /// 前端 `tool.confirm / deny` 调到这里；返回是否被授权。
        pub fn resolve(&self, token: &str, allow: bool) -> bool {
            let mut p = self.pending.lock();
            let found = p.contains_key(token);
            if let Some(e) = p.get_mut(token) {
                e.allowed = Some(allow);
            }
            p.remove(token);
            found && allow
        }

        /// 人类可读的拒绝原因，用于 `permission.denied` 事件。
        pub fn denied_payload(tool: &str, reason: &str) -> serde_json::Value {
            json!({"tool": tool, "reason": reason})
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn safe_auto_allowed_dangerous_needs_confirm() {
            let mut g = Gate::new(true);
            g.register("tool.fs.read", Risk::Safe);
            g.register("tool.shell", Risk::Dangerous);
            assert_eq!(g.check("tool.fs.read", "s"), Decision::Allowed);
            let d = g.check("tool.shell", "s");
            match d {
                Decision::ConfirmNeeded { token } => {
                    assert!(!g.resolve(&token, false));
                    assert!(!g.resolve(&token, true)); // 已消费
                }
                _ => panic!("expected confirm"),
            }
        }
    }
}

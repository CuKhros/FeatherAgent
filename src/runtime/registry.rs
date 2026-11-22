//! 插件注册表（§4.4 / §8）：安装、启用/停用、清单持久化。
//!
//! 清单落 `data/plugins/registry.json`，插件目录落 `data/plugins/<name>/`。
//! 安装时的内容哈希（BLAKE3）与签名字段按 §4.4 预留。

use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::proto::types::PluginManifest;
use crate::runtime::plugin::PluginSpec;
use crate::proto::Codec;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub category: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub blake3: String,
    /// 签名位预留；为空表示未签名
    #[serde(default)]
    pub signature: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RegistryFile {
    #[serde(default)]
    plugins: Vec<Entry>,
}

pub struct Registry {
    path: PathBuf,
    dir: PathBuf,
    inner: Mutex<RegistryFile>,
}

impl Registry {
    pub fn load(dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("registry.json");
        let file = if path.exists() {
            serde_json::from_slice(&std::fs::read(&path)?).unwrap_or_default()
        } else {
            RegistryFile::default()
        };
        Ok(Self {
            path,
            dir: dir.to_path_buf(),
            inner: Mutex::new(file),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn save(&self, f: &RegistryFile) -> std::io::Result<()> {
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(f).unwrap())?;
        crate::util::replace_file(&tmp, &self.path)?;
        Ok(())
    }

    pub fn list(&self) -> Vec<Entry> {
        self.inner.lock().plugins.clone()
    }

    /// 安装：从目录读取 `manifest.json`（可选 `plugin.json`），登记并计算 BLAKE3。
    pub fn install(&self, source: &Path) -> std::io::Result<Entry> {
        let manifest_path = source.join("plugin.json");
        let mut entry = Entry {
            name: source
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "plugin".into()),
            program: source.to_string_lossy().into_owned(),
            args: Vec::new(),
            category: "tool".into(),
            enabled: true,
            blake3: String::new(),
            signature: None,
        };
        if manifest_path.exists() {
            let v: Value = serde_json::from_slice(&std::fs::read(&manifest_path)?)?;
            if let Some(m) = v.get("manifest").and_then(|m| serde_json::from_value::<PluginManifest>(m.clone()).ok()) {
                entry.name = m.name;
                entry.category = m.category;
            }
            if let Some(p) = v.get("program").and_then(|p| p.as_str()) {
                let mut prog = source.join(p);
                #[cfg(windows)]
                {
                    // 清单里的 program 不带扩展名；Windows 包内实际是 .exe
                    if !prog.exists() {
                        let cand = prog.with_extension("exe");
                        if cand.exists() {
                            prog = cand;
                        }
                    }
                }
                entry.program = prog.to_string_lossy().into_owned();
            }
            if let Some(a) = v.get("args").and_then(|a| a.as_array()) {
                entry.args = a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect();
            }
            entry.signature = v.get("signature").and_then(|s| s.as_str()).map(|s| s.to_string());
        }
        // 内容哈希：对可执行文件
        if let Ok(bytes) = std::fs::read(&entry.program) {
            entry.blake3 = blake3::hash(&bytes).to_hex().to_string();
        }
        let mut f = self.inner.lock();
        f.plugins.retain(|p| p.name != entry.name);
        f.plugins.push(entry.clone());
        self.save(&f)?;
        Ok(entry)
    }

    pub fn enable(&self, name: &str, on: bool) -> Option<Entry> {
        let mut f = self.inner.lock();
        let mut found = None;
        for p in f.plugins.iter_mut() {
            if p.name == name {
                p.enabled = on;
                found = Some(p.clone());
            }
        }
        let _ = self.save(&f);
        found
    }

    pub fn specs(&self) -> Vec<PluginSpec> {
        self.inner
            .lock()
            .plugins
            .iter()
            .filter(|p| p.enabled)
            .map(|p| PluginSpec {
                name: p.name.clone(),
                program: PathBuf::from(&p.program),
                args: p.args.clone(),
                codec: Codec::MsgPack,
            })
            .collect()
    }

    /// 扫描内置插件目录：每个子目录若含 `plugin.json` 且未登记，则自动安装。
    pub fn scan_builtin(&self, dir: &Path) -> std::io::Result<Vec<Entry>> {
        let mut installed = Vec::new();
        let existing: Vec<String> = self.list().iter().map(|e| e.name.clone()).collect();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if !p.is_dir() || !p.join("plugin.json").exists() {
                    continue;
                }
                let name = p
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if existing.iter().any(|n| n == &name) {
                    continue;
                }
                if let Ok(entry) = self.install(&p) {
                    installed.push(entry);
                }
            }
        }
        Ok(installed)
    }

    pub fn find(&self, name: &str) -> Option<Entry> {
        self.inner.lock().plugins.iter().find(|p| p.name == name).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_and_toggle() {
        let d = tempfile::tempdir().unwrap();
        let reg = Registry::load(d.path()).unwrap();
        let exe = d.path().join("fake");
        std::fs::write(&exe, b"#!/bin/sh\nexit 0\n").unwrap();
        let src = d.path().join("plug");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("plugin.json"),
            serde_json::json!({
                "manifest": {"name": "p1", "version": "0.1", "api": ">=1.0 <2.0", "category": "tool"},
                "program": "../fake"
            })
            .to_string(),
        )
        .unwrap();
        let e = reg.install(&src).unwrap();
        assert_eq!(e.name, "p1");
        assert!(!e.blake3.is_empty());
        assert!(reg.enable("p1", false).is_some());
        assert!(reg.specs().is_empty());
    }
}

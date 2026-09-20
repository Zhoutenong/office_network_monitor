//! 构建脚本：把 app.ico 作为 exe 图标嵌入。
//!
//! 找不到 rc.exe（Windows SDK 的资源编译器）时只打印警告并跳过，
//! 不影响编译产物本身；可用环境变量 `RC_EXE` 显式指定。

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=app.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let rc = match find_rc() {
        Some(path) => path,
        None => {
            println!("cargo:warning=未找到 rc.exe，跳过 exe 图标嵌入（可用 RC_EXE 指定路径）");
            return;
        }
    };

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default());
    let resource = out_dir.join("app.res");
    let compiled = std::process::Command::new(&rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&resource)
        .arg("app.rc")
        .status();

    match compiled {
        Ok(status) if status.success() && resource.exists() => {
            println!("cargo:rustc-link-arg={}", resource.display());
        }
        _ => println!("cargo:warning=图标资源编译失败，跳过嵌入"),
    }
}

fn find_rc() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("RC_EXE") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
    }

    let root = std::env::var("ProgramFiles(x86)")
        .unwrap_or_else(|_| r"C:\Program Files (x86)".to_string());
    let base = Path::new(&root).join("Windows Kits").join("10").join("bin");
    let mut candidates: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(&base).ok()? {
        let candidate = entry.ok()?.path().join("x64").join("rc.exe");
        if candidate.exists() {
            candidates.push(candidate);
        }
    }
    candidates.sort();
    candidates.pop()
}

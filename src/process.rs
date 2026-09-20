//! 进程枚举（用于判断 openvpn.exe 之类是否在运行），基于 CreateToolhelp32Snapshot。

use crate::ffi::win;
use crate::ffi::{self};

/// 一次快照拿到全部进程名，避免每个名字各扫一遍。
pub fn snapshot_process_names() -> Vec<String> {
    let snapshot = unsafe { win::CreateToolhelp32Snapshot(win::TH32CS_SNAPPROCESS, 0) };
    if snapshot == win::INVALID_HANDLE_VALUE || snapshot.is_null() {
        return Vec::new();
    }
    let mut entry: win::PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dw_size = std::mem::size_of::<win::PROCESSENTRY32W>() as u32;

    let mut names = Vec::new();
    let mut ok = unsafe { win::Process32FirstW(snapshot, &mut entry) };
    while ok != 0 {
        names.push(ffi::from_wide(&entry.sz_exe_file));
        if names.len() > 2000 {
            break;
        }
        ok = unsafe { win::Process32NextW(snapshot, &mut entry) };
    }
    unsafe { win::CloseHandle(snapshot) };
    names
}

pub fn is_running(name: &str) -> bool {
    let wanted = name.to_lowercase();
    snapshot_process_names()
        .iter()
        .any(|item| item.to_lowercase() == wanted)
}

//! 原生 Win32 FFI 声明。全部使用系统 API，不引入任何第三方 crate。

#![allow(non_snake_case, non_camel_case_types, dead_code, clippy::upper_case_acronyms)]

pub mod net;
pub mod win;

use std::ffi::c_void;

pub type HANDLE = *mut c_void;
pub type HWND = *mut c_void;
pub type HMODULE = *mut c_void;
pub type HICON = *mut c_void;
pub type HCURSOR = *mut c_void;
pub type HBRUSH = *mut c_void;
pub type HFONT = *mut c_void;
pub type HDC = *mut c_void;
pub type HBITMAP = *mut c_void;
pub type HMENU = *mut c_void;
pub type HKEY = *mut c_void;
pub type HINTERNET = *mut c_void;
pub type LRESULT = isize;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type BOOL = i32;
pub type DWORD = u32;
pub type UINT = u32;

/// 转成以 0 结尾的 UTF-16，用于 Win32 的 W 系列接口。
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 从定长 UTF-16 缓冲区取出字符串。
pub fn from_wide(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

/// 把字符串写进定长 UTF-16 缓冲区（带截断与结尾 0）。
pub fn write_wide(target: &mut [u16], text: &str) {
    if target.is_empty() {
        return;
    }
    let mut index = 0;
    for code in text.encode_utf16() {
        if index + 1 >= target.len() {
            break;
        }
        target[index] = code;
        index += 1;
    }
    for slot in target.iter_mut().skip(index) {
        *slot = 0;
    }
}

pub fn last_error() -> u32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0) as u32
}

/// 解析 IPv4 字符串为 "网络字节序" 的 u32（内存里就是那 4 个字节）。
pub fn ipv4_to_u32(text: &str) -> Option<u32> {
    let mut octets = [0u8; 4];
    let mut count = 0;
    for part in text.split('.') {
        if count >= 4 {
            return None;
        }
        octets[count] = part.trim().parse::<u8>().ok()?;
        count += 1;
    }
    if count != 4 {
        return None;
    }
    Some(u32::from_ne_bytes(octets))
}

pub fn u32_to_ipv4(value: u32) -> String {
    let octets = value.to_ne_bytes();
    format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3])
}

/// RFC1918 私有地址判断。
pub fn is_private_ipv4(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    let (first, second) = match (parts[0].parse::<u8>(), parts[1].parse::<u8>()) {
        (Ok(a), Ok(b)) => (a, b),
        _ => return false,
    };
    first == 10 || (first == 172 && (16..=31).contains(&second)) || (first == 192 && second == 168)
}

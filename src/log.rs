//! 极简日志：只在有内容时写文件，避免桌面上多出空文件。

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn init(path: PathBuf) {
    if let Ok(mut guard) = LOG_PATH.lock() {
        *guard = Some(path);
    }
}

pub fn write(level: &str, message: &str) {
    let path = match LOG_PATH.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => None,
    };
    let path = match path {
        Some(value) => value,
        None => return,
    };
    // 超过 256KB 就直接重开，等价于简单轮转
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > 256 * 1024 {
            let _ = std::fs::remove_file(&path);
        }
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or(0);
        let _ = writeln!(file, "[{}] {} {}", stamp, level, message);
    }
}

pub fn warn(message: &str) {
    write("WARN", message);
}

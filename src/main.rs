//! 办公网络状态监测 · 原生版
//!
//! 与 Python 版同名同结构的 config.json、同样的三条通道与判定逻辑，
//! 但全部使用系统 API（WinHTTP / IP Helper / GDI+ / Shell_NotifyIcon），
//! 不依赖任何第三方库。

#![cfg_attr(
    all(not(debug_assertions), not(feature = "console")),
    windows_subsystem = "windows"
)]

mod bridge;
mod config;
mod ffi;
mod gdiplus;
mod json;
mod log;
mod monitor;
mod panel;
mod probe;
mod process;
mod report;
mod tray;

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use config::Config;
use ffi::win;
use monitor::{Engine, Level};

const MUTEX_NAME: &str = "Global\\OfficeNetworkMonitor";
const ERROR_ALREADY_EXISTS: u32 = 183;

pub fn base_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn level_tag(level: Level) -> &'static str {
    match level {
        Level::Good => "正常",
        Level::Warn => "偏慢",
        Level::Bad => "不通",
        Level::Off => "未连接",
        Level::Unknown => "未知",
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let base = base_dir();
    log::init(base.join("netmon.log"));
    let config_path = base.join("config.json");

    if arguments.iter().any(|item| item == "--selftest") {
        selftest(&config_path);
        return;
    }
    run_gui(&config_path);
}

fn run_gui(config_path: &Path) {
    unsafe {
        if win::SetProcessDpiAwarenessContext(win::DPI_PER_MONITOR_V2 as *mut std::ffi::c_void) == 0 {
            win::SetProcessDPIAware();
        }
    }

    let name = ffi::wide(MUTEX_NAME);
    let mutex = unsafe { win::CreateMutexW(std::ptr::null_mut(), 1, name.as_ptr()) };
    if !mutex.is_null() && ffi::last_error() == ERROR_ALREADY_EXISTS {
        let text = ffi::wide("网络状态监测已在运行中，请查看任务栏托盘图标。");
        let caption = ffi::wide("网络状态监测");
        unsafe {
            win::MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                caption.as_ptr(),
                win::MB_OK | win::MB_ICONINFORMATION,
            )
        };
        return;
    }

    gdiplus::init();
    let config = Config::load(config_path);
    let runtime = Arc::new(tray::Runtime::new(Engine::new(config)));
    let hwnd = tray::create(runtime.clone());

    // 本机只读接口（/status、/report），供脚本 / AI Agent 拉取
    if let Some(url) = bridge::start(Arc::clone(&runtime)) {
        if let Ok(mut guard) = runtime.bridge_url.lock() {
            *guard = Some(url);
        }
    }

    log::write(
        "INFO",
        &format!("main thread id = {}", unsafe { win::GetCurrentThreadId() }),
    );
    let worker = runtime.clone();
    let hwnd_value = hwnd as isize;
    std::thread::spawn(move || monitor_loop(worker, hwnd_value));

    // 自检用：启动后自动把面板弹出来
    if std::env::args().any(|item| item == "--show-panel") {
        let target = hwnd as isize;
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(2500));
            unsafe { win::PostMessageW(target as *mut std::ffi::c_void, win::WM_TRAY_PANEL, 0, 0) };
        });
    }

    tray::message_loop();

    runtime.quit.store(true, Ordering::Relaxed);
    gdiplus::shutdown();
}

/// 探测线程：按配置间隔刷新，结果通过消息通知界面线程。
fn monitor_loop(runtime: Arc<tray::Runtime>, hwnd: isize) {
    log::write(
        "INFO",
        &format!("monitor thread id = {}", unsafe { win::GetCurrentThreadId() }),
    );
    loop {
        if runtime.quit.load(Ordering::Relaxed) {
            break;
        }
        // 精确固定周期在流量分析里是"信标"特征（更容易被当成异常），加 ±20% 抖动打散
        let interval = runtime.interval() * (0.8 + next_random() * 0.4);
        let probe_started = Instant::now();
        let snapshot = match runtime.engine.lock() {
            Ok(mut engine) => engine.refresh(),
            Err(_) => break,
        };
        // 正常每轮零点几秒；明显偏慢才记一笔，避免日志被日常噪声淹没
        let probe_ms = probe_started.elapsed().as_secs_f64() * 1000.0;
        if probe_ms > 5000.0 {
            log::write(
                "WARN",
                &format!("本轮探测耗时 {:.0} ms，偏慢（网络或探测目标异常）", probe_ms),
            );
        }
        runtime.stage(&snapshot);
        if let Ok(mut guard) = runtime.snapshot.lock() {
            *guard = Some(snapshot);
        }
        tray::post_update();
        let _ = hwnd;

        // 分片等待，便于“立即刷新”即时生效
        let mut waited = 0.0f64;
        while waited < interval {
            std::thread::sleep(Duration::from_millis(100));
            waited += 0.1;
            if runtime.kick.swap(false, Ordering::Relaxed) || runtime.quit.load(Ordering::Relaxed) {
                break;
            }
        }
    }
}

/// 极简 xorshift 伪随机数（0.0 ~ 1.0），只为探测间隔抖动服务，不引第三方库。
fn next_random() -> f64 {
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEED: AtomicU64 = AtomicU64::new(0);
    let mut state = SEED.load(Ordering::Relaxed);
    if state == 0 {
        state = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15)
            | 1;
    }
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    SEED.store(state, Ordering::Relaxed);
    (state >> 11) as f64 / (1u64 << 53) as f64
}

fn selftest(config_path: &Path) {
    let config = Config::load(config_path);
    println!("配置文件：{}", config_path.display());
    println!(
        "间隔 {}s / 超时 {}s / Clash 代理：[{}]",
        config.interval, config.timeout, config.clash.proxy
    );

    let mut engine = Engine::new(config);
    for round in 1..=2u32 {
        let started = Instant::now();
        let snapshot = engine.refresh();
        println!(
            "\n== 第 {} 轮，用时 {:.2}s ==",
            round,
            started.elapsed().as_secs_f64()
        );
        for channel in &snapshot.channels {
            let latency = channel
                .latency_ms
                .map(|value| format!("{:.0} ms", value))
                .unwrap_or_else(|| "—".to_string());
            println!(
                "{:<16} {:<6} {:>9}  {}  |  {}",
                channel.name,
                level_tag(channel.level),
                latency,
                channel.status,
                channel.detail
            );
        }
        for (label, ms) in &engine.last_timing {
            println!("  · {:<12} {:>7.0} ms", label, ms);
        }
        println!(
            "系统代理：{} {}",
            if snapshot.proxy_enabled { "开启" } else { "关闭" },
            snapshot.proxy_server
        );
    }

    if let Some(snapshot) = Some(engine.last()) {
        println!("\n=== 报告预览（Markdown 前 20 行）===");
        for line in report::to_markdown(&snapshot).lines().take(20) {
            println!("{}", line);
        }
    }
}

//! 原生托盘：图标、悬浮提示、右键菜单、消息循环。

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::ffi::win::{self, NOTIFYICONDATAW};
use crate::ffi::{self, HICON, HWND, LPARAM, LRESULT, WPARAM};
use crate::gdiplus;
use crate::monitor::{Engine, Level, Snapshot};

pub const MENU_PANEL: usize = 1;
pub const MENU_COPY: usize = 2;
pub const MENU_REFRESH: usize = 3;
pub const MENU_EDIT: usize = 4;
pub const MENU_RELOAD: usize = 5;
pub const MENU_QUIT: usize = 6;

struct SafeIcon(HICON);

// HICON 只是句柄；同一时刻只在一个线程内使用
unsafe impl Send for SafeIcon {}

struct PendingTray {
    icon: SafeIcon,
    tooltip: String,
}

pub struct Runtime {
    pub engine: Mutex<Engine>,
    pub snapshot: Mutex<Option<Snapshot>>,
    pending: Mutex<Option<PendingTray>>,
    pub kick: AtomicBool,
    pub quit: AtomicBool,
    /// 本机只读接口地址（未启用时为空）
    pub bridge_url: Mutex<Option<String>>,
}

impl Runtime {
    pub fn new(engine: Engine) -> Runtime {
        Runtime {
            engine: Mutex::new(engine),
            snapshot: Mutex::new(None),
            pending: Mutex::new(None),
            kick: AtomicBool::new(false),
            quit: AtomicBool::new(false),
            bridge_url: Mutex::new(None),
        }
    }

    pub fn interval(&self) -> f64 {
        self.engine
            .lock()
            .map(|engine| engine.config.interval)
            .unwrap_or(5.0)
    }

    /// 根据快照渲染新图标与提示，等待消息线程应用。
    pub fn stage(&self, snapshot: &Snapshot) {
        let color_of = |level: Level| match level {
            Level::Good => gdiplus::COLOR_GOOD,
            Level::Warn => gdiplus::COLOR_WARN,
            Level::Bad => gdiplus::COLOR_BAD,
            _ => gdiplus::COLOR_OFF,
        };
        let clash = snapshot.channel("clash").map(|c| color_of(c.level)).unwrap_or(gdiplus::COLOR_OFF);
        let vpn = snapshot.channel("vpn").map(|c| color_of(c.level)).unwrap_or(gdiplus::COLOR_OFF);
        let direct = snapshot.channel("direct").map(|c| c.level).map(color_of).unwrap_or(gdiplus::COLOR_OFF);
        let overall = color_of(snapshot.overall().0);
        let size = unsafe { win::GetSystemMetrics(49) }.clamp(16, 32); // SM_CXSMICON
        let icon = gdiplus::render_ring_icon(clash, vpn, direct, overall, size);
        let tooltip = tooltip_text(snapshot);
        if let Ok(mut guard) = self.pending.lock() {
            *guard = Some(PendingTray {
                icon: SafeIcon(icon),
                tooltip,
            });
        }
    }
}

pub fn tooltip_text(snapshot: &Snapshot) -> String {
    let mut parts = Vec::new();
    for channel in &snapshot.channels {
        let mark = match channel.level {
            Level::Good => "√",
            Level::Warn => "!",
            Level::Bad => "×",
            Level::Off => "○",
            Level::Unknown => "…",
        };
        let latency = channel
            .latency_ms
            .map(|value| format!("{:.0}ms", value))
            .unwrap_or_else(|| "—".to_string());
        let short = match channel.key {
            "clash" => "外网",
            "vpn" => "内网",
            _ => "国内",
        };
        parts.push(format!("{} {} {}", short, mark, latency));
    }
    parts.join(" · ")
}

static RUNTIME: OnceLock<Arc<Runtime>> = OnceLock::new();
static TRAY_WINDOW: AtomicIsize = AtomicIsize::new(0);

fn runtime() -> Option<&'static Arc<Runtime>> {
    RUNTIME.get()
}

fn class_name() -> &'static [u16] {
    Box::leak(ffi::wide("NetworkMonitorTrayWindow").into_boxed_slice())
}

/// 创建消息窗口并挂上托盘图标。
pub fn create(runtime: Arc<Runtime>) -> HWND {
    let _ = RUNTIME.set(runtime);
    unsafe {
        let instance = win::GetModuleHandleW(std::ptr::null());
        let mut window_class: win::WNDCLASSEXW = std::mem::zeroed();
        window_class.cb_size = std::mem::size_of::<win::WNDCLASSEXW>() as u32;
        window_class.lpfn_wnd_proc = Some(window_proc);
        window_class.h_instance = instance;
        window_class.lpsz_class_name = class_name().as_ptr();
        win::RegisterClassExW(&window_class);

        let hwnd = win::CreateWindowExW(
            0,
            class_name().as_ptr(),
            class_name().as_ptr(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null_mut(),
        );
        TRAY_WINDOW.store(hwnd as isize, Ordering::Relaxed);
        add_icon(hwnd, std::ptr::null_mut(), "网络状态监测");
        hwnd
    }
}

pub fn post_update() {
    let hwnd = TRAY_WINDOW.load(Ordering::Relaxed) as HWND;
    if !hwnd.is_null() {
        unsafe { win::PostMessageW(hwnd, win::WM_TRAY_UPDATE, 0, 0) };
    }
    // 面板若开着，让它一起刷新
    crate::panel::post_update();
}

/// 当前快照（面板与“复制报告”用）
pub fn current_snapshot() -> Option<Snapshot> {
    runtime().and_then(|rt| rt.snapshot.lock().ok().and_then(|guard| guard.clone()))
}

/// 本机只读接口地址（用于面板底部展示）
pub fn bridge_url() -> Option<String> {
    runtime().and_then(|rt| rt.bridge_url.lock().ok().and_then(|guard| guard.clone()))
}

fn add_icon(hwnd: HWND, icon: HICON, tooltip: &str) {
    let mut data: NOTIFYICONDATAW = NOTIFYICONDATAW::default();
    data.cb_size = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.h_wnd = hwnd;
    data.u_id = 1;
    data.u_flags = win::NIF_MESSAGE | win::NIF_ICON | win::NIF_TIP;
    data.u_callback_message = win::TRAY_CALLBACK_MESSAGE;
    data.h_icon = icon;
    ffi::write_wide(&mut data.sz_tip, tooltip);
    unsafe { win::Shell_NotifyIconW(win::NIM_ADD, &data) };
}

fn apply_pending(hwnd: HWND) {
    let pending = match runtime().and_then(|rt| rt.pending.lock().ok().and_then(|mut g| g.take())) {
        Some(value) => value,
        None => return,
    };
    let mut data: NOTIFYICONDATAW = NOTIFYICONDATAW::default();
    data.cb_size = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.h_wnd = hwnd;
    data.u_id = 1;
    data.u_flags = win::NIF_ICON | win::NIF_TIP;
    data.h_icon = pending.icon.0;
    ffi::write_wide(&mut data.sz_tip, &pending.tooltip);
    unsafe {
        win::Shell_NotifyIconW(win::NIM_MODIFY, &data);
        // 释放上一张图标句柄，避免泄漏
        let previous = PREVIOUS_ICON.swap(pending.icon.0 as isize, Ordering::Relaxed);
        if previous != 0 {
            win::DestroyIcon(previous as HICON);
        }
        if pending.icon.0.is_null() {
            crate::log::warn("托盘图标渲染失败");
        } else if !ICON_LOGGED.swap(true, Ordering::Relaxed) {
            crate::log::write("INFO", "托盘图标已挂载");
        }
    }
}

static ICON_LOGGED: AtomicBool = AtomicBool::new(false);

static PREVIOUS_ICON: AtomicIsize = AtomicIsize::new(0);

fn show_menu(hwnd: HWND) {
    unsafe {
        let menu = win::CreatePopupMenu();
        let items: [(usize, &str); 6] = [
            (MENU_PANEL, "打开面板"),
            (MENU_COPY, "复制诊断报告"),
            (MENU_REFRESH, "立即刷新"),
            (MENU_EDIT, "编辑配置"),
            (MENU_RELOAD, "重载配置"),
            (MENU_QUIT, "退出"),
        ];
        for (id, text) in items {
            let wide = ffi::wide(text);
            win::AppendMenuW(menu, win::MF_STRING, id, wide.as_ptr());
        }
        win::SetForegroundWindow(hwnd);
        let mut point = win::POINT::default();
        win::GetCursorPos(&mut point);
        let id = win::TrackPopupMenu(
            menu,
            win::TPM_RIGHTALIGN | win::TPM_BOTTOMALIGN | win::TPM_RETURNCMD | win::TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            hwnd,
            std::ptr::null(),
        ) as usize;
        win::DestroyMenu(menu);
        handle_menu(hwnd, id);
    }
}

fn handle_menu(hwnd: HWND, id: usize) {
    match id {
        MENU_PANEL => crate::panel::toggle(hwnd),
        MENU_COPY => copy_report(hwnd),
        MENU_REFRESH => {
            if let Some(rt) = runtime() {
                rt.kick.store(true, Ordering::Relaxed);
            }
        }
        MENU_EDIT => {
            let path = crate::base_dir().join("config.json");
            let mut command = std::process::Command::new("notepad.exe");
            command.arg(path);
            let _ = command.spawn();
        }
        MENU_RELOAD => {
            if let Some(rt) = runtime() {
                if let Ok(mut engine) = rt.engine.lock() {
                    engine.reload();
                }
                rt.kick.store(true, Ordering::Relaxed);
            }
        }
        MENU_QUIT => {
            unsafe { win::DestroyWindow(hwnd) };
        }
        _ => {}
    }
}

pub fn copy_report(hwnd: HWND) {
    let text = match runtime().and_then(|rt| rt.snapshot.lock().ok().and_then(|g| g.clone())) {
        Some(snapshot) => crate::report::to_markdown(&snapshot),
        None => "尚未完成首轮探测".to_string(),
    };
    set_clipboard_text(hwnd, &text);
}

pub fn set_clipboard_text(hwnd: HWND, text: &str) -> bool {
    let units: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = units.len() * 2;
    unsafe {
        if win::OpenClipboard(hwnd) == 0 {
            return false;
        }
        win::EmptyClipboard();
        let mem = win::GlobalAlloc(win::GMEM_MOVEABLE, bytes);
        if mem.is_null() {
            win::CloseClipboard();
            return false;
        }
        let pointer = win::GlobalLock(mem) as *mut u8;
        if pointer.is_null() {
            win::GlobalFree(mem);
            win::CloseClipboard();
            return false;
        }
        std::ptr::copy_nonoverlapping(units.as_ptr() as *const u8, pointer, bytes);
        win::GlobalUnlock(mem);
        win::SetClipboardData(win::CF_UNICODETEXT, mem);
        win::CloseClipboard();
        true
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    match message {
        win::WM_TRAY_UPDATE => {
            apply_pending(hwnd);
            0
        }
        win::TRAY_CALLBACK_MESSAGE => {
            match l_param as u32 {
                win::WM_LBUTTONUP => crate::panel::toggle(hwnd),
                win::WM_RBUTTONUP => show_menu(hwnd),
                _ => {}
            }
            0
        }
        win::WM_TRAY_PANEL => {
            crate::panel::toggle(hwnd);
            0
        }
        win::WM_COMMAND => {
            handle_menu(hwnd, w_param & 0xFFFF);
            0
        }
        win::WM_DESTROY => {
            if let Some(rt) = runtime() {
                rt.quit.store(true, Ordering::Relaxed);
            }
            let mut data: NOTIFYICONDATAW = NOTIFYICONDATAW::default();
            data.cb_size = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            data.h_wnd = hwnd;
            data.u_id = 1;
            win::Shell_NotifyIconW(win::NIM_DELETE, &data);
            win::PostQuitMessage(0);
            0
        }
        _ => win::DefWindowProcW(hwnd, message, w_param, l_param),
    }
}

pub fn message_loop() {
    unsafe {
        let mut message: win::MSG = std::mem::zeroed();
        let mut count: u64 = 0;
        while win::GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            count += 1;
            if count % 5000 == 0 {
                crate::log::write(
                    "INFO",
                    &format!(
                        "message loop {} iters, last msg=0x{:x}",
                        count, message.message
                    ),
                );
            }
            win::TranslateMessage(&message);
            win::DispatchMessageW(&message);
        }
    }
}

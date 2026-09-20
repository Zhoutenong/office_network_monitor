//! 状态面板：分层窗口（逐像素 alpha）+ GDI+ 绘制，点击托盘图标弹出。
//!
//! 视觉与 Python 版对齐：深色圆角、三张卡片（状态点 / 通道名 / 大号延迟 / 迷你波形）、
//! 底部环境信息与两个操作按钮；失焦或 Esc 自动收起。

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Mutex;

use crate::ffi::win::{self, BLENDFUNCTION, BITMAPINFO, BITMAPINFOHEADER, POINT, RECT, SIZE};
use crate::ffi::{self, HBITMAP, HDC, HWND, LPARAM, LRESULT, WPARAM};
use crate::gdiplus;
use crate::monitor::{Level, Snapshot};
use crate::tray;

// 逻辑尺寸（96 DPI 下的像素），实际会按 DPI 缩放
const WIDTH: i32 = 344;
const RADIUS: i32 = 14;
const PAD: i32 = 10;
const HEADER_H: i32 = 42;
const CARD_H: i32 = 62;
const CARD_GAP: i32 = 6;
const FOOTER_H: i32 = 74;

const BG: u32 = 0xFF15_1A21;
const CARD: u32 = 0xFF1B_2130;
const BORDER: u32 = 0xFF2C_3648;
const TEXT: u32 = 0xFFE8_EDF5;
const MUTED: u32 = 0xFF8B_98AB;
const DIM: u32 = 0xFF3B_4557;
const BUTTON: u32 = 0xFF23_2C3C;
const BUTTON_ACTIVE: u32 = 0xFF2F_3A4E;
const ACCENT: u32 = 0xFF5B_8DEF;

const BTN_REFRESH: i32 = 1;
const BTN_COPY: i32 = 2;
const BTN_CLOSE: i32 = 3;

struct PanelState {
    visible: bool,
    hover: i32,
    scale: f32,
    /// 面板是否真正拿到过焦点（拿不到就不靠失焦来收起）
    activated: bool,
    /// 弹出时刻，用于给“刚弹出就被判定失焦”留出宽限期
    shown_at: Option<std::time::Instant>,
}

static STATE: Mutex<PanelState> = Mutex::new(PanelState {
    visible: false,
    hover: 0,
    scale: 1.0,
    activated: false,
    shown_at: None,
});
static PANEL_HWND: AtomicIsize = AtomicIsize::new(0);
static CREATED: AtomicBool = AtomicBool::new(false);

fn class_name() -> &'static [u16] {
    Box::leak(ffi::wide("NetworkMonitorPanelWindow").into_boxed_slice())
}

fn with_state<F: FnOnce(&mut PanelState) -> R, R>(f: F) -> R {
    let mut guard = match STATE.lock() {
        Ok(value) => value,
        Err(poisoned) => poisoned.into_inner(),
    };
    f(&mut guard)
}

pub fn is_visible() -> bool {
    with_state(|state| state.visible)
}

/// 托盘图标被点击时调用：显示 / 收起。
pub fn toggle(_owner: HWND) {
    if is_visible() {
        hide();
    } else {
        show();
    }
}

/// 探测线程调用：告知面板重绘。
pub fn post_update() {
    let hwnd = PANEL_HWND.load(Ordering::Relaxed) as HWND;
    if !hwnd.is_null() {
        unsafe { win::PostMessageW(hwnd, win::WM_PANEL_UPDATE, 0, 0) };
    }
}

fn ensure_window() -> HWND {
    let existing = PANEL_HWND.load(Ordering::Relaxed) as HWND;
    if !existing.is_null() {
        return existing;
    }
    unsafe {
        let instance = win::GetModuleHandleW(std::ptr::null());
        if !CREATED.swap(true, Ordering::Relaxed) {
            let mut window_class: win::WNDCLASSEXW = std::mem::zeroed();
            window_class.cb_size = std::mem::size_of::<win::WNDCLASSEXW>() as u32;
            window_class.lpfn_wnd_proc = Some(window_proc);
            window_class.h_instance = instance;
            window_class.lpsz_class_name = class_name().as_ptr();
            window_class.h_cursor = win::LoadCursorW(std::ptr::null_mut(), win::IDC_ARROW as *const u16);
            win::RegisterClassExW(&window_class);
        }
        let hwnd = win::CreateWindowExW(
            win::WS_EX_LAYERED | win::WS_EX_TOOLWINDOW | win::WS_EX_TOPMOST,
            class_name().as_ptr(),
            class_name().as_ptr(),
            win::WS_POPUP,
            0,
            0,
            10,
            10,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null_mut(),
        );
        PANEL_HWND.store(hwnd as isize, Ordering::Relaxed);
        hwnd
    }
}

pub fn show() {
    let hwnd = ensure_window();
    if hwnd.is_null() {
        crate::log::warn("面板窗口创建失败");
        return;
    }
    let scale = unsafe {
        let dpi = win::GetDpiForWindow(hwnd);
        if dpi == 0 {
            1.0
        } else {
            dpi as f32 / 96.0
        }
    };
    with_state(|state| {
        state.scale = scale;
        state.visible = true;
        state.hover = 0;
        state.activated = false;
        state.shown_at = Some(std::time::Instant::now());
    });

    let (width, height) = panel_size(scale);
    let (x, y) = panel_position(width, height);
    crate::log::write(
        "INFO",
        &format!("面板已显示 {}x{} @ ({},{}) dpi-scale {:.2}", width, height, x, y, scale),
    );
    unsafe {
        win::SetWindowPos(
            hwnd,
            win::HWND_TOPMOST,
            x,
            y,
            width,
            height,
            win::SWP_SHOWWINDOW,
        );
        // 抢前台：先把本线程挂到当前前台线程上，否则 SetForegroundWindow 会被系统拒绝，
        // 之后“点击外部自动收起”就永远收不到 WM_ACTIVATE(WA_INACTIVE)
        let foreground = win::GetForegroundWindow();
        let foreground_thread = win::GetWindowThreadProcessId(foreground, std::ptr::null_mut());
        let current_thread = win::GetCurrentThreadId();
        let attached = foreground_thread != 0
            && foreground_thread != current_thread
            && win::AttachThreadInput(foreground_thread, current_thread, 1) != 0;
        win::SetForegroundWindow(hwnd);
        win::SetFocus(hwnd);
        if attached {
            win::AttachThreadInput(foreground_thread, current_thread, 0);
        }
    }
    render(hwnd);
}

pub fn hide() {
    let hwnd = PANEL_HWND.load(Ordering::Relaxed) as HWND;
    let was_visible = with_state(|state| {
        let previous = state.visible;
        state.visible = false;
        previous
    });
    if !hwnd.is_null() {
        unsafe { win::ShowWindow(hwnd, win::SW_HIDE) };
    }
    if was_visible {
        crate::log::write("INFO", "面板已隐藏");
    }
}

fn panel_size(scale: f32) -> (i32, i32) {
    let height = HEADER_H + 3 * (CARD_H + CARD_GAP) + FOOTER_H;
    (
        (WIDTH as f32 * scale).round() as i32,
        (height as f32 * scale).round() as i32,
    )
}

fn panel_position(width: i32, height: i32) -> (i32, i32) {
    let mut area = RECT::default();
    unsafe {
        win::SystemParametersInfoW(
            win::SPI_GETWORKAREA,
            0,
            &mut area as *mut RECT as *mut c_void,
            0,
        )
    };
    let right = if area.right == 0 { 1280 } else { area.right };
    let bottom = if area.bottom == 0 { 720 } else { area.bottom };
    (
        (right - width - (12.0 * (width as f32 / WIDTH as f32)) as i32).max(0),
        (bottom - height - 8).max(0),
    )
}

fn level_color(level: Level) -> u32 {
    match level {
        Level::Good => gdiplus::COLOR_GOOD,
        Level::Warn => gdiplus::COLOR_WARN,
        Level::Bad => gdiplus::COLOR_BAD,
        _ => gdiplus::COLOR_OFF,
    }
}

/// 画一张面板位图并提交给分层窗口
fn render(hwnd: HWND) {
    let snapshot = match tray::current_snapshot() {
        Some(value) => value,
        None => Snapshot::placeholder(&crate::config::Config::default()),
    };
    let scale = with_state(|state| state.scale);
    let (width, height) = panel_size(scale);

    unsafe {
        let screen_dc = win::GetDC(std::ptr::null_mut());
        let memory_dc = win32_create_compatible_dc(screen_dc);
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.header.bi_size = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.header.bi_width = width;
        info.header.bi_height = -height; // 自上而下
        info.header.bi_planes = 1;
        info.header.bi_bit_count = 32;
        info.header.bi_compression = win::BI_RGB;
        let mut bits: *mut c_void = std::ptr::null_mut();
        let bitmap: HBITMAP = win::CreateDIBSection(
            screen_dc,
            &info,
            win::DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        if bitmap.is_null() || bits.is_null() {
            win::ReleaseDC(std::ptr::null_mut(), screen_dc);
            return;
        }
        let previous = win::SelectObject(memory_dc, bitmap);

        // 用预乘 alpha 的 GDI+ 位图包住这块像素内存，直接画
        let surface = gdiplus::Bitmap::from_bits(
            width,
            height,
            width * 4,
            gdiplus::PIXEL_FORMAT_32BPP_PARGB,
            bits as *mut u32,
        );
        if let Some(surface) = surface.as_ref() {
            if let Some(graphics) = gdiplus::Graphics::from_bitmap(surface.raw()) {
                draw_panel(&graphics, &snapshot, scale, width, height);
            }
        }
        drop(surface);

        let size = SIZE {
            cx: width,
            cy: height,
        };
        let source = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            blend_op: win::AC_SRC_OVER,
            blend_flags: 0,
            source_constant_alpha: 255,
            alpha_format: win::AC_SRC_ALPHA,
        };
        let mut rect = RECT::default();
        win::GetWindowRect(hwnd, &mut rect);
        let window_position = POINT {
            x: rect.left,
            y: rect.top,
        };
        win::UpdateLayeredWindow(
            hwnd,
            screen_dc,
            &window_position,
            &size,
            memory_dc,
            &source,
            0,
            &blend,
            win::ULW_ALPHA,
        );

        win::SelectObject(memory_dc, previous);
        win::DeleteObject(bitmap);
        win32_delete_dc(memory_dc);
        win::ReleaseDC(std::ptr::null_mut(), screen_dc);
    }
}

fn win32_create_compatible_dc(reference: HDC) -> HDC {
    unsafe { win::CreateCompatibleDC(reference) }
}

fn win32_delete_dc(dc: HDC) {
    unsafe { win::DeleteDC(dc) };
}

fn draw_panel(graphics: &gdiplus::Graphics, snapshot: &Snapshot, scale: f32, width: i32, height: i32) {
    let s = |value: i32| (value as f32 * scale).round() as i32;
    let hover = with_state(|state| state.hover);

    graphics.clear(0);
    graphics.fill_round_rect(0, 0, width, height, s(RADIUS), BG);
    graphics.stroke_round_rect(0, 0, width, height, s(RADIUS), scale.max(1.0), BORDER);

    // 标题栏
    graphics.draw_text(
        "网络状态",
        s(14) as f32,
        s(11) as f32,
        14.0 * scale,
        true,
        gdiplus::ALIGN_NEAR,
        200.0 * scale,
        TEXT,
    );
    let clock = format!(
        "更新于 {}",
        crate::ffi::win::local_clock_string()
    );
    graphics.draw_text(
        &clock,
        s(WIDTH - 34) as f32 - 130.0 * scale,
        s(14) as f32,
        10.5 * scale,
        false,
        gdiplus::ALIGN_NEAR,
        130.0 * scale,
        MUTED,
    );
    graphics.draw_text(
        "✕",
        s(WIDTH - 30) as f32,
        s(11) as f32,
        13.0 * scale,
        false,
        gdiplus::ALIGN_NEAR,
        30.0 * scale,
        if hover == BTN_CLOSE { TEXT } else { MUTED },
    );

    // 三张卡片
    for (index, channel) in snapshot.channels.iter().enumerate() {
        let top = s(HEADER_H + index as i32 * (CARD_H + CARD_GAP));
        graphics.fill_round_rect(
            s(PAD),
            top,
            s(WIDTH - PAD * 2),
            s(CARD_H),
            s(10),
            CARD,
        );
        let color = level_color(channel.level);

        // 状态点
        graphics.fill_ellipse(s(16), top + s(18), s(11), s(11), color);

        // 通道名
        graphics.draw_text(
            &channel.name,
            s(36) as f32,
            (top + s(12)) as f32,
            13.0 * scale,
            false,
            gdiplus::ALIGN_NEAR,
            200.0 * scale,
            TEXT,
        );

        // 大号延迟数字（右对齐）
        let number = match channel.latency_ms {
            Some(value) => format!("{:.0}", value),
            None => "—".to_string(),
        };
        graphics.draw_text(
            &number,
            s(WIDTH - PAD - 16) as f32 - 78.0 * scale,
            (top + s(9)) as f32,
            22.0 * scale,
            true,
            gdiplus::ALIGN_FAR,
            78.0 * scale,
            color,
        );
        if channel.latency_ms.is_some() {
            graphics.draw_text(
                "ms",
                s(WIDTH - PAD - 14) as f32 - 22.0 * scale,
                (top + s(20)) as f32,
                10.0 * scale,
                false,
                gdiplus::ALIGN_NEAR,
                22.0 * scale,
                MUTED,
            );
        }

        // 状态 · 说明
        let detail = if channel.detail.is_empty() {
            channel.status.clone()
        } else {
            format!("{} · {}", channel.status, channel.detail)
        };
        let clipped = truncate(graphics, &detail, 30, 10.5 * scale);
        graphics.draw_text(
            &clipped,
            s(36) as f32,
            (top + s(34)) as f32,
            10.5 * scale,
            false,
            gdiplus::ALIGN_NEAR,
            210.0 * scale,
            MUTED,
        );

        // 迷你波形
        draw_spark(graphics, channel, scale, top);
    }

    // 底部信息与按钮
    let footer_top = s(HEADER_H + 3 * (CARD_H + CARD_GAP)) + s(2);
    let mut info = Vec::new();
    if !snapshot.clash_meta.is_empty() {
        info.push(snapshot.clash_meta.clone());
    }
    if snapshot.proxy_server.is_empty() {
        info.push(format!(
            "系统代理 {}",
            if snapshot.proxy_enabled { "开启" } else { "关闭" }
        ));
    } else {
        info.push(format!("系统代理 {}", snapshot.proxy_server));
    }
    if let Some(url) = tray::bridge_url() {
        info.push(format!("接口 {}", url.trim_start_matches("http://")));
    }
    let info_text = truncate(graphics, &info.join(" · "), 46, 10.5 * scale);
    graphics.draw_text(
        &info_text,
        s(14) as f32,
        footer_top as f32,
        10.5 * scale,
        false,
        gdiplus::ALIGN_NEAR,
        300.0 * scale,
        MUTED,
    );

    let button_top = footer_top + s(20);
    let button_width = s(152);
    let button_height = s(26);
    draw_button(
        graphics,
        s(14),
        button_top,
        button_width,
        button_height,
        s(8),
        "立即刷新",
        hover == BTN_REFRESH,
        scale,
    );
    draw_button(
        graphics,
        s(14) + button_width + s(10),
        button_top,
        button_width,
        button_height,
        s(8),
        "复制报告",
        hover == BTN_COPY,
        scale,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_button(
    graphics: &gdiplus::Graphics,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    radius: i32,
    text: &str,
    hovered: bool,
    scale: f32,
) {
    graphics.fill_round_rect(
        x,
        y,
        width,
        height,
        radius,
        if hovered { BUTTON_ACTIVE } else { BUTTON },
    );
    let text_width = graphics.measure_text(text, 11.0 * scale, false);
    graphics.draw_text(
        text,
        (x + width / 2) as f32 - text_width / 2.0,
        y as f32 + (height as f32 - 15.0 * scale) / 2.0,
        11.0 * scale,
        false,
        gdiplus::ALIGN_NEAR,
        text_width + 4.0,
        if hovered { TEXT } else { ACCENT },
    );
}

fn draw_spark(
    graphics: &gdiplus::Graphics,
    channel: &crate::monitor::ChannelState,
    scale: f32,
    card_top: i32,
) {
    let s = |value: i32| (value as f32 * scale).round() as i32;
    let width = s(56);
    let height = s(16);
    let right = s(WIDTH - PAD - 16);
    let top = card_top + s(34);
    let history = &channel.history;
    if history.is_empty() {
        return;
    }
    let capacity = 14usize;
    let recent: Vec<Option<f64>> = history
        .iter()
        .rev()
        .take(capacity)
        .rev()
        .copied()
        .collect();
    let scale_max = recent
        .iter()
        .filter_map(|value| *value)
        .fold(50.0f64, f64::max);
    let color = level_color(channel.level);
    let step = width / capacity as i32;
    let bar_width = (step - s(1)).max(1);
    for (index, value) in recent.iter().enumerate() {
        let x = right - width + index as i32 * step;
        match value {
            None => {
                graphics.fill_rect(x, top + height - s(2), bar_width, s(2), DIM);
            }
            Some(ms) => {
                let bar = ((height as f64) * (ms.min(scale_max) / scale_max)).max(2.0) as i32;
                graphics.fill_rect(x, top + height - bar, bar_width, bar, color);
            }
        }
    }
}

/// 按字符宽度粗略截断，避免文字溢出卡片
fn truncate(graphics: &gdiplus::Graphics, text: &str, max_chars: usize, size: f32) -> String {
    let _ = graphics;
    let _ = size;
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let mut result: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    result.push('…');
    result
}

/// 命中测试：返回按钮编号
fn hit_test(x: i32, y: i32) -> i32 {
    let scale = with_state(|state| state.scale);
    let s = |value: i32| (value as f32 * scale).round() as i32;
    if x >= s(WIDTH - 40) && y <= s(HEADER_H) {
        return BTN_CLOSE;
    }
    let footer_top = s(HEADER_H + 3 * (CARD_H + CARD_GAP)) + s(2) + s(20);
    if y >= footer_top && y <= footer_top + s(26) {
        if x >= s(14) && x <= s(14) + s(152) {
            return BTN_REFRESH;
        }
        if x >= s(14) + s(152) + s(10) && x <= s(14) + s(152) * 2 + s(10) {
            return BTN_COPY;
        }
    }
    0
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    match message {
        win::WM_PANEL_UPDATE => {
            if is_visible() {
                render(hwnd);
            }
            0
        }
        win::WM_LBUTTONUP => {
            let x = (l_param & 0xFFFF) as u16 as i16 as i32;
            let y = ((l_param >> 16) & 0xFFFF) as u16 as i16 as i32;
            match hit_test(x, y) {
                BTN_CLOSE => hide(),
                BTN_REFRESH => {
                    if let Some(runtime) = tray::runtime_handle() {
                        runtime.kick.store(true, Ordering::Relaxed);
                    }
                }
                BTN_COPY => tray::copy_report(hwnd),
                _ => {}
            }
            0
        }
        win::WM_MOUSEMOVE => {
            let x = (l_param & 0xFFFF) as u16 as i16 as i32;
            let y = ((l_param >> 16) & 0xFFFF) as u16 as i16 as i32;
            let hover = hit_test(x, y);
            let changed = with_state(|state| {
                if state.hover != hover {
                    state.hover = hover;
                    true
                } else {
                    false
                }
            });
            if changed {
                let cursor = unsafe {
                    win::SetCursor(win::LoadCursorW(
                        std::ptr::null_mut(),
                        if hover != 0 {
                            win::IDC_HAND as *const u16
                        } else {
                            win::IDC_ARROW as *const u16
                        },
                    ))
                };
                let _ = cursor;
                render(hwnd);
            }
            0
        }
        win::WM_ACTIVATE => {
            let action = (w_param & 0xFFFF) as u16;
            if action == win::WA_INACTIVE {
                // 新建窗口被 ShowWindow 时会先收到一次 WA_INACTIVE，
                // 只有“拿到过焦点后的失焦”或超过宽限期才算真的点到外面了
                let should_hide = with_state(|state| {
                    if state.activated {
                        return true;
                    }
                    state
                        .shown_at
                        .map(|at| at.elapsed() > std::time::Duration::from_millis(400))
                        .unwrap_or(false)
                });
                if should_hide {
                    hide();
                }
            } else {
                with_state(|state| state.activated = true);
            }
            0
        }
        win::WM_KEYDOWN => {
            if w_param == win::VK_ESCAPE {
                hide();
            }
            0
        }
        win::WM_ERASEBKGND => 1,
        _ => win::DefWindowProcW(hwnd, message, w_param, l_param),
    }
}

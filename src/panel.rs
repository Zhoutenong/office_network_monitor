//! 状态面板：分层窗口（逐像素 alpha）+ GDI+ 绘制，点击托盘图标弹出。
//!
//! 交互：左键点托盘图标开合；点到面板以外或按 Esc 自动收起。
//! 面板上只有一个可点区域 —— 标题右侧的「复制报告」图标。

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Mutex;

use crate::ffi::win::{self, BLENDFUNCTION, BITMAPINFO, BITMAPINFOHEADER, POINT, RECT, SIZE};
use crate::ffi::{self, HBITMAP, HWND, LPARAM, LRESULT, WPARAM};
use crate::gdiplus;
use crate::monitor::{Level, Snapshot};
use crate::tray;

// 逻辑尺寸（96 DPI 下的像素），实际渲染按 DPI 缩放
const WIDTH: i32 = 344;
const RADIUS: i32 = 14;
const PAD: i32 = 10;
const HEADER_H: i32 = 40;
const CARD_H: i32 = 62;
const CARD_GAP: i32 = 6;
const FOOTER_H: i32 = 32;

const TITLE_SIZE: f32 = 14.0;
const CLOCK_SIZE: f32 = 11.0;
const NAME_SIZE: f32 = 13.0;
const NUMBER_SIZE: f32 = 22.0;
const UNIT_SIZE: f32 = 11.0;
const DETAIL_SIZE: f32 = 11.0;
const INFO_SIZE: f32 = 11.0;

const BG: u32 = 0xFF15_1A21;
const CARD: u32 = 0xFF1B_2130;
const BORDER: u32 = 0xFF2C_3648;
const TEXT: u32 = 0xFFE8_EDF5;
const MUTED: u32 = 0xFF8B_98AB;
const DIM: u32 = 0xFF3B_4557;
const BUTTON: u32 = 0xFF23_2C3C;
const ACCENT: u32 = 0xFF5B_8DEF;

const HIT_NONE: i32 = 0;
const HIT_COPY: i32 = 1;

/// 面板可见期间的心跳定时器：用于不依赖焦点的「点外部收起」
const TIMER_ID: usize = 1;
const TIMER_INTERVAL_MS: u32 = 100;
/// 弹出后多久开始判定「点击了面板外」（避开弹出那一下自身的按键）
const OUTSIDE_CLICK_GRACE_MS: u128 = 350;

struct PanelState {
    visible: bool,
    hover: i32,
    scale: f32,
    /// 复制图标在窗口内的位置（渲染时算好，命中测试直接用）
    copy_rect: (i32, i32, i32, i32),
    /// 面板是否真正拿到过焦点（拿不到就不靠失焦来收起）
    activated: bool,
    shown_at: Option<std::time::Instant>,
}

static STATE: Mutex<PanelState> = Mutex::new(PanelState {
    visible: false,
    hover: HIT_NONE,
    scale: 1.0,
    copy_rect: (0, 0, 0, 0),
    activated: false,
    shown_at: None,
});

static PANEL_HWND: AtomicIsize = AtomicIsize::new(0);
static CREATED: AtomicBool = AtomicBool::new(false);
static FIRST_SHOW_LOGGED: AtomicBool = AtomicBool::new(false);

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
            window_class.h_cursor =
                win::LoadCursorW(std::ptr::null_mut(), win::IDC_ARROW as *const u16);
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
        state.hover = HIT_NONE;
        state.activated = false;
        state.shown_at = Some(std::time::Instant::now());
    });

    let (width, height) = panel_size(scale);
    let (x, y) = panel_position(width, height);
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
        // 「点击外部自动收起」就永远收不到 WM_ACTIVATE(WA_INACTIVE)
        let foreground = win::GetForegroundWindow();
        let foreground_thread = win::GetWindowThreadProcessId(foreground, std::ptr::null_mut());
        let current_thread = win::GetCurrentThreadId();
        let attached = foreground_thread != 0
            && foreground_thread != current_thread
            && win::AttachThreadInput(foreground_thread, current_thread, 1) != 0;
        win::BringWindowToTop(hwnd);
        let activated = win::SetForegroundWindow(hwnd);
        win::SetFocus(hwnd);
        if attached {
            win::AttachThreadInput(foreground_thread, current_thread, 0);
        }
        crate::log::write(
            "INFO",
            &format!(
                "抢前台: SetForegroundWindow={} 已挂靠={} 当前前台是本窗口={}",
                activated,
                attached,
                win::GetForegroundWindow() == hwnd
            ),
        );
    }
    // 心跳定时器：托盘点击不一定能拿到前台焦点，靠它兜底判断「点了面板外」
    unsafe { win::SetTimer(hwnd, TIMER_ID, TIMER_INTERVAL_MS, std::ptr::null_mut()) };
    render(hwnd);
    if !FIRST_SHOW_LOGGED.swap(true, Ordering::Relaxed) {
        crate::log::write(
            "INFO",
            &format!("面板参数 {}x{} @ ({},{}) 缩放 {:.2}", width, height, x, y, scale),
        );
    }
}

pub fn hide() {
    let hwnd = PANEL_HWND.load(Ordering::Relaxed) as HWND;
    let was_visible = with_state(|state| {
        let previous = state.visible;
        state.visible = false;
        previous
    });
    if !hwnd.is_null() {
        unsafe {
            win::KillTimer(hwnd, TIMER_ID);
            win::ShowWindow(hwnd, win::SW_HIDE);
        }
    }
    if was_visible {
        crate::log::write("INFO", "面板已隐藏");
    }
}

/// 心跳检查：托盘点击拿不到前台焦点时，靠这个判断「用户点了面板以外」
fn check_outside_click(hwnd: HWND) {
    let (visible, activated, elapsed_ms) = with_state(|state| {
        (
            state.visible,
            state.activated,
            state
                .shown_at
                .map(|at| at.elapsed().as_millis())
                .unwrap_or(u128::MAX),
        )
    });
    if !visible {
        return;
    }

    let foreground = unsafe { win::GetForegroundWindow() };
    if foreground == hwnd {
        with_state(|state| state.activated = true);
    }

    if elapsed_ms < OUTSIDE_CLICK_GRACE_MS {
        return;
    }

    // 主判据：光标在面板外、且左键按下 —— 覆盖点桌面/任务栏这类"不抢焦点"的点击。
    // 不吞掉这次点击，用户的操作照常生效。
    let mut cursor = POINT::default();
    unsafe { win::GetCursorPos(&mut cursor) };
    let mut rect = RECT::default();
    unsafe { win::GetWindowRect(hwnd, &mut rect) };
    let inside = cursor.x >= rect.left
        && cursor.x < rect.right
        && cursor.y >= rect.top
        && cursor.y < rect.bottom;
    if !inside {
        let state = unsafe { win::GetAsyncKeyState(win::VK_LBUTTON) } as u16;
        // 同时看「当前按下」和「自上次询问后被按过」，避免点击快于轮询间隔时漏判
        if state & (win::KEY_DOWN_MASK | win::KEY_PRESSED_SINCE_LAST) != 0 {
            hide();
            return;
        }
    }

    // 辅助判据：曾拿到过焦点、现在前台换成别的窗口（比如被通知/其他程序抢走）
    if activated && foreground != hwnd {
        hide();
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
    let margin = (12.0 * (width as f32 / WIDTH as f32)).round() as i32;
    ((right - width - margin).max(0), (bottom - height - 8).max(0))
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
        let memory_dc = win::CreateCompatibleDC(screen_dc);
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

        // 预乘 alpha 的 GDI+ 位图直接包住这块像素内存
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
        win::DeleteDC(memory_dc);
        win::ReleaseDC(std::ptr::null_mut(), screen_dc);
    }
}

fn draw_panel(graphics: &gdiplus::Graphics, snapshot: &Snapshot, scale: f32, width: i32, height: i32) {
    // 所有坐标与字号都取整，避免文字落在半个像素上发虚
    let s = |value: i32| (value as f32 * scale).round() as i32;
    let f = |value: f32| (value * scale).round();
    let hover = with_state(|state| state.hover);

    graphics.clear(0);
    graphics.fill_round_rect(0, 0, width, height, s(RADIUS), BG);
    graphics.stroke_round_rect(0, 0, width, height, s(RADIUS), scale.max(1.0), BORDER);

    // ---- 标题栏：标题 + 复制图标 + 更新时间 ----
    let title = "网络状态";
    graphics.draw_text(
        title,
        s(14) as f32,
        s(12) as f32,
        f(TITLE_SIZE),
        true,
        gdiplus::ALIGN_NEAR,
        200.0 * scale,
        TEXT,
    );
    let title_width = graphics.measure_text(title, f(TITLE_SIZE), true);
    let icon_size = s(15);
    let icon_x = s(14) + title_width.round() as i32 + s(10);
    let icon_y = s(13);
    with_state(|state| state.copy_rect = (icon_x - s(4), icon_y - s(4), icon_size + s(8), icon_size + s(8)));
    draw_copy_icon(
        graphics,
        icon_x,
        icon_y,
        icon_size,
        s(6),
        hover == HIT_COPY,
        scale,
    );

    let clock = format!("更新于 {}", crate::ffi::win::local_clock_string());
    graphics.draw_text_ex(
        &clock,
        0.0,
        s(14) as f32,
        f(CLOCK_SIZE),
        false,
        gdiplus::ALIGN_FAR,
        s(WIDTH - 14) as f32,
        MUTED,
        true,
    );

    // ---- 三张卡片 ----
    let card_width = s(WIDTH - PAD * 2);
    for (index, channel) in snapshot.channels.iter().enumerate() {
        let top = s(HEADER_H + index as i32 * (CARD_H + CARD_GAP));
        graphics.fill_round_rect(s(PAD), top, card_width, s(CARD_H), s(10), CARD);
        let color = level_color(channel.level);

        graphics.fill_ellipse(s(16), top + s(19), s(11), s(11), color);

        // 右侧：延迟数字 + 单位，按测量结果贴在一起右对齐，杜绝重叠
        let number = match channel.latency_ms {
            Some(value) => format!("{:.0}", value),
            None => "—".to_string(),
        };
        let has_value = channel.latency_ms.is_some();
        let number_width = graphics.measure_text_ex(&number, f(NUMBER_SIZE), true, true);
        let unit_width = if has_value {
            graphics.measure_text("ms", f(UNIT_SIZE), false)
        } else {
            0.0
        };
        let gap = s(4) as f32;
        let group_right = s(WIDTH - PAD - 16) as f32;
        let group_width = number_width + if has_value { gap + unit_width } else { 0.0 };
        let number_x = group_right - group_width;

        graphics.draw_text_ex(
            &number,
            number_x,
            (top + s(11)) as f32,
            f(NUMBER_SIZE),
            true,
            gdiplus::ALIGN_NEAR,
            number_width + 4.0,
            color,
            true,
        );
        if has_value {
            graphics.draw_text(
                "ms",
                number_x + number_width + gap,
                (top + s(21)) as f32,
                f(UNIT_SIZE),
                false,
                gdiplus::ALIGN_NEAR,
                unit_width + 4.0,
                MUTED,
            );
        }

        // 通道名（过长时按实际宽度截断）
        let name_width = (number_x - s(36) as f32 - 8.0).max(40.0);
        let name = fit_text(
            graphics,
            &channel.name,
            name_width,
            f(NAME_SIZE),
            false,
            false,
        );
        graphics.draw_text(
            &name,
            s(36) as f32,
            (top + s(13)) as f32,
            f(NAME_SIZE),
            false,
            gdiplus::ALIGN_NEAR,
            name_width,
            TEXT,
        );

        // 状态 · 说明
        let detail = if channel.detail.is_empty() {
            channel.status.clone()
        } else {
            format!("{} · {}", channel.status, channel.detail)
        };
        let detail_width = (card_width - s(36 - PAD) - s(74)) as f32;
        let detail = fit_text(
            graphics,
            &detail,
            detail_width,
            f(DETAIL_SIZE),
            false,
            false,
        );
        graphics.draw_text(
            &detail,
            s(36) as f32,
            (top + s(35)) as f32,
            f(DETAIL_SIZE),
            false,
            gdiplus::ALIGN_NEAR,
            detail_width,
            MUTED,
        );

        draw_spark(graphics, channel, scale, top);
    }

    // ---- 底部信息 ----
    let footer_top = s(HEADER_H + 3 * (CARD_H + CARD_GAP)) + s(9);
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
    let info_text = fit_text(
        graphics,
        &info.join(" · "),
        s(WIDTH - 28) as f32,
        f(INFO_SIZE),
        false,
        false,
    );
    graphics.draw_text(
        &info_text,
        s(14) as f32,
        footer_top as f32,
        f(INFO_SIZE),
        false,
        gdiplus::ALIGN_NEAR,
        s(WIDTH - 28) as f32,
        MUTED,
    );
}

/// 「复制」图标：两张错位的圆角纸片
fn draw_copy_icon(
    graphics: &gdiplus::Graphics,
    x: i32,
    y: i32,
    size: i32,
    radius: i32,
    hovered: bool,
    scale: f32,
) {
    if hovered {
        let pad = (4.0 * scale).round() as i32;
        graphics.fill_round_rect(
            x - pad,
            y - pad,
            size + pad * 2,
            size + pad * 2,
            (7.0 * scale).round() as i32,
            BUTTON,
        );
    }
    let color = if hovered { ACCENT } else { MUTED };
    let thickness = scale.max(1.0);
    let offset = (size as f32 * 0.22).round() as i32;
    let sheet = size - offset;

    graphics.stroke_round_rect(x + offset, y, sheet, sheet, radius, thickness, color);
    graphics.fill_round_rect(x, y + offset, sheet, sheet, radius, BG);
    graphics.stroke_round_rect(x, y + offset, sheet, sheet, radius, thickness, color);
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
    let top = card_top + s(36);
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
            None => graphics.fill_rect(x, top + height - s(2), bar_width, s(2), DIM),
            Some(ms) => {
                let bar = ((height as f64) * (ms.min(scale_max) / scale_max)).max(2.0) as i32;
                graphics.fill_rect(x, top + height - bar, bar_width, bar, color);
            }
        }
    }
}

/// 按真实宽度截断（二分查找，避免逐字符测量）
fn fit_text(
    graphics: &gdiplus::Graphics,
    text: &str,
    max_width: f32,
    size: f32,
    bold: bool,
    numeric: bool,
) -> String {
    if graphics.measure_text_ex(text, size, bold, numeric) <= max_width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut low = 1usize;
    let mut high = chars.len();
    let mut best = String::from("…");
    while low <= high {
        let middle = (low + high) / 2;
        let candidate = format!("{}…", chars[..middle].iter().collect::<String>());
        if graphics.measure_text_ex(&candidate, size, bold, numeric) <= max_width {
            best = candidate;
            low = middle + 1;
        } else {
            if middle == 0 {
                break;
            }
            high = middle - 1;
        }
    }
    best
}

/// 命中测试：目前只有标题旁的复制图标可点
fn hit_test(x: i32, y: i32) -> i32 {
    let (left, top, width, height) = with_state(|state| state.copy_rect);
    if width > 0 && x >= left && x <= left + width && y >= top && y <= top + height {
        return HIT_COPY;
    }
    HIT_NONE
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    match message {
        win::WM_TIMER => {
            if w_param == TIMER_ID {
                check_outside_click(hwnd);
            }
            0
        }
        win::WM_PANEL_UPDATE => {
            if is_visible() {
                render(hwnd);
            }
            0
        }
        win::WM_LBUTTONUP => {
            let x = (l_param & 0xFFFF) as u16 as i16 as i32;
            let y = ((l_param >> 16) & 0xFFFF) as u16 as i16 as i32;
            if hit_test(x, y) == HIT_COPY {
                tray::copy_report(hwnd);
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
                win::SetCursor(win::LoadCursorW(
                    std::ptr::null_mut(),
                    if hover != HIT_NONE {
                        win::IDC_HAND as *const u16
                    } else {
                        win::IDC_ARROW as *const u16
                    },
                ));
                render(hwnd);
            }
            0
        }
        win::WM_ACTIVATE => {
            let action = (w_param & 0xFFFF) as u16;
            crate::log::write("INFO", &format!("收到 WM_ACTIVATE action={}", action));
            if action == win::WA_INACTIVE {
                // 新建窗口被 ShowWindow 时会先收到一次 WA_INACTIVE，
                // 只有「拿到过焦点后的失焦」或超过宽限期才算真的点到外面了
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
        win::WM_KILLFOCUS => {
            crate::log::write("INFO", "收到 WM_KILLFOCUS");
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

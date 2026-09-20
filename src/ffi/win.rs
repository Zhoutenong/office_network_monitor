//! kernel32 / user32 / shell32 / gdi32 的声明。

use super::*;
use std::ffi::c_void;

// ---------------------------------------------------------------- 常量
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_ACTIVATE: u32 = 0x0006;
pub const WM_KILLFOCUS: u32 = 0x0008;
pub const WM_PAINT: u32 = 0x000F;
pub const WM_COMMAND: u32 = 0x0111;
pub const WM_TIMER: u32 = 0x0113;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_RBUTTONUP: u32 = 0x0205;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_APP: u32 = 0x8000;
pub const TRAY_CALLBACK_MESSAGE: u32 = WM_APP + 1;
pub const WM_TRAY_UPDATE: u32 = WM_APP + 2;
pub const WM_TRAY_PANEL: u32 = WM_APP + 3;
pub const WM_TRAY_QUIT: u32 = WM_APP + 4;
pub const WM_TRAY_COPY: u32 = WM_APP + 5;
pub const WM_TRAY_RELOAD: u32 = WM_APP + 6;
pub const WM_PANEL_UPDATE: u32 = WM_APP + 20;
pub const WM_SETCURSOR: u32 = 0x0020;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_NCHITTEST: u32 = 0x0084;
pub const WA_INACTIVE: u16 = 0;
pub const VK_ESCAPE: usize = 0x1B;
pub const VK_LBUTTON: i32 = 0x01;
/// GetAsyncKeyState 的"当前按下"位
pub const KEY_DOWN_MASK: u16 = 0x8000;
/// GetAsyncKeyState 的"自上次调用以来被按过"位：用来抓住短于轮询间隔的点击
pub const KEY_PRESSED_SINCE_LAST: u16 = 0x0001;
pub const IDC_HAND: usize = 32649;

pub const WS_POPUP: u32 = 0x8000_0000;
pub const WS_EX_TOPMOST: u32 = 0x0000_0008;
pub const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
pub const WS_EX_LAYERED: u32 = 0x0008_0000;
pub const WS_EX_NOACTIVATE: u32 = 0x0800_0000;

pub const SW_SHOW: i32 = 5;
pub const SW_HIDE: i32 = 0;
pub const SWP_NOSIZE: u32 = 0x0001;
pub const SWP_NOMOVE: u32 = 0x0002;
pub const SWP_NOZORDER: u32 = 0x0004;
pub const SWP_SHOWWINDOW: u32 = 0x0040;

pub const NIM_ADD: u32 = 0;
pub const NIM_MODIFY: u32 = 1;
pub const NIM_DELETE: u32 = 2;
pub const NIF_MESSAGE: u32 = 0x0000_0001;
pub const NIF_ICON: u32 = 0x0000_0002;
pub const NIF_TIP: u32 = 0x0000_0004;

pub const TPM_RIGHTALIGN: u32 = 0x0008;
pub const TPM_BOTTOMALIGN: u32 = 0x0020;
pub const TPM_RETURNCMD: u32 = 0x0100;
pub const TPM_RIGHTBUTTON: u32 = 0x0002;
pub const MF_STRING: u32 = 0x0000_0000;
pub const MF_SEPARATOR: u32 = 0x0000_0800;
pub const MF_GRAYED: u32 = 0x0000_0001;

pub const SPI_GETWORKAREA: u32 = 0x0030;
pub const ULW_ALPHA: u32 = 0x0000_0002;
pub const AC_SRC_OVER: u8 = 0x00;
pub const AC_SRC_ALPHA: u8 = 0x01;
pub const BI_RGB: u32 = 0;
pub const DIB_RGB_COLORS: u32 = 0;
pub const DEFAULT_GUI_FONT: i32 = 17;
pub const IDC_ARROW: usize = 32512;
pub const MB_OK: u32 = 0x0000_0000;
pub const MB_ICONINFORMATION: u32 = 0x0000_0040;
pub const CF_UNICODETEXT: u32 = 13;
pub const GMEM_MOVEABLE: u32 = 0x0002;
pub const HWND_TOPMOST: HWND = -1isize as HWND;
/// DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
pub const DPI_PER_MONITOR_V2: isize = -4;

// ---------------------------------------------------------------- 结构体
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SIZE {
    pub cx: i32,
    pub cy: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: u32,
    pub w_param: WPARAM,
    pub l_param: LPARAM,
    pub time: u32,
    pub pt: POINT,
    pub private: u32,
}

#[repr(C)]
pub struct GUID {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

pub type WNDPROC = Option<unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT>;

#[repr(C)]
pub struct WNDCLASSEXW {
    pub cb_size: u32,
    pub style: u32,
    pub lpfn_wnd_proc: WNDPROC,
    pub cb_cls_extra: i32,
    pub cb_wnd_extra: i32,
    pub h_instance: HMODULE,
    pub h_icon: HICON,
    pub h_cursor: HCURSOR,
    pub hbr_background: HBRUSH,
    pub lpsz_menu_name: *const u16,
    pub lpsz_class_name: *const u16,
    pub h_icon_sm: HICON,
}

#[repr(C)]
pub struct NOTIFYICONDATAW {
    pub cb_size: u32,
    pub h_wnd: HWND,
    pub u_id: u32,
    pub u_flags: u32,
    pub u_callback_message: u32,
    pub h_icon: HICON,
    pub sz_tip: [u16; 128],
    pub dw_state: u32,
    pub dw_state_mask: u32,
    pub sz_info: [u16; 256],
    pub u_version: u32,
    pub sz_info_title: [u16; 64],
    pub dw_info_flags: u32,
    pub guid_item: GUID,
    pub h_balloon_icon: HICON,
}

impl Default for NOTIFYICONDATAW {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BLENDFUNCTION {
    pub blend_op: u8,
    pub blend_flags: u8,
    pub source_constant_alpha: u8,
    pub alpha_format: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BITMAPINFOHEADER {
    pub bi_size: u32,
    pub bi_width: i32,
    pub bi_height: i32,
    pub bi_planes: u16,
    pub bi_bit_count: u16,
    pub bi_compression: u32,
    pub bi_size_image: u32,
    pub bi_x_pels_per_meter: i32,
    pub bi_y_pels_per_meter: i32,
    pub bi_clr_used: u32,
    pub bi_clr_important: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RGBQUAD {
    pub blue: u8,
    pub green: u8,
    pub red: u8,
    pub reserved: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BITMAPINFO {
    pub header: BITMAPINFOHEADER,
    pub colors: [RGBQUAD; 1],
}

pub const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
pub const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SYSTEMTIME {
    pub year: u16,
    pub month: u16,
    pub day_of_week: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    pub milliseconds: u16,
}

/// 当前本地时间（yyyy-MM-dd HH:mm:ss），供报告使用。
pub fn local_time_string() -> String {
    let mut time = SYSTEMTIME::default();
    unsafe { GetLocalTime(&mut time) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        time.year, time.month, time.day, time.hour, time.minute, time.second
    )
}

pub fn local_clock_string() -> String {
    let mut time = SYSTEMTIME::default();
    unsafe { GetLocalTime(&mut time) };
    format!("{:02}:{:02}:{:02}", time.hour, time.minute, time.second)
}

#[repr(C)]
pub struct PROCESSENTRY32W {
    pub dw_size: u32,
    pub cnt_usage: u32,
    pub th32_process_id: u32,
    pub th32_default_heap_id: usize,
    pub th32_module_id: u32,
    pub cnt_threads: u32,
    pub th32_parent_process_id: u32,
    pub pc_pri_class_base: i32,
    pub dw_flags: u32,
    pub sz_exe_file: [u16; 260],
}

// ---------------------------------------------------------------- kernel32
#[link(name = "kernel32")]
extern "system" {
    pub fn GetModuleHandleW(name: *const u16) -> HMODULE;
    pub fn CreateMutexW(attrs: *mut c_void, initial_owner: BOOL, name: *const u16) -> HANDLE;
    pub fn CloseHandle(handle: HANDLE) -> BOOL;
    pub fn GlobalAlloc(flags: UINT, bytes: usize) -> HANDLE;
    pub fn GlobalLock(mem: HANDLE) -> *mut c_void;
    pub fn GlobalUnlock(mem: HANDLE) -> BOOL;
    pub fn GlobalFree(mem: HANDLE) -> HANDLE;
    pub fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> HANDLE;
    pub fn GetLocalTime(time: *mut SYSTEMTIME);
    pub fn GetCurrentThreadId() -> u32;
    pub fn Process32FirstW(snapshot: HANDLE, entry: *mut PROCESSENTRY32W) -> BOOL;
    pub fn Process32NextW(snapshot: HANDLE, entry: *mut PROCESSENTRY32W) -> BOOL;
}

// ---------------------------------------------------------------- user32
#[link(name = "user32")]
extern "system" {
    pub fn RegisterClassExW(class: *const WNDCLASSEXW) -> u16;
    pub fn CreateWindowExW(
        ex_style: DWORD,
        class: *const u16,
        title: *const u16,
        style: DWORD,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: HWND,
        menu: HMENU,
        instance: HMODULE,
        param: *mut c_void,
    ) -> HWND;
    pub fn DefWindowProcW(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT;
    pub fn DestroyWindow(hwnd: HWND) -> BOOL;
    pub fn GetMessageW(msg: *mut MSG, hwnd: HWND, min: u32, max: u32) -> BOOL;
    pub fn TranslateMessage(msg: *const MSG) -> BOOL;
    pub fn DispatchMessageW(msg: *const MSG) -> LRESULT;
    pub fn PostQuitMessage(code: i32);
    pub fn PostMessageW(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> BOOL;
    pub fn SetForegroundWindow(hwnd: HWND) -> BOOL;
    pub fn GetCursorPos(point: *mut POINT) -> BOOL;
    pub fn CreatePopupMenu() -> HMENU;
    pub fn AppendMenuW(menu: HMENU, flags: UINT, id: usize, item: *const u16) -> BOOL;
    pub fn TrackPopupMenu(
        menu: HMENU,
        flags: UINT,
        x: i32,
        y: i32,
        reserved: i32,
        hwnd: HWND,
        rect: *const RECT,
    ) -> BOOL;
    pub fn DestroyMenu(menu: HMENU) -> BOOL;
    pub fn SystemParametersInfoW(action: UINT, param: UINT, data: *mut c_void, win_ini: UINT) -> BOOL;
    pub fn MessageBoxW(hwnd: HWND, text: *const u16, caption: *const u16, flags: UINT) -> i32;
    pub fn SetProcessDPIAware() -> BOOL;
    pub fn SetProcessDpiAwarenessContext(ctx: *mut c_void) -> BOOL;
    pub fn GetDpiForWindow(hwnd: HWND) -> u32;
    pub fn ScreenToClient(hwnd: HWND, point: *mut POINT) -> BOOL;
    pub fn SetCursor(cursor: HCURSOR) -> HCURSOR;
    pub fn RegisterWindowMessageW(text: *const u16) -> u32;
    pub fn GetSystemMetrics(index: i32) -> i32;
    pub fn ShowWindow(hwnd: HWND, cmd: i32) -> BOOL;
    pub fn SetWindowPos(
        hwnd: HWND,
        after: HWND,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: UINT,
    ) -> BOOL;
    pub fn UpdateLayeredWindow(
        hwnd: HWND,
        dst_dc: HDC,
        dst: *const POINT,
        size: *const SIZE,
        src_dc: HDC,
        src: *const POINT,
        color_key: u32,
        blend: *const BLENDFUNCTION,
        flags: DWORD,
    ) -> BOOL;
    pub fn GetDC(hwnd: HWND) -> HDC;
    pub fn ReleaseDC(hwnd: HWND, dc: HDC) -> i32;
    pub fn LoadCursorW(instance: HMODULE, name: *const u16) -> HCURSOR;
    pub fn SetCapture(hwnd: HWND) -> HWND;
    pub fn ReleaseCapture() -> BOOL;
    pub fn GetFocus() -> HWND;
    pub fn SetFocus(hwnd: HWND) -> HWND;
    pub fn GetForegroundWindow() -> HWND;
    pub fn BringWindowToTop(hwnd: HWND) -> BOOL;
    pub fn GetAsyncKeyState(key: i32) -> i16;
    pub fn GetWindowThreadProcessId(hwnd: HWND, process_id: *mut u32) -> u32;
    pub fn AttachThreadInput(attach: u32, attach_to: u32, do_attach: BOOL) -> BOOL;
    pub fn DestroyIcon(icon: HICON) -> BOOL;
    pub fn InvalidateRect(hwnd: HWND, rect: *const RECT, erase: BOOL) -> BOOL;
    pub fn SetTimer(hwnd: HWND, id: usize, elapse: u32, func: *mut c_void) -> usize;
    pub fn KillTimer(hwnd: HWND, id: usize) -> BOOL;
    pub fn GetClientRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
    pub fn GetWindowRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
    pub fn OpenClipboard(hwnd: HWND) -> BOOL;
    pub fn EmptyClipboard() -> BOOL;
    pub fn SetClipboardData(format: UINT, mem: HANDLE) -> HANDLE;
    pub fn CloseClipboard() -> BOOL;
}

// ---------------------------------------------------------------- gdi32
#[link(name = "gdi32")]
extern "system" {
    pub fn CreateCompatibleDC(dc: HDC) -> HDC;
    pub fn DeleteDC(dc: HDC) -> BOOL;
    pub fn CreateDIBSection(
        dc: HDC,
        info: *const BITMAPINFO,
        usage: UINT,
        bits: *mut *mut c_void,
        section: HANDLE,
        offset: DWORD,
    ) -> HBITMAP;
    pub fn SelectObject(dc: HDC, obj: HANDLE) -> HANDLE;
    pub fn DeleteObject(obj: HANDLE) -> BOOL;
    pub fn GetStockObject(index: i32) -> HANDLE;
    pub fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: DWORD,
        underline: DWORD,
        strike_out: DWORD,
        charset: DWORD,
        out_precision: DWORD,
        clip_precision: DWORD,
        quality: DWORD,
        pitch_and_family: DWORD,
        face: *const u16,
    ) -> HFONT;
    pub fn TextOutW(dc: HDC, x: i32, y: i32, text: *const u16, count: i32) -> BOOL;
    pub fn GetTextExtentPoint32W(dc: HDC, text: *const u16, count: i32, size: *mut SIZE) -> BOOL;
}

// ---------------------------------------------------------------- shell32
#[link(name = "shell32")]
extern "system" {
    pub fn Shell_NotifyIconW(message: DWORD, data: *const NOTIFYICONDATAW) -> BOOL;
}

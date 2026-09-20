//! GDI+ 极简封装：画托盘图标与面板（圆角、抗锯齿）。

#![allow(dead_code)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::ffi::{self, HDC, HICON};

type GpStatus = i32;
type GpGraphics = *mut c_void;
type GpBrush = *mut c_void;
type GpPen = *mut c_void;
type GpPath = *mut c_void;
type GpBitmap = *mut c_void;
type GpImage = *mut c_void;
type GpFontFamily = *mut c_void;
type GpFont = *mut c_void;
type GpStringFormat = *mut c_void;

pub const UNIT_PIXEL: i32 = 2;
pub const SMOOTHING_ANTIALIAS: i32 = 4;
pub const TEXT_ANTIALIAS: i32 = 4;
pub const ALIGN_NEAR: i32 = 0;
pub const ALIGN_CENTER: i32 = 1;
pub const ALIGN_FAR: i32 = 2;
pub const FONT_STYLE_REGULAR: i32 = 0;
pub const FONT_STYLE_BOLD: i32 = 1;
pub const PIXEL_FORMAT_32BPP_ARGB: i32 = 0x0026_200A;
pub const PIXEL_FORMAT_32BPP_PARGB: i32 = 0x000E_200B;

#[repr(C)]
struct StartupInput {
    version: u32,
    callback: *mut c_void,
    suppress_background: i32,
    suppress_external: i32,
}

#[repr(C)]
struct StartupOutput {
    notify_hook: *mut c_void,
    notify_unhook: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[link(name = "gdiplus")]
extern "system" {
    fn GdiplusStartup(token: *mut usize, input: *const StartupInput, output: *mut StartupOutput) -> GpStatus;
    fn GdiplusShutdown(token: usize);
    fn GdipCreateFromHDC(hdc: HDC, graphics: *mut GpGraphics) -> GpStatus;
    fn GdipDeleteGraphics(graphics: GpGraphics) -> GpStatus;
    fn GdipSetSmoothingMode(graphics: GpGraphics, mode: i32) -> GpStatus;
    fn GdipSetTextRenderingHint(graphics: GpGraphics, hint: i32) -> GpStatus;
    fn GdipSetPixelOffsetMode(graphics: GpGraphics, mode: i32) -> GpStatus;
    fn GdipCreateSolidFill(color: u32, brush: *mut GpBrush) -> GpStatus;
    fn GdipDeleteBrush(brush: GpBrush) -> GpStatus;
    fn GdipFillRectangleI(graphics: GpGraphics, brush: GpBrush, x: i32, y: i32, w: i32, h: i32) -> GpStatus;
    fn GdipFillEllipseI(graphics: GpGraphics, brush: GpBrush, x: i32, y: i32, w: i32, h: i32) -> GpStatus;
    fn GdipCreatePath(mode: i32, path: *mut GpPath) -> GpStatus;
    fn GdipDeletePath(path: GpPath) -> GpStatus;
    fn GdipAddPathArcI(path: GpPath, x: i32, y: i32, w: i32, h: i32, start: f32, sweep: f32) -> GpStatus;
    fn GdipAddPathLineI(path: GpPath, x1: i32, y1: i32, x2: i32, y2: i32) -> GpStatus;
    fn GdipClosePathFigure(path: GpPath) -> GpStatus;
    fn GdipFillPath(graphics: GpGraphics, brush: GpBrush, path: GpPath) -> GpStatus;
    fn GdipDrawPath(graphics: GpGraphics, pen: GpPen, path: GpPath) -> GpStatus;
    fn GdipDrawArcI(graphics: GpGraphics, pen: GpPen, x: i32, y: i32, w: i32, h: i32, start: f32, sweep: f32) -> GpStatus;
    fn GdipDrawLineI(graphics: GpGraphics, pen: GpPen, x1: i32, y1: i32, x2: i32, y2: i32) -> GpStatus;
    fn GdipCreatePen1(color: u32, width: f32, unit: i32, pen: *mut GpPen) -> GpStatus;
    fn GdipDeletePen(pen: GpPen) -> GpStatus;
    fn GdipCreateBitmapFromScan0(
        width: i32,
        height: i32,
        stride: i32,
        format: i32,
        scan0: *mut u32,
        bitmap: *mut GpBitmap,
    ) -> GpStatus;
    fn GdipGetImageGraphicsContext(image: GpImage, graphics: *mut GpGraphics) -> GpStatus;
    fn GdipCreateHICONFromBitmap(bitmap: GpBitmap, icon: *mut HICON) -> GpStatus;
    fn GdipDrawImageRectI(graphics: GpGraphics, image: GpImage, x: i32, y: i32, w: i32, h: i32) -> GpStatus;
    fn GdipSetInterpolationMode(graphics: GpGraphics, mode: i32) -> GpStatus;
    fn GdipDisposeImage(image: GpImage) -> GpStatus;
    fn GdipCreateFontFamilyFromName(
        name: *const u16,
        collection: *mut c_void,
        family: *mut GpFontFamily,
    ) -> GpStatus;
    fn GdipDeleteFontFamily(family: GpFontFamily) -> GpStatus;
    fn GdipCreateFont(
        family: GpFontFamily,
        size: f32,
        style: i32,
        unit: i32,
        font: *mut GpFont,
    ) -> GpStatus;
    fn GdipDeleteFont(font: GpFont) -> GpStatus;
    fn GdipCreateStringFormat(attributes: i32, language: u16, format: *mut GpStringFormat) -> GpStatus;
    fn GdipDeleteStringFormat(format: GpStringFormat) -> GpStatus;
    fn GdipSetStringFormatAlign(format: GpStringFormat, align: i32) -> GpStatus;
    fn GdipSetStringFormatLineAlign(format: GpStringFormat, align: i32) -> GpStatus;
    fn GdipDrawString(
        graphics: GpGraphics,
        text: *const u16,
        length: i32,
        font: GpFont,
        layout: *const RectF,
        format: GpStringFormat,
        brush: GpBrush,
    ) -> GpStatus;
    fn GdipMeasureString(
        graphics: GpGraphics,
        text: *const u16,
        length: i32,
        font: GpFont,
        layout: *const RectF,
        format: GpStringFormat,
        bounds: *mut RectF,
        codepoints: *mut i32,
        lines: *mut i32,
    ) -> GpStatus;
}

static TOKEN: AtomicUsize = AtomicUsize::new(0);

pub fn init() {
    if TOKEN.load(Ordering::Relaxed) != 0 {
        return;
    }
    let input = StartupInput {
        version: 1,
        callback: std::ptr::null_mut(),
        suppress_background: 0,
        suppress_external: 0,
    };
    let mut token: usize = 0;
    let mut output = StartupOutput {
        notify_hook: std::ptr::null_mut(),
        notify_unhook: std::ptr::null_mut(),
    };
    let status = unsafe { GdiplusStartup(&mut token, &input, &mut output) };
    if status == 0 {
        TOKEN.store(token, Ordering::Relaxed);
    }
}

pub fn shutdown() {
    let token = TOKEN.swap(0, Ordering::Relaxed);
    if token != 0 {
        unsafe { GdiplusShutdown(token) };
    }
}

/// GDI+ 颜色：0xAARRGGBB
pub fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

pub struct Graphics(pub GpGraphics);

impl Graphics {
    pub fn from_hdc(hdc: HDC) -> Option<Graphics> {
        let mut graphics: GpGraphics = std::ptr::null_mut();
        if unsafe { GdipCreateFromHDC(hdc, &mut graphics) } != 0 {
            return None;
        }
        unsafe {
            GdipSetSmoothingMode(graphics, SMOOTHING_ANTIALIAS);
            GdipSetTextRenderingHint(graphics, TEXT_ANTIALIAS);
            GdipSetPixelOffsetMode(graphics, 2);
        }
        Some(Graphics(graphics))
    }

    pub fn from_bitmap(bitmap: GpBitmap) -> Option<Graphics> {
        let mut graphics: GpGraphics = std::ptr::null_mut();
        if unsafe { GdipGetImageGraphicsContext(bitmap, &mut graphics) } != 0 {
            return None;
        }
        unsafe {
            GdipSetSmoothingMode(graphics, SMOOTHING_ANTIALIAS);
            GdipSetTextRenderingHint(graphics, TEXT_ANTIALIAS);
            GdipSetPixelOffsetMode(graphics, 2);
        }
        Some(Graphics(graphics))
    }

    pub fn clear(&self, color: u32) {
        let brush = match Brush::solid(color) {
            Some(value) => value,
            None => return,
        };
        unsafe { GdipFillRectangleI(self.0, brush.0, -1, -1, 100000, 100000) };
    }

    pub fn fill_rect(&self, x: i32, y: i32, width: i32, height: i32, color: u32) {
        if let Some(brush) = Brush::solid(color) {
            unsafe { GdipFillRectangleI(self.0, brush.0, x, y, width, height) };
        }
    }

    pub fn fill_ellipse(&self, x: i32, y: i32, width: i32, height: i32, color: u32) {
        if let Some(brush) = Brush::solid(color) {
            unsafe { GdipFillEllipseI(self.0, brush.0, x, y, width, height) };
        }
    }

    pub fn fill_round_rect(&self, x: i32, y: i32, width: i32, height: i32, radius: i32, color: u32) {
        let mut path: GpPath = std::ptr::null_mut();
        if unsafe { GdipCreatePath(0, &mut path) } != 0 {
            return;
        }
        let d = radius * 2;
        unsafe {
            GdipAddPathArcI(path, x, y, d, d, 180.0, 90.0);
            GdipAddPathArcI(path, x + width - d, y, d, d, 270.0, 90.0);
            GdipAddPathArcI(path, x + width - d, y + height - d, d, d, 0.0, 90.0);
            GdipAddPathArcI(path, x, y + height - d, d, d, 90.0, 90.0);
            GdipClosePathFigure(path);
        }
        if let Some(brush) = Brush::solid(color) {
            unsafe { GdipFillPath(self.0, brush.0, path) };
        }
        unsafe { GdipDeletePath(path) };
    }

    pub fn stroke_round_rect(
        &self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        radius: i32,
        thickness: f32,
        color: u32,
    ) {
        let pen = match Pen::new(color, thickness) {
            Some(value) => value,
            None => return,
        };
        let mut path: GpPath = std::ptr::null_mut();
        if unsafe { GdipCreatePath(0, &mut path) } != 0 {
            return;
        }
        let d = radius * 2;
        unsafe {
            GdipAddPathArcI(path, x, y, d, d, 180.0, 90.0);
            GdipAddPathArcI(path, x + width - d, y, d, d, 270.0, 90.0);
            GdipAddPathArcI(path, x + width - d, y + height - d, d, d, 0.0, 90.0);
            GdipAddPathArcI(path, x, y + height - d, d, d, 90.0, 90.0);
            GdipClosePathFigure(path);
            GdipDrawPath(self.0, pen.0, path);
        }
        unsafe { GdipDeletePath(path) };
    }

    /// 画一段圆环（角度制，0 点在 3 点钟方向，顺时针）
    pub fn draw_arc(
        &self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        start: f32,
        sweep: f32,
        thickness: f32,
        color: u32,
    ) {
        if let Some(pen) = Pen::new(color, thickness) {
            unsafe { GdipDrawArcI(self.0, pen.0, x, y, width, height, start, sweep) };
        }
    }

    pub fn draw_line(&self, x1: i32, y1: i32, x2: i32, y2: i32, thickness: f32, color: u32) {
        if let Some(pen) = Pen::new(color, thickness) {
            unsafe { GdipDrawLineI(self.0, pen.0, x1, y1, x2, y2) };
        }
    }

    pub fn draw_text(
        &self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        bold: bool,
        align: i32,
        width: f32,
        color: u32,
    ) {
        let font = match Font::new(size, bold) {
            Some(value) => value,
            None => return,
        };
        let brush = match Brush::solid(color) {
            Some(value) => value,
            None => return,
        };
        let format = StringFormat::new(align);
        let units: Vec<u16> = text.encode_utf16().collect();
        let layout = RectF {
            x,
            y,
            width: width.max(10.0),
            height: 200.0,
        };
        unsafe {
            GdipDrawString(
                self.0,
                units.as_ptr(),
                units.len() as i32,
                font.0,
                &layout,
                format.0,
                brush.0,
            );
        }
    }

    /// 量一段文字，用于自适应布局
    pub fn measure_text(&self, text: &str, size: f32, bold: bool) -> f32 {
        let font = match Font::new(size, bold) {
            Some(value) => value,
            None => return 0.0,
        };
        let format = StringFormat::new(ALIGN_NEAR);
        let units: Vec<u16> = text.encode_utf16().collect();
        let layout = RectF {
            x: 0.0,
            y: 0.0,
            width: 4000.0,
            height: 200.0,
        };
        let mut bounds = RectF::default();
        let status = unsafe {
            GdipMeasureString(
                self.0,
                units.as_ptr(),
                units.len() as i32,
                font.0,
                &layout,
                format.0,
                &mut bounds,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if status == 0 {
            bounds.width + 2.0
        } else {
            units.len() as f32 * size * 0.75
        }
    }
}

impl Drop for Graphics {
    fn drop(&mut self) {
        unsafe { GdipDeleteGraphics(self.0) };
    }
}

pub struct Brush(GpBrush);

impl Brush {
    pub fn solid(color: u32) -> Option<Brush> {
        let mut brush: GpBrush = std::ptr::null_mut();
        if unsafe { GdipCreateSolidFill(color, &mut brush) } != 0 {
            return None;
        }
        Some(Brush(brush))
    }
}

impl Drop for Brush {
    fn drop(&mut self) {
        unsafe { GdipDeleteBrush(self.0) };
    }
}

pub struct Pen(GpPen);

impl Pen {
    pub fn new(color: u32, width: f32) -> Option<Pen> {
        let mut pen: GpPen = std::ptr::null_mut();
        if unsafe { GdipCreatePen1(color, width, UNIT_PIXEL, &mut pen) } != 0 {
            return None;
        }
        Some(Pen(pen))
    }
}

impl Drop for Pen {
    fn drop(&mut self) {
        unsafe { GdipDeletePen(self.0) };
    }
}

pub struct Font(GpFont);

impl Font {
    pub fn new(size: f32, bold: bool) -> Option<Font> {
        let family = FontFamily::new("Microsoft YaHei UI")
            .or_else(|| FontFamily::new("Microsoft YaHei"))
            .or_else(|| FontFamily::new("Segoe UI"))?;
        let mut font: GpFont = std::ptr::null_mut();
        let style = if bold { FONT_STYLE_BOLD } else { FONT_STYLE_REGULAR };
        if unsafe { GdipCreateFont(family.0, size, style, UNIT_PIXEL, &mut font) } != 0 {
            return None;
        }
        Some(Font(font))
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        unsafe { GdipDeleteFont(self.0) };
    }
}

struct FontFamily(GpFontFamily);

impl FontFamily {
    fn new(name: &str) -> Option<FontFamily> {
        let wide = ffi::wide(name);
        let mut family: GpFontFamily = std::ptr::null_mut();
        if unsafe { GdipCreateFontFamilyFromName(wide.as_ptr(), std::ptr::null_mut(), &mut family) } != 0
        {
            return None;
        }
        Some(FontFamily(family))
    }
}

impl Drop for FontFamily {
    fn drop(&mut self) {
        unsafe { GdipDeleteFontFamily(self.0) };
    }
}

pub struct StringFormat(GpStringFormat);

impl StringFormat {
    pub fn new(align: i32) -> StringFormat {
        let mut format: GpStringFormat = std::ptr::null_mut();
        if unsafe { GdipCreateStringFormat(0, 0, &mut format) } != 0 {
            return StringFormat(std::ptr::null_mut());
        }
        unsafe {
            GdipSetStringFormatAlign(format, align);
            GdipSetStringFormatLineAlign(format, ALIGN_NEAR);
        }
        StringFormat(format)
    }
}

impl Drop for StringFormat {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { GdipDeleteStringFormat(self.0) };
        }
    }
}

pub struct Bitmap(pub GpBitmap);

impl Bitmap {
    pub fn new(width: i32, height: i32, premultiplied: bool) -> Option<Bitmap> {
        let mut bitmap: GpBitmap = std::ptr::null_mut();
        let format = if premultiplied {
            PIXEL_FORMAT_32BPP_PARGB
        } else {
            PIXEL_FORMAT_32BPP_ARGB
        };
        if unsafe {
            GdipCreateBitmapFromScan0(
                width,
                height,
                width * 4,
                format,
                std::ptr::null_mut(),
                &mut bitmap,
            )
        } != 0
        {
            return None;
        }
        Some(Bitmap(bitmap))
    }

    /// 直接包住一块 DIB 像素内存（分层窗口用，预乘 alpha）
    pub fn from_bits(
        width: i32,
        height: i32,
        stride: i32,
        format: i32,
        bits: *mut u32,
    ) -> Option<Bitmap> {
        let mut bitmap: GpBitmap = std::ptr::null_mut();
        if unsafe {
            GdipCreateBitmapFromScan0(width, height, stride, format, bits, &mut bitmap)
        } != 0
        {
            return None;
        }
        Some(Bitmap(bitmap))
    }

    /// 内部 GpBitmap 句柄，供 Graphics 等接口使用
    pub fn raw(&self) -> GpBitmap {
        self.0
    }

    pub fn to_hicon(&self) -> HICON {
        let mut icon: HICON = std::ptr::null_mut();
        if unsafe { GdipCreateHICONFromBitmap(self.0, &mut icon) } != 0 {
            return std::ptr::null_mut();
        }
        icon
    }
}

impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe { GdipDisposeImage(self.0) };
    }
}

// ---------------------------------------------------------------- 状态配色
pub const COLOR_GOOD: u32 = 0xFF3D_DC84;
pub const COLOR_WARN: u32 = 0xFFFF_B648;
pub const COLOR_BAD: u32 = 0xFFFF_5F56;
pub const COLOR_OFF: u32 = 0xFF7C_8798;

pub fn argb_to_gdi(color: u32) -> u32 {
    // GDI+ 的 0xAARRGGBB 与 COLORREF(0x00BBGGRR) 互换
    let r = (color >> 16) & 0xFF;
    let g = (color >> 8) & 0xFF;
    let b = color & 0xFF;
    (r << 16) | (g << 8) | b
}

/// 画一个三段式圆环图标，用于托盘。
pub fn render_ring_icon(
    clash: u32,
    vpn: u32,
    direct: u32,
    overall: u32,
    size: i32,
) -> HICON {
    let scale = 4; // 超采样后缩小，边缘更平滑
    let big = size * scale;
    let bitmap = match Bitmap::new(big, big, false) {
        Some(value) => value,
        None => return std::ptr::null_mut(),
    };
    let graphics = match Graphics::from_bitmap(bitmap.0) {
        Some(value) => value,
        None => return std::ptr::null_mut(),
    };

    let margin = (big as f32 * 0.035) as i32;
    let diameter = big - margin * 2;
    graphics.fill_ellipse(margin, margin, diameter, diameter, argb(255, 23, 27, 35));
    graphics.stroke_round_rect(
        margin,
        margin,
        diameter,
        diameter,
        diameter / 2,
        big as f32 * 0.035,
        argb(215, 150, 163, 184),
    );

    let inset = (big as f32 * 0.17) as i32;
    let arc_size = big - inset * 2;
    let thickness = big as f32 * 0.14;
    // 三段各 108°，间隔 12°；0° 在 3 点钟方向、顺时针
    graphics.draw_arc(inset, inset, arc_size, arc_size, 216.0, 108.0, thickness, clash);
    graphics.draw_arc(inset, inset, arc_size, arc_size, 336.0, 108.0, thickness, vpn);
    graphics.draw_arc(inset, inset, arc_size, arc_size, 96.0, 108.0, thickness, direct);

    let dot = (big as f32 * 0.125) as i32;
    let center = big / 2;
    graphics.fill_ellipse(
        center - dot,
        center - dot,
        dot * 2,
        dot * 2,
        overall,
    );

    drop(graphics);

    // 缩放到目标尺寸（高质插值），再转成 HICON
    let small = match Bitmap::new(size, size, false) {
        Some(value) => value,
        None => return std::ptr::null_mut(),
    };
    if let Some(target) = Graphics::from_bitmap(small.0) {
        unsafe {
            GdipSetInterpolationMode(target.0, 7); // HighQualityBicubic
            GdipDrawImageRectI(target.0, bitmap.0, 0, 0, size, size);
        }
    }
    small.to_hicon()
}

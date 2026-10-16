//! Win32 glue: the screen, taskbar, other windows, input idleness. The only file with `unsafe`.
#![allow(unsafe_code)]

use crate::world::Rect;
use anyhow::{Context, bail};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, RECT, TRUE, WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::Win32::System::SystemInformation::{GetLocalTime, GetTickCount};
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::Shell::{
    ABM_GETSTATE, ABM_GETTASKBARPOS, ABS_AUTOHIDE, APPBARDATA, SHAppBarMessage,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EnumWindows, FindWindowExW, FindWindowW, GWL_EXSTYLE, GetClassNameW,
    GetCursorPos, GetForegroundWindow, GetMessageW, GetShellWindow, GetSystemMetrics,
    GetWindowLongW, GetWindowRect, GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, MSG, PostMessageW, PostThreadMessageW, SM_CXSCREEN, SM_CYSCREEN,
    TranslateMessage, WM_APP, WM_CANCELMODE, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT,
};
use windows::core::{BOOL, PCWSTR, w};

fn rect(r: RECT) -> Rect {
    Rect {
        x: r.left as f32,
        y: r.top as f32,
        w: (r.right - r.left) as f32,
        h: (r.bottom - r.top) as f32,
    }
}

/// Call once, before any window exists, so every coordinate is a physical pixel.
pub fn set_dpi_aware() {
    // SAFETY: plain Win32 call with a constant.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

pub fn screen() -> Rect {
    // SAFETY: GetSystemMetrics has no preconditions.
    let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    Rect {
        x: 0.0,
        y: 0.0,
        w: w as f32,
        h: h as f32,
    }
}

/// `None` when the taskbar is auto-hidden or not along the bottom of the primary monitor.
pub fn taskbar() -> Option<Rect> {
    let mut data = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        ..Default::default()
    };
    // SAFETY: `data` is a valid APPBARDATA with cbSize set.
    unsafe {
        if SHAppBarMessage(ABM_GETSTATE, &mut data) as u32 & ABS_AUTOHIDE != 0 {
            return None;
        }
        if SHAppBarMessage(ABM_GETTASKBARPOS, &mut data) == 0 {
            return None;
        }
    }
    let r = rect(data.rc);
    let s = screen();
    (r.y > s.h / 2.0 && r.w >= s.w / 2.0).then_some(r)
}

pub fn tray_left() -> f32 {
    // SAFETY: FindWindow* and GetWindowRect take a valid out pointer.
    unsafe {
        if let Ok(tray) = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null())
            && let Ok(notify) = FindWindowExW(Some(tray), None, w!("TrayNotifyWnd"), PCWSTR::null())
        {
            let mut r = RECT::default();
            if GetWindowRect(notify, &mut r).is_ok() {
                return r.left as f32;
            }
        }
    }
    screen().right() - 200.0
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: lparam is the &mut Vec<HWND> passed by top_windows.
    let list = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
    list.push(hwnd);
    TRUE
}

/// Visible, non-minimised, non-tool, non-cloaked windows with a title, front-most first.
pub fn top_windows() -> Vec<Rect> {
    let mut all: Vec<HWND> = Vec::new();
    // SAFETY: the callback only touches the Vec behind lparam, alive for the whole call.
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut all as *mut Vec<HWND> as isize));
    }
    // SAFETY: GetCurrentProcessId has no preconditions.
    let ours = unsafe { GetCurrentProcessId() };
    let mut out = Vec::new();
    for hwnd in all {
        // SAFETY: hwnd came from EnumWindows; every call below tolerates a stale handle.
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                continue;
            }
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            if ex & WS_EX_TOOLWINDOW.0 != 0 {
                continue;
            }
            // Click-through overlays (PetLink, Discord, Overwolf...) are not real windows.
            let see_through = WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0;
            if ex & WS_EX_TRANSPARENT.0 != 0 || (ex & WS_EX_LAYERED.0 != 0 && ex & see_through != 0)
            {
                continue;
            }
            if GetWindowTextLengthW(hwnd) == 0 {
                continue;
            }
            let mut cloaked = 0u32;
            let _ =
                DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4);
            if cloaked != 0 {
                continue;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == ours {
                continue;
            }
            let mut r = RECT::default();
            if DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut r as *mut RECT as *mut _,
                std::mem::size_of::<RECT>() as u32,
            )
            .is_err()
                && GetWindowRect(hwnd, &mut r).is_err()
            {
                continue;
            }
            if r.right > r.left && r.bottom > r.top {
                out.push(rect(r));
            }
        }
    }
    out
}

pub fn cursor() -> (f32, f32) {
    let mut p = Default::default();
    // SAFETY: valid out pointer.
    if unsafe { GetCursorPos(&mut p) }.is_ok() {
        (p.x as f32, p.y as f32)
    } else {
        (-1.0, -1.0)
    }
}

pub fn idle_seconds() -> u64 {
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: valid LASTINPUTINFO with cbSize set.
    unsafe {
        if !GetLastInputInfo(&mut info).as_bool() {
            return 0;
        }
        u64::from(GetTickCount().wrapping_sub(info.dwTime)) / 1000
    }
}

pub fn foreground_is_fullscreen() -> bool {
    // SAFETY: all handles come from the system and are only passed back to it.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() || hwnd == GetShellWindow() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == GetCurrentProcessId() {
            return false;
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..n]);
        if class == "Progman" || class == "WorkerW" {
            return false;
        }
        let mut r = RECT::default();
        if GetWindowRect(hwnd, &mut r).is_err() {
            return false;
        }
        let s = screen();
        let r = rect(r);
        r.x <= s.x && r.y <= s.y && r.right() >= s.right() && r.y + r.h >= s.y + s.h
    }
}

/// Plays a WAV image from memory without waiting for it to finish.
pub fn play_wav(wav: &'static [u8]) {
    // SAFETY: with SND_MEMORY the pointer is the WAV image itself, which is 'static.
    unsafe {
        let _ = PlaySoundW(
            PCWSTR(wav.as_ptr().cast()),
            None,
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

/// False if another copy with the same `name` runs. The handle is leaked on purpose.
pub fn single_instance(name: &str) -> bool {
    // A failed install falls back to running in the same process, which already holds the lock.
    static HELD: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let mut held = HELD.lock().unwrap_or_else(|e| e.into_inner());
    if held.iter().any(|n| n == name) {
        return true;
    }
    let wide: Vec<u16> = format!("Local\\{name}\0").encode_utf16().collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call.
    unsafe {
        let handle = CreateMutexW(None, true, PCWSTR(wide.as_ptr()));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // Close our handle so the old copy's exit can free the name (update restarts retry).
            if let Ok(h) = handle {
                let _ = CloseHandle(h);
            }
            return false;
        }
        held.push(name.to_owned());
        true
    }
}

/// Local date and time as (year, month, day, hour, minute).
pub fn local_time() -> (u16, u16, u16, u16, u16) {
    // SAFETY: GetLocalTime has no preconditions.
    let t = unsafe { GetLocalTime() };
    (t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute)
}

/// One snapshot of the primary monitor as a JPEG, at most `max_w` wide.
pub fn capture_primary_jpeg(max_w: u32, quality: u8) -> anyhow::Result<Vec<u8>> {
    use image::{DynamicImage, RgbaImage, imageops::FilterType};
    let monitors = xcap::Monitor::all().context("listing monitors")?;
    let primary = monitors
        .into_iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .context("no primary monitor")?;
    let shot = primary.capture_image().context("capturing the screen")?;
    // xcap may link another version of `image`: go through raw pixels.
    let (w, h) = (shot.width(), shot.height());
    let img = RgbaImage::from_raw(w, h, shot.into_raw()).context("odd screenshot buffer")?;
    let img = if w > max_w {
        image::imageops::resize(&img, max_w, (h * max_w / w).max(1), FilterType::Triangle)
    } else {
        img
    };
    let rgb = DynamicImage::ImageRgba8(img).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(&rgb)
        .context("encoding the JPEG")?;
    Ok(out)
}

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");

pub fn autostart_enabled() -> bool {
    // SAFETY: null data pointer with null size just probes for existence.
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            w!("PetLink"),
            RRF_RT_REG_SZ,
            None,
            None,
            None,
        )
        .0 == 0
    }
}

pub fn set_autostart(on: bool) -> anyhow::Result<()> {
    if on {
        return set_autostart_for(&std::env::current_exe().context("exe path")?);
    }
    // SAFETY: plain registry delete with static names.
    unsafe {
        let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, w!("PetLink"));
    }
    Ok(())
}

pub fn set_autostart_for(exe: &std::path::Path) -> anyhow::Result<()> {
    // SAFETY: the data pointer and byte length describe `wide`, alive for the call.
    unsafe {
        {
            let wide: Vec<u16> = format!("\"{}\"\0", exe.display()).encode_utf16().collect();
            let e = RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                w!("PetLink"),
                REG_SZ.0,
                Some(wide.as_ptr() as *const _),
                (wide.len() * 2) as u32,
            );
            if e.0 != 0 {
                bail!("registry write failed ({})", e.0);
            }
        }
    }
    Ok(())
}

pub fn thread_id() -> u32 {
    unsafe { GetCurrentThreadId() }
}

/// Wakes the message loop of thread `tid` (see `pump_one`).
pub fn post_wake(tid: u32) {
    unsafe {
        let _ = PostThreadMessageW(tid, WM_APP, WPARAM(0), LPARAM(0));
    }
}

/// Blocks for one message and dispatches it; false once the loop should end.
pub fn pump_one() -> bool {
    unsafe {
        let mut m = MSG::default();
        if GetMessageW(&mut m, None, 0, 0).0 <= 0 {
            return false;
        }
        let _ = TranslateMessage(&m);
        DispatchMessageW(&m);
        true
    }
}

/// Debug autotest: closes a popup menu owned by `hwnd`.
#[cfg(debug_assertions)]
pub fn post_cancel_mode(hwnd: isize) {
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd as _)), WM_CANCELMODE, WPARAM(0), LPARAM(0));
    }
}

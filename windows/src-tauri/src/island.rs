// Island window: placement on the chosen display, the two window sizes
// (full panel / invisible wake strip), click-through and the cursor poll.
//
// There is no notch on a PC, so the island is a black shape drawn at the top
// centre of the main display inside a borderless, transparent, always-on-top
// window that never takes focus.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Monitor, PhysicalPosition, PhysicalSize, WebviewWindow};

use crate::platform::{self, cursor_physical, left_button_down};

/// Logical size of the full window — the largest island view, like the macOS panel.
pub const PANEL_W: f64 = 720.0;
pub const PANEL_H: f64 = 320.0;
/// Logical size of the strip that holds the minimized pill when the island is hidden.
pub const STRIP_W: f64 = 240.0;
pub const STRIP_H: f64 = 20.0;

pub const WINDOW_LABEL: &str = "island";

/// Margin around the island that still counts as "on the island", in logical px.
/// Wider than the macOS 6 pt because a click must never be swallowed.
const HIT_MARGIN: f64 = 24.0;

#[derive(Serialize, Clone)]
pub struct CursorPayload {
    pub x: f64,
    pub y: f64,
}

#[derive(Serialize, Clone)]
pub struct ScreenInfo {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
}

/// The island shape in window-logical coordinates, pushed by the front end.
/// The poll thread owns the click-through decision so it lands in the same 16 ms
/// tick as the cursor read — an IPC round trip here loses clicks.
#[derive(Clone, Copy, Default)]
pub struct IslandRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Wakes / parks the cursor poll thread so a hidden island costs literally nothing.
pub struct PollGate {
    active: Mutex<bool>,
    cv: Condvar,
    pub collapsed: AtomicBool,
    pub rect: Mutex<IslandRect>,
    /// Mirrors the window flag so we only call into the OS when it changes.
    ignoring: AtomicBool,
}

impl PollGate {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(false),
            cv: Condvar::new(),
            collapsed: AtomicBool::new(true),
            rect: Mutex::new(IslandRect::default()),
            ignoring: AtomicBool::new(false),
        }
    }

    pub fn set_rect(&self, rect: IslandRect) {
        *self.rect.lock().unwrap() = rect;
    }

    /// Forces the next poll tick to re-apply the flag (after a window resize).
    pub fn forget_ignore_state(&self) {
        self.ignoring.store(false, Ordering::Relaxed);
    }

    pub fn set_active(&self, on: bool) {
        let mut guard = self.active.lock().unwrap();
        *guard = on;
        self.cv.notify_all();
    }

    pub(crate) fn wait_until_active(&self) {
        let mut guard = self.active.lock().unwrap();
        while !*guard {
            guard = self.cv.wait(guard).unwrap();
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        *self.active.lock().unwrap()
    }
}

pub fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(WINDOW_LABEL)
}

fn monitor_contains(m: &Monitor, x: f64, y: f64) -> bool {
    let p = m.position();
    let s = m.size();
    x >= p.x as f64
        && x < (p.x + s.width as i32) as f64
        && y >= p.y as f64
        && y < (p.y + s.height as i32) as f64
}

/// A display's logical origin, the key `at:<x>,<y>` preferences are matched on.
/// Names are no good for that: two monitors of the same model share one.
fn logical_origin(m: &Monitor) -> (i32, i32) {
    let scale = m.scale_factor();
    let p = m.position();
    ((p.x as f64 / scale).round() as i32, (p.y as f64 / scale).round() as i32)
}

/// One entry of the "Island lives on" list in Settings.
#[derive(Serialize, Clone)]
pub struct MonitorChoice {
    pub key: String,
    pub label: String,
}

pub fn monitor_choices(app: &AppHandle) -> Vec<MonitorChoice> {
    let Ok(monitors) = app.available_monitors() else { return Vec::new() };
    monitors
        .iter()
        .map(|m| {
            let d = describe(m);
            MonitorChoice {
                key: d.key(),
                label: crate::i18n::tf(
                    "{name} — {width}×{height} at {x},{y}",
                    &[
                        ("name", &d.name),
                        ("width", &d.w.to_string()),
                        ("height", &d.h.to_string()),
                        ("x", &d.x.to_string()),
                        ("y", &d.y.to_string()),
                    ],
                ),
            }
        })
        .collect()
}

/// What a display is remembered by: its logical origin, plus its name and
/// logical size, so it is still found after the layout is rearranged or the
/// resolution changes (the Mac keeps the display's UUID for the same reason;
/// Tauri has no stable ID).
#[derive(Debug, Clone, PartialEq)]
struct DisplayId {
    name: String,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl DisplayId {
    /// `at:<x>,<y>` stays first, so a preference saved before still matches.
    fn key(&self) -> String {
        format!("at:{},{}|{}|{}x{}", self.x, self.y, self.name.replace('|', " "), self.w, self.h)
    }
}

fn describe(m: &Monitor) -> DisplayId {
    let (x, y) = logical_origin(m);
    let scale = m.scale_factor();
    let s = m.size();
    DisplayId {
        name: m.name().cloned().unwrap_or_else(|| "Display".into()),
        x,
        y,
        w: (s.width as f64 / scale).round() as i32,
        h: (s.height as f64 / scale).round() as i32,
    }
}

/// Which display a saved `at:` preference points at, best match first: same
/// place and name; the same name and size elsewhere (layout rearranged); the
/// same name alone when unique (resolution changed); the same place. None
/// means unplugged, and the caller falls back to the primary display.
fn pick_display(pref: &str, displays: &[DisplayId]) -> Option<usize> {
    let rest = pref.strip_prefix("at:")?;
    let mut parts = rest.split('|');
    let (x, y) = parts.next()?.split_once(',')?;
    let (x, y) = (x.trim().parse::<i32>().ok()?, y.trim().parse::<i32>().ok()?);
    let name = parts.next();
    let size = parts.next().and_then(|s| {
        let (w, h) = s.split_once('x')?;
        Some((w.parse::<i32>().ok()?, h.parse::<i32>().ok()?))
    });
    let at = |d: &DisplayId| d.x == x && d.y == y;
    if let Some(name) = name {
        if let Some(i) = displays.iter().position(|d| at(d) && d.name == name) {
            return Some(i);
        }
        if let Some((w, h)) = size {
            if let Some(i) = displays.iter().position(|d| d.name == name && d.w == w && d.h == h) {
                return Some(i);
            }
        }
        let mut same_name = displays.iter().enumerate().filter(|(_, d)| d.name == name);
        if let (Some((i, _)), None) = (same_name.next(), same_name.next()) {
            return Some(i);
        }
    }
    displays.iter().position(at)
}

/// The display the island lives on: a chosen one, the primary one, or the one
/// under the cursor.
fn target_monitor(app: &AppHandle, pref: &str) -> Option<Monitor> {
    let monitors = app.available_monitors().ok()?;
    let ids: Vec<DisplayId> = monitors.iter().map(describe).collect();
    if let Some(i) = pick_display(pref, &ids) {
        return Some(monitors[i].clone());
    }
    if pref == "cursor" {
        if let Some((cx, cy)) = cursor_physical() {
            if let Some(m) = monitors.iter().find(|m| monitor_contains(m, cx, cy)) {
                return Some(m.clone());
            }
        }
    }
    app.primary_monitor()
        .ok()
        .flatten()
        .or_else(|| monitors.into_iter().next())
}

#[cfg(test)]
mod display_tests {
    use super::*;

    fn d(name: &str, x: i32, y: i32, w: i32, h: i32) -> DisplayId {
        DisplayId { name: name.into(), x, y, w, h }
    }

    #[test]
    fn a_display_is_found_again_after_changes() {
        let dell = d("DELL U2720Q", 1920, 0, 2560, 1440);
        let lap = d("eDP-1", 0, 0, 1920, 1200);
        let key = dell.key();
        assert_eq!(pick_display(&key, &[lap.clone(), dell.clone()]), Some(1));
        // Rearranged: the Dell moved to the left of the laptop.
        let moved = [d("eDP-1", 2560, 0, 1920, 1200), d("DELL U2720Q", 0, 0, 2560, 1440)];
        assert_eq!(pick_display(&key, &moved), Some(1));
        // Resolution changed, still the only Dell.
        let rescaled = [lap.clone(), d("DELL U2720Q", 1920, 0, 1920, 1080)];
        assert_eq!(pick_display(&key, &rescaled), Some(1));
        // Unplugged: nothing, so the caller falls back to the primary display.
        assert_eq!(pick_display(&key, &[lap.clone()]), None);
    }

    #[test]
    fn two_identical_monitors_are_told_apart_by_place() {
        let a = d("LG 27UL500", 0, 0, 1920, 1080);
        let b = d("LG 27UL500", 1920, 0, 1920, 1080);
        assert_eq!(pick_display(&b.key(), &[a.clone(), b.clone()]), Some(1));
        assert_eq!(pick_display(&a.key(), &[a, b]), Some(0));
    }

    #[test]
    fn preferences_saved_before_still_match() {
        let lap = d("eDP-1", 0, 0, 1920, 1200);
        let ext = d("HDMI-1", 1920, 0, 1920, 1080);
        assert_eq!(pick_display("at:1920,0", &[lap.clone(), ext.clone()]), Some(1));
        assert_eq!(pick_display("primary", &[lap, ext]), None);
        assert_eq!(pick_display("at:nonsense", &[]), None);
    }
}

/// Calculates the zoom correction factor so that 1 CSS pixel equals 1 window logical pixel.
/// If WebView2's devicePixelRatio differs from the window's scale factor (e.g. system text
/// scaling or multi-monitor DPI mismatch), adjusting the webview zoom ensures the island
/// canvas (PANEL_W / EXPANDED_W) fits within the window without clipping.
pub fn calculate_zoom_correction(current_zoom: f64, scale: f64, dpr: f64) -> Option<f64> {
    if dpr <= 0.0 || !dpr.is_finite() || scale <= 0.0 || !scale.is_finite() {
        return None;
    }
    if (dpr - scale).abs() / scale < 0.02 {
        return None;
    }
    let new_zoom = (current_zoom * (scale / dpr)).clamp(0.25, 4.0);
    Some(new_zoom)
}

#[cfg(test)]
mod zoom_tests {
    use super::*;

    #[test]
    fn zoom_correction_is_none_when_dpr_matches_window_scale() {
        assert_eq!(calculate_zoom_correction(1.0, 1.0, 1.0), None);
        assert_eq!(calculate_zoom_correction(1.0, 1.25, 1.25), None);
        // Within 2% threshold
        assert_eq!(calculate_zoom_correction(1.0, 1.0, 1.01), None);
    }

    #[test]
    fn zoom_correction_adjusts_for_dpi_mismatch() {
        // Issue #346: 100% scale display (scale 1.0) but WebView2 dpr = 1.25
        let zoom = calculate_zoom_correction(1.0, 1.0, 1.25);
        assert_eq!(zoom, Some(0.8));

        // 125% display with 125% text scale (dpr 1.5625 vs scale 1.25)
        let zoom = calculate_zoom_correction(1.0, 1.25, 1.5625);
        assert_eq!(zoom, Some(0.8));

        // 150% display with 100% webview dpr
        let zoom = calculate_zoom_correction(1.0, 1.5, 1.0);
        assert_eq!(zoom, Some(1.5));
    }

    #[test]
    fn zoom_correction_handles_invalid_inputs() {
        assert_eq!(calculate_zoom_correction(1.0, 0.0, 1.0), None);
        assert_eq!(calculate_zoom_correction(1.0, 1.0, -1.0), None);
        assert_eq!(calculate_zoom_correction(1.0, f64::NAN, 1.0), None);
        assert_eq!(calculate_zoom_correction(1.0, 1.0, f64::INFINITY), None);
    }

    #[test]
    fn test_tab_count_blocking() {
        assert!(!super::is_tab_count_blocked(Some(1)));
        assert!(!super::is_tab_count_blocked(Some(2)));
        assert!(super::is_tab_count_blocked(Some(3)));
        assert!(super::is_tab_count_blocked(Some(4)));
        assert!(super::is_tab_count_blocked(Some(10)));
        assert!(!super::is_tab_count_blocked(None));
    }
}


pub fn screen_info(app: &AppHandle, pref: &str) -> ScreenInfo {
    match target_monitor(app, pref) {
        Some(m) => {
            let scale = m.scale_factor();
            let p = m.position();
            let s = m.size();
            ScreenInfo {
                x: p.x as f64 / scale,
                y: p.y as f64 / scale,
                width: s.width as f64 / scale,
                height: s.height as f64 / scale,
                scale,
            }
        }
        None => ScreenInfo { x: 0.0, y: 0.0, width: 1920.0, height: 1080.0, scale: 1.0 },
    }
}

/// Places and sizes the window. `collapsed` picks the wake strip instead of the panel.
pub fn apply_geometry(app: &AppHandle, pref: &str, collapsed: bool) {
    let Some(win) = window(app) else { return };
    let Some(m) = target_monitor(app, pref) else { return };

    let scale = m.scale_factor();
    let mp = *m.position();
    let ms = *m.size();

    let (lw, lh) = if collapsed { (STRIP_W, STRIP_H) } else { (PANEL_W, PANEL_H) };
    let pw = (lw * scale).round().max(1.0) as u32;
    let ph = (lh * scale).round().max(1.0) as u32;
    let x = mp.x + (ms.width as i32 - pw as i32) / 2;
    let y = mp.y;

    // GTK never sizes a non-resizable window below its natural size (200 px
    // here), so on Linux the 6 px wake strip would stay a 200 px block. tao
    // re-applies the config's `resizable: false` after the first configure, so
    // this is asked every time, just before the resize. Undecorated, the window
    // still offers the user nothing to resize it by. (Found by @YossiYad, #44.)
    #[cfg(target_os = "linux")]
    let _ = win.set_resizable(true);
    let _ = win.set_size(PhysicalSize::new(pw, ph));
    let _ = win.set_position(PhysicalPosition::new(x, y));
    let (lx, ly) = logical_origin(&m);
    platform::pin_to_monitor(&win, lx, ly);
    // Moving across displays can rescale the window: re-assert the physical size.
    let _ = win.set_size(PhysicalSize::new(pw, ph));
    let _ = win.set_always_on_top(true);

    // On Windows, moving across displays or waking from the strip can clamp or lag
    // the resize. If the physical inner size differs from what was requested, retry once.
    if let Ok(got) = win.inner_size() {
        if got.width != pw || got.height != ph {
            crate::log::line(format!(
                "window size mismatch: wanted {pw}x{ph}, got {}x{} (scale {scale}, monitor {}x{})",
                got.width, got.height, ms.width, ms.height
            ));
            let _ = win.set_size(PhysicalSize::new(pw, ph));
            let _ = win.set_position(PhysicalPosition::new(x, y));
        }
    }
}

/// Position, size and scale of the monitor the island lives on. Any change here
/// means the island has to be placed again.
fn current_screen_key(app: &AppHandle) -> Option<(i32, i32, u32, u32, u64)> {
    let pref = app
        .try_state::<crate::Shared>()
        .map(|s| s.settings.lock().unwrap().screen.clone())
        .unwrap_or_else(|| "primary".into());
    let m = target_monitor(app, &pref)?;
    let p = m.position();
    let size = m.size();
    Some((p.x, p.y, size.width, size.height, m.scale_factor().to_bits()))
}

/// Emits `cursor` (window-logical coordinates) at ~60 Hz while the island is
/// visible. Parked on a condvar the rest of the time.
pub fn spawn_cursor_poll(app: AppHandle, gate: Arc<PollGate>) {
    std::thread::spawn(move || {
        let mut was_down = false;
        // Remembered across wakes so a display change while hidden is noticed the
        // moment the island comes back.
        let mut last_screen: Option<(i32, i32, u32, u32, u64)> = None;
        // Without a cursor to read (Linux) the loop only watches the display
        // layout, and twice a second is plenty for that: waking at 60 Hz just to
        // find no cursor costs CPU for nothing.
        let (period, screen_every) = if platform::CURSOR_POLL { (16, 30) } else { (500, 1) };
        loop {
            gate.wait_until_active();
            let mut last = (f64::MIN, f64::MIN);
            let mut ticks: u32 = 0;
            while gate.is_active() {
                let cur_period = if platform::CURSOR_POLL && gate.collapsed.load(Ordering::Relaxed) {
                    33
                } else {
                    period
                };
                std::thread::sleep(Duration::from_millis(cur_period));

                // Monitors get plugged in, unplugged, rearranged and rescaled, and
                // an island pinned to coordinates that no longer exist is an island
                // nobody can reach. Checked about twice a second — the cursor poll
                // is already running, so this costs one monitor query.
                ticks = ticks.wrapping_add(1);
                if ticks % screen_every == 0 {
                    let now = current_screen_key(&app);
                    if now.is_some() && now != last_screen {
                        let first = last_screen.is_none();
                        last_screen = now;
                        if !first {
                            crate::log::line("display layout changed — repositioning".to_string());
                            let _ = app.emit_to(WINDOW_LABEL, "screen-changed", ());
                        }
                    }
                }

                let Some(win) = window(&app) else { continue };
                let Ok(origin) = win.outer_position() else { continue };
                let scale = win.scale_factor().unwrap_or(1.0);
                let Some((cx, cy)) = cursor_physical() else { continue };
                let x = (cx - origin.x as f64) / scale;
                let y = (cy - origin.y as f64) / scale;
                let size = match win.inner_size() {
                    Ok(s) => (s.width as f64 / scale, s.height as f64 / scale),
                    Err(_) => (PANEL_W, PANEL_H),
                };
                if (x - last.0).abs() < 1.0 && (y - last.1).abs() < 1.0 {
                    continue;
                }
                last = (x, y);

                // Click-through: the window only takes the mouse over the island
                // shape. A small entry margin means the flag is already off by the
                // time a moving cursor reaches a button.
                let r = *gate.rect.lock().unwrap();
                let on_island = r.w > 0.0
                    && x >= r.x - HIT_MARGIN
                    && x <= r.x + r.w + HIT_MARGIN
                    && y >= r.y - HIT_MARGIN
                    && y <= r.y + r.h + HIT_MARGIN;

                // A file being dragged has to be able to find us. WS_EX_TRANSPARENT
                // — what click-through is on Windows — hides the window from
                // WindowFromPoint, so OLE finds no drop target and shows the "no
                // drop" cursor. macOS has no such problem: AppKit delivers drags to
                // registered destinations whatever ignoresMouseEvents says. So while
                // a button is held anywhere over the panel, the whole panel takes
                // the mouse, which also makes the drop zone as forgiving as the Mac's.
                // A press may be the start of a drag: make sure the drop target is
                // ours before the file arrives.
                let down = left_button_down();
                if down && !was_down {
                    let handle = app.clone();
                    let _ = app.run_on_main_thread(move || platform::unblock_webview_drops(&handle));
                }
                was_down = down;

                let dragging = down
                    && x >= 0.0
                    && x <= size.0
                    && y >= 0.0
                    && y <= size.1;

                let accept = on_island || dragging;
                if gate.ignoring.load(Ordering::Relaxed) == accept {
                    gate.ignoring.store(!accept, Ordering::Relaxed);
                    let _ = win.set_ignore_cursor_events(!accept);
                }

                let _ = win.emit("cursor", CursorPayload { x, y });
            }
        }
    });
}

/// Re-applies click-through after the window or the island changed shape.
///
/// With the cursor poll (Windows) the window takes the mouse again and the next
/// tick decides from the cursor. Without it (Linux) the input region is set to
/// the island itself, or to the whole wake strip while collapsed.
pub fn refresh_click_through(app: &AppHandle, gate: &PollGate) {
    if platform::CURSOR_POLL {
        set_ignore_cursor(app, false);
        gate.forget_ignore_state();
        return;
    }
    let Some(win) = window(app) else { return };
    let region = if gate.collapsed.load(Ordering::Relaxed) {
        // The wake strip itself, never "the whole window": if the window ever
        // fails to shrink to the strip, the rest of it must not swallow clicks
        // meant for whatever sits under the top of the screen.
        Some((0.0, 0.0, STRIP_W, STRIP_H))
    } else {
        let r = *gate.rect.lock().unwrap();
        if r.w <= 0.0 {
            // Nothing drawn yet: nothing takes the mouse.
            Some((0.0, 0.0, 0.0, 0.0))
        } else {
            let x0 = (r.x - HIT_MARGIN).max(0.0);
            let y0 = (r.y - HIT_MARGIN).max(0.0);
            let x1 = r.x + r.w + HIT_MARGIN;
            let y1 = r.y + r.h + HIT_MARGIN;
            Some((x0, y0, x1 - x0, y1 - y0))
        }
    };
    platform::set_input_region(&win, region);
}

pub fn set_ignore_cursor(app: &AppHandle, ignore: bool) {
    if let Some(win) = window(app) {
        let _ = win.set_ignore_cursor_events(ignore);
    }
}

#[cfg(target_os = "windows")]
struct BrowserTabCache {
    hwnd: isize,
    title: String,
    count: Option<usize>,
    timestamp: std::time::Instant,
}

#[cfg(target_os = "windows")]
static BROWSER_TAB_CACHE: std::sync::Mutex<Option<BrowserTabCache>> = std::sync::Mutex::new(None);

/// Counts the open tabs in a browser window using UI Automation with caching.
#[cfg(target_os = "windows")]
pub fn browser_tab_count(hwnd: windows::Win32::Foundation::HWND) -> Option<usize> {
    unsafe {
        // Read window title (< 1 microsecond) to check cache
        let mut title_buf = [0u16; 512];
        let len = windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut title_buf);
        let title = if len > 0 {
            String::from_utf16_lossy(&title_buf[..len as usize])
        } else {
            String::new()
        };

        // Cache valid for 2000ms if HWND and window title match
        if let Ok(guard) = BROWSER_TAB_CACHE.lock() {
            if let Some(ref cache) = *guard {
                if cache.hwnd == hwnd.0 as isize
                    && cache.title == title
                    && cache.timestamp.elapsed() < Duration::from_millis(2000)
                {
                    return cache.count;
                }
            }
        }

        use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
        use windows::Win32::UI::Accessibility::*;
        use windows::Win32::System::Variant::*;

        thread_local! {
            static AUTOMATION: Option<IUIAutomation> = unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()
            };
        }

        let automation = match AUTOMATION.with(|auto| auto.clone()) {
            Some(auto) => auto,
            None => return None,
        };
        let element = match automation.ElementFromHandle(hwnd) {
            Ok(el) => el,
            Err(_) => return None,
        };

        let mut var = VARIANT::default();
        let anon = &mut *var.Anonymous.Anonymous;
        anon.vt = VT_I4;
        anon.Anonymous.lVal = UIA_TabItemControlTypeId.0;

        let condition = automation.CreatePropertyCondition(UIA_ControlTypePropertyId, &var).ok()?;
        let tabs = element.FindAll(TreeScope_Descendants, &condition).ok()?;
        let count = Some(tabs.Length().ok()? as usize);

        if let Ok(mut guard) = BROWSER_TAB_CACHE.lock() {
            *guard = Some(BrowserTabCache {
                hwnd: hwnd.0 as isize,
                title,
                count,
                timestamp: std::time::Instant::now(),
            });
        }

        count
    }
}

/// Helper to determine if tab count requires blocking (>= 3 tabs).
pub fn is_tab_count_blocked(tabs: Option<usize>) -> bool {
    match tabs {
        Some(count) => count >= 3,
        None => false, // do not block if tab count could not be reliably determined
    }
}

/// Checks whether a browser with full tabs is open and active at the top of the screen.
/// Only blocks if there are 3 or more tabs open (1 or 2 tabs is not blocked).
pub fn is_browser_tabs_blocked() -> bool {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect, IsWindowVisible, IsZoomed};
        use windows::Win32::Foundation::RECT;
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() || !IsWindowVisible(hwnd).as_bool() {
            return false;
        }
        if let Some((app_id, _)) = crate::flow::os::foreground_app() {
            let is_browser = matches!(
                app_id.as_str(),
                "chrome" | "msedge" | "firefox" | "brave" | "opera" | "vivaldi" | "arc" | "browser"
            );
            if is_browser {
                let mut rect = RECT::default();
                if GetWindowRect(hwnd, &mut rect).is_ok() {
                    let at_top = rect.top <= 10;
                    let is_maximized = IsZoomed(hwnd).as_bool();
                    let browser_window_at_top = (at_top || is_maximized) && (rect.bottom - rect.top > 200);
                    if browser_window_at_top {
                        // "saat tab chrome lebih atau sama dengan 3 jika dibawahnya tidak masalah"
                        let tabs = browser_tab_count(hwnd);
                        return is_tab_count_blocked(tabs);
                    }
                }
            }
        }
        false
    }
    #[cfg(not(target_os = "windows"))]
    false
}

/// Spawns a lightweight watcher for foreground browser tabs with debounce
pub fn spawn_browser_tabs_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last_emitted = false;
        let mut pending_candidate = false;
        let mut candidate_streak: u8 = 0;
        loop {
            std::thread::sleep(Duration::from_millis(400));
            let raw_blocked = is_browser_tabs_blocked();

            if raw_blocked == pending_candidate {
                candidate_streak = candidate_streak.saturating_add(1);
            } else {
                pending_candidate = raw_blocked;
                candidate_streak = 1;
            }

            // Require 2 consecutive checks (~800ms) of consistent state before emitting change
            if candidate_streak >= 2 && pending_candidate != last_emitted {
                last_emitted = pending_candidate;
                let _ = app.emit_to(WINDOW_LABEL, "browser-tabs-blocked", last_emitted);
            }
        }
    });
}

/// Spawns a watcher checking power/battery status every 3 seconds
pub fn spawn_battery_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last_status = platform::battery_status();
        loop {
            std::thread::sleep(Duration::from_millis(3000));
            let status = platform::battery_status();
            if status != last_status {
                last_status = status;
                let _ = app.emit_to(WINDOW_LABEL, "battery-status", status);
            }
        }
    });
}

/// Spawns a watcher checking connected bluetooth devices every 3 seconds
pub fn spawn_bluetooth_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last_status = platform::bluetooth_status();
        loop {
            std::thread::sleep(Duration::from_millis(3000));
            let status = platform::bluetooth_status();
            if status != last_status {
                last_status = status.clone();
                let _ = app.emit_to(WINDOW_LABEL, "bluetooth-status", status);
            }
        }
    });
}

/// Spawns a watcher checking microphone / camera activity every 1.5 seconds
pub fn spawn_privacy_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last_status = platform::privacy_status();
        loop {
            std::thread::sleep(Duration::from_millis(1500));
            let status = platform::privacy_status();
            if status != last_status {
                last_status = status.clone();
                let _ = app.emit_to(WINDOW_LABEL, "privacy-status", status);
            }
        }
    });
}

/// Spawns a watcher checking active browser / file downloads every 1 second
pub fn spawn_download_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last_bytes: u64 = 0;
        let mut last_name = String::new();
        let mut last_target_path: Option<PathBuf> = None;
        let mut last_instant = Instant::now();
        let mut was_active = false;
        let mut smoothed_speed: f64 = 0.0;
        let mut inactive_ticks: u32 = 0;

        loop {
            std::thread::sleep(Duration::from_millis(1000));
            let mut status = platform::check_download_status();

            if status.active {
                inactive_ticks = 0;
                let dt = last_instant.elapsed().as_secs_f64().max(0.2);
                last_instant = Instant::now();

                let is_same_download = !last_name.is_empty() && (
                    status.filename == last_name || 
                    last_name == "Mengunduh file..." || 
                    status.filename == "Mengunduh file..." ||
                    status.target_path.as_deref() == last_target_path.as_ref().and_then(|p| p.to_str())
                );

                if is_same_download && status.downloaded_bytes >= last_bytes {
                    let bytes_diff = status.downloaded_bytes - last_bytes;
                    let raw_speed = bytes_diff as f64 / dt;
                    smoothed_speed = if smoothed_speed <= 0.0 {
                        raw_speed
                    } else {
                        smoothed_speed * 0.35 + raw_speed * 0.65
                    };
                    status.speed_bytes_per_sec = smoothed_speed.round() as u64;
                } else {
                    smoothed_speed = 0.0;
                    status.speed_bytes_per_sec = 0;
                }

                last_bytes = status.downloaded_bytes;
                if status.filename != "Mengunduh file..." || last_name.is_empty() {
                    last_name = status.filename.clone();
                }
                if let Some(ref tp) = status.target_path {
                    last_target_path = Some(PathBuf::from(tp));
                }
                was_active = true;
                let _ = app.emit_to(WINDOW_LABEL, "download-status", status);
            } else if was_active {
                inactive_ticks += 1;
                if inactive_ticks >= 1 {
                    was_active = false;
                    inactive_ticks = 0;
                    last_bytes = 0;
                    smoothed_speed = 0.0;

                    let target = last_target_path.take();
                    let completed_name = std::mem::take(&mut last_name);

                    // Check if the final file actually exists!
                    let actually_completed = target.as_ref().map(|p| p.is_file()).unwrap_or(false);
                    let is_valid_name = !completed_name.is_empty() && completed_name != "Mengunduh file...";

                    if actually_completed && is_valid_name {
                        let _ = app.emit_to(WINDOW_LABEL, "download-complete", completed_name);
                    }
                    let _ = app.emit_to(WINDOW_LABEL, "download-status", status);
                }
            }
        }
    });
}

/// Spawns a watcher checking active Google Antigravity session and tasks every 1.5 seconds
pub fn spawn_antigravity_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last_task: Option<crate::antigravity::AntigravityLiveTask> = None;
        loop {
            std::thread::sleep(Duration::from_millis(1500));
            if let Some(task) = crate::antigravity::get_live_antigravity_task() {
                if last_task.as_ref() != Some(&task) {
                    last_task = Some(task.clone());
                    let _ = app.emit_to(WINDOW_LABEL, "antigravity-live-task", task);
                }
            } else if last_task.is_some() {
                last_task = None;
                let _ = app.emit_to(WINDOW_LABEL, "antigravity-live-task", serde_json::Value::Null);
            }
        }
    });
}

/// Trims physical memory every 60 seconds when the island is collapsed/idle
pub fn spawn_memory_trimmer(gate: Arc<PollGate>) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(60));
            if gate.collapsed.load(Ordering::Relaxed) {
                platform::trim_memory();
            }
        }
    });
}



// Windows: Win32 for the island window and the cursor, %APPDATA% for files.

use std::collections::HashMap;
use std::os::windows::io::RawHandle;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use tauri::{AppHandle, Manager, WebviewWindow};

use ::windows::core::{w, BOOL, PWSTR};
use ::windows::Win32::Foundation::{
    CloseHandle, HANDLE, HLOCAL, HWND, LPARAM, LocalFree, POINT, RPC_E_CHANGED_MODE,
};
use ::windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use ::windows::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
use ::windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use ::windows::Win32::Media::Audio::{
    eCapture, eConsole, eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
};
use ::windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    COINIT_DISABLE_OLE1DDE,
};
use ::windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_READ,
};
use ::windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use ::windows::Win32::System::Ole::RevokeDragDrop;
use ::windows::Win32::System::Pipes::GetNamedPipeClientProcessId;
use ::windows::Win32::System::SystemInformation::GetLocalTime;
use ::windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use ::windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyboardLayoutList, MapVirtualKeyExW, ToUnicodeEx, HKL, MAPVK_VK_TO_VSC,
    VK_CONTROL, VK_LBUTTON, VK_MENU, VK_SHIFT,
};
use ::windows::Win32::UI::Shell::{
    ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GetClassNameW, GetCursorPos, GetWindow, GetWindowLongPtrW,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow,
    SetWindowLongPtrW, ShowWindow, GWL_EXSTYLE, GW_OWNER, SW_RESTORE, SW_SHOWNORMAL, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW,
};

use super::LocalTime;
use crate::session_window::{self, Proc};

/// File name of the Claude Code relay.
pub const HOOK_EXE: &str = "coucou-hook.exe";

/// Environment variable holding the home directory.
pub const HOME_VAR: &str = "USERPROFILE";

/// Keeps spawned helpers from flashing a console window.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// ── Files ─────────────────────────────────────────────────────────────────────

/// %APPDATA%\Coucou — preferences.
pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("Coucou")
}

/// %LOCALAPPDATA%\Coucou — where coucou-hook.exe, the inbox and the log live.
pub fn local_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("Coucou")
}

/// Where a saved image goes, best first: Pictures (also where OneDrive moves
/// it), then Downloads and the profile folder. The caller takes the first one
/// that exists. Read from the environment rather than the Known Folders API: a
/// relocated Pictures folder falls back to Downloads, which is fine for a PNG.
pub fn picture_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
    if let Some(home) = &home {
        dirs.push(home.join("Pictures"));
    }
    if let Some(onedrive) = std::env::var_os("OneDrive").map(PathBuf::from) {
        dirs.push(onedrive.join("Pictures"));
    }
    if let Some(home) = home {
        dirs.push(home.join("Downloads"));
        dirs.push(home);
    }
    dirs
}

/// %APPDATA% and %LOCALAPPDATA% are already private to the user.
pub fn ensure_private_dir(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

/// Optimizes WebView2 memory footprint and background resource usage before startup.
pub fn prepare_environment() {
    if std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_none() {
        std::env::set_var(
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            "--js-flags=--max-old-space-size=128 --renderer-process-limit=1 --process-per-site --disable-features=Translate,InterestFeedContentSuggestions,CalculateNativeWinOcclusion --enable-features=MojoIpcz",
        );
    }
}

/// Trims physical memory (working set) of the current process, freeing unused heap pages to OS.
pub fn trim_memory() {
    unsafe {
        use windows::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
        let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

pub fn local_time() -> LocalTime {
    let t = unsafe { GetLocalTime() };
    LocalTime {
        year: t.wYear.into(),
        month: t.wMonth.into(),
        day: t.wDay.into(),
        hour: t.wHour.into(),
        minute: t.wMinute.into(),
        second: t.wSecond.into(),
    }
}

// ── Processes ─────────────────────────────────────────────────────────────────

/// Spawned helpers must never flash a console window.
pub fn no_console(cmd: &mut Command) -> &mut Command {
    cmd.creation_flags(CREATE_NO_WINDOW)
}

pub fn open_url(url: &str) {
    let _ = no_console(Command::new("rundll32.exe").args(["url.dll,FileProtocolHandler", url]))
        .spawn();
}

pub fn reveal_folder(path: &str) {
    let _ = Command::new("explorer").arg(path).spawn();
}

/// Our own `where`: walks %PATH% against %PATHEXT%, no shell involved.
/// Rust quotes arguments correctly for `.cmd`/`.bat` targets since 1.77, so
/// spawning `code.cmd` directly is safe.
pub fn find_on_path(stem: &str) -> Option<PathBuf> {
    let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    let dirs = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&dirs) {
        for ext in exts.split(';').filter(|e| !e.is_empty()) {
            let candidate = dir.join(format!("{stem}{}", ext.to_lowercase()));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Where the Codex CLI may be, best first: %PATH% (codex.exe or npm's
/// codex.cmd), then npm's global folder and the Volta / Bun / pnpm ones.
/// Rust quotes the one fixed argument safely for a `.cmd` (see find_on_path).
pub fn codex_candidates() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = find_on_path("codex").into_iter().collect();
    let var = |k: &str| std::env::var_os(k).map(PathBuf::from).filter(|p| p.is_absolute());
    if let Some(appdata) = var("APPDATA") {
        out.push(appdata.join("npm").join("codex.cmd"));
    }
    if let Some(local) = var("LOCALAPPDATA") {
        out.push(local.join("Volta").join("bin").join("codex.exe"));
        out.push(local.join("pnpm").join("codex.cmd"));
    }
    if let Some(home) = var("USERPROFILE") {
        out.push(home.join(".bun").join("bin").join("codex.exe"));
        out.push(home.join(".local").join("bin").join("codex.exe"));
    }
    out.retain(|p| p.is_file());
    out
}

// ── Who we are ────────────────────────────────────────────────────────────────
//
// Named pipes share one machine-wide namespace, so the SID in the name is what
// keeps two accounts on the same machine from ever meeting on `coucou-*`.
// coucou-hook computes the same string (hook/src/win.rs) and additionally checks
// that the process serving the pipe really is us.

/// The SID of the account this process runs as, as `S-1-5-21-…`.
pub fn current_user_sid() -> Option<String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;

        // First call sizes the buffer, second fills it.
        let mut needed = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
        if needed == 0 {
            let _ = CloseHandle(token);
            return None;
        }
        let mut buf = vec![0u8; needed as usize];
        let ok = GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )
        .is_ok();
        let _ = CloseHandle(token);
        if !ok {
            return None;
        }

        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut text).ok()?;
        let sid = text.to_string().ok();
        let _ = LocalFree(Some(HLOCAL(text.0 as *mut _)));
        sid
    }
}

// The display name only feeds Mochi's greeting (identity.rs). The calls are
// declared here by hand, with their documented C signatures, so they need no
// extra `windows` crate features.
#[link(name = "secur32")]
extern "system" {
    fn GetUserNameExW(name_format: i32, name_buffer: *mut u16, size: *mut u32) -> u8;
}

#[link(name = "netapi32")]
extern "system" {
    fn NetUserGetInfo(server: *const u16, user: *const u16, level: u32, buffer: *mut *mut u8) -> u32;
    fn NetApiBufferFree(buffer: *mut core::ffi::c_void) -> u32;
}

/// EXTENDED_NAME_FORMAT::NameDisplay.
const NAME_DISPLAY: i32 = 3;

/// The account's display name ("Louis Raille"): the directory's for a domain
/// or Entra account, else the local account's "Full name", else nothing.
pub fn user_full_name() -> Option<String> {
    directory_display_name().or_else(local_full_name).filter(|n| !n.trim().is_empty())
}

fn directory_display_name() -> Option<String> {
    let mut buf = [0u16; 256];
    let mut size = buf.len() as u32;
    // On success `size` is the length copied, without the terminating null.
    let ok = unsafe { GetUserNameExW(NAME_DISPLAY, buf.as_mut_ptr(), &mut size) } != 0;
    if !ok || size as usize > buf.len() {
        return None;
    }
    String::from_utf16(&buf[..size as usize]).ok()
}

fn local_full_name() -> Option<String> {
    let user = std::env::var("USERNAME").ok().filter(|u| !u.is_empty())?;
    let wide: Vec<u16> = user.encode_utf16().chain(std::iter::once(0)).collect();
    let mut info: *mut u8 = std::ptr::null_mut();
    // Level 10 is USER_INFO_10 { name, comment, usr_comment, full_name }, four
    // wide-string pointers; any user may read it about themselves.
    let status = unsafe { NetUserGetInfo(std::ptr::null(), wide.as_ptr(), 10, &mut info) };
    if status != 0 || info.is_null() {
        return None;
    }
    unsafe {
        let full = *(info as *const *const u16).add(3);
        let text = if full.is_null() {
            None
        } else {
            let mut len = 0usize;
            while len < 256 && *full.add(len) != 0 {
                len += 1;
            }
            String::from_utf16(std::slice::from_raw_parts(full, len)).ok()
        };
        NetApiBufferFree(info.cast());
        text
    }
}

// ── Cursor ────────────────────────────────────────────────────────────────────

/// The 60 Hz poll reads the cursor and flips click-through from it.
pub const CURSOR_POLL: bool = true;

/// Cursor position in physical screen pixels.
pub fn cursor_physical() -> Option<(f64, f64)> {
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p).ok()? };
    Some((p.x as f64, p.y as f64))
}

/// True while the left mouse button is held — the only signal we get that a
/// drag might be in flight before it reaches the window.
pub fn left_button_down() -> bool {
    unsafe { (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0 }
}

// ── Island window ─────────────────────────────────────────────────────────────

fn hwnd_of(win: &WebviewWindow) -> Option<HWND> {
    let raw = win.hwnd().ok()?.0 as isize;
    if raw == 0 {
        return None;
    }
    Some(HWND(raw as *mut _))
}

/// Keeps the settings window from showing the "no drop" cursor.
///
/// wry installs its drop target by walking the webview's child windows **once**,
/// when the webview is created. WebView2 creates `Chrome_RenderWidgetHostHWND`
/// later and registers its own target on it; being the innermost window, that one
/// wins, and since the page has no HTML5 drop handler it refuses everything.
/// Revoking it makes OLE fall through to wry's target. The island is the other
/// way round: it takes drops through WebView2's own target (see webview_drop.rs),
/// so its windows are left alone.
///
/// Cheap and idempotent, so it is simply re-run whenever a drag might be starting.
pub fn unblock_webview_drops(app: &AppHandle) {
    let Some(win) = app.get_webview_window("settings") else { return };
    let Some(hwnd) = hwnd_of(&win) else { return };
    unsafe {
        let _ = EnumChildWindows(Some(hwnd), Some(revoke_render_widget), LPARAM(0));
    }
}

unsafe extern "system" fn revoke_render_widget(hwnd: HWND, _: LPARAM) -> BOOL {
    let mut name = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut name) };
    if len > 0 {
        let class = String::from_utf16_lossy(&name[..len as usize]);
        if class == "Chrome_RenderWidgetHostHWND" {
            let _ = unsafe { RevokeDragDrop(hwnd) };
        }
    }
    true.into()
}

/// WS_EX_NOACTIVATE keeps clicks from stealing focus; WS_EX_TOOLWINDOW keeps the
/// island out of Alt-Tab.
pub fn make_non_activating(win: &WebviewWindow) {
    let Some(hwnd) = hwnd_of(win) else { return };
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let want = ex | WS_EX_NOACTIVATE.0 as isize | WS_EX_TOOLWINDOW.0 as isize;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, want);
    }
}

/// Temporarily allow activation so a text field inside the island can be typed in.
pub fn set_activating(win: &WebviewWindow, activating: bool) {
    let Some(hwnd) = hwnd_of(win) else { return };
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let want = if activating {
            ex & !(WS_EX_NOACTIVATE.0 as isize)
        } else {
            ex | WS_EX_NOACTIVATE.0 as isize
        };
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, want);
    }
}

/// Click-through here is the poll's WS_EX_TRANSPARENT toggle, not a region.
pub fn set_input_region(_win: &WebviewWindow, _rect: Option<(f64, f64, f64, f64)>) {}

/// The cursor poll already parks itself with the island (island.rs): nothing to do.
pub fn set_pointer_watch(_active: bool) {}

/// `set_position` already places a Win32 window on the right display.
pub fn pin_to_monitor(_win: &WebviewWindow, _x: i32, _y: i32) {}

// ── Session windows ("Open terminal") ─────────────────────────────────────────
//
// See session_window.rs: the relay's ancestors lead to the terminal or editor
// window a Claude Code session runs in.

/// The process on the other end of a relay connection.
pub fn pipe_client_pid(handle: RawHandle) -> Option<u32> {
    let mut pid = 0u32;
    unsafe { GetNamedPipeClientProcessId(HANDLE(handle), &mut pid).ok()? };
    (pid != 0).then_some(pid)
}

/// Every process, by ID: its parent and its executable's name.
fn process_table() -> HashMap<u32, Proc> {
    let mut out = HashMap::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            let len = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(entry.szExeFile.len());
            out.insert(
                entry.th32ProcessID,
                Proc {
                    parent: entry.th32ParentProcessID,
                    exe: String::from_utf16_lossy(&entry.szExeFile[..len]),
                },
            );
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    out
}

/// The ancestors of a process, nearest first, below the desktop shell.
pub fn process_ancestors(pid: u32) -> Vec<u32> {
    session_window::ancestors(&process_table(), pid)
}

/// Visible top-level windows that are nobody's dialog and have a title:
/// (handle, process ID, title).
fn top_windows() -> Vec<(isize, u32, String)> {
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = unsafe { &mut *(lparam.0 as *mut Vec<(isize, u32, String)>) };
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || GetWindow(hwnd, GW_OWNER).is_ok() {
                return true.into();
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
            let mut text = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut text);
            if len > 0 {
                list.push((hwnd.0 as isize, pid, String::from_utf16_lossy(&text[..len as usize])));
            }
        }
        true.into()
    }
    let mut list: Vec<(isize, u32, String)> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut list as *mut _ as isize));
    }
    list
}

/// The first of `pids` that owns a window.
pub fn first_with_window(pids: &[u32]) -> Option<u32> {
    let windows = top_windows();
    pids.iter().copied().find(|pid| windows.iter().any(|(_, owner, _)| owner == pid))
}

/// What a session remembers: the ancestor that owns a window, or nothing.
pub fn window_owners(ancestors: &[u32]) -> Vec<u32> {
    first_with_window(ancestors).into_iter().collect()
}

fn bring_forward(raw: isize) -> bool {
    let hwnd = HWND(raw as *mut _);
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        // Allowed: the click on the island that asked for this was the last input.
        SetForegroundWindow(hwnd).as_bool()
    }
}

/// Brings `pid`'s window forward — the one titled after `folder` if there are several.
pub fn focus_process_window(pid: u32, folder: &str) -> bool {
    let candidates: Vec<(isize, String)> = top_windows()
        .into_iter()
        .filter(|(_, owner, _)| *owner == pid)
        .map(|(hwnd, _, title)| (hwnd, title))
        .collect();
    session_window::pick_window(&candidates, folder).is_some_and(|hwnd| bring_forward(*hwnd))
}

/// Brings forward the window a session remembered (`window_owners`).
pub fn focus_session_window(owners: &[u32], folder: &str) -> bool {
    owners.first().is_some_and(|pid| focus_process_window(*pid, folder))
}

/// Brings forward the window of a running app, by executable name.
fn focus_app(exe: &str) -> bool {
    let pids: Vec<u32> = process_table()
        .into_iter()
        .filter(|(_, p)| p.exe.eq_ignore_ascii_case(exe))
        .map(|(pid, _)| pid)
        .collect();
    top_windows()
        .into_iter()
        .find(|(_, owner, _)| pids.contains(owner))
        .is_some_and(|(hwnd, _, _)| bring_forward(hwnd))
}

/// The Claude desktop app: brought forward when it runs (Claude Code's own
/// `claude.exe` has no window of its own, so it is never the one picked),
/// started from the Squirrel path or its registered MSIX protocol otherwise.
pub fn open_claude_desktop() -> bool {
    open_claude_with(
        || focus_app("claude.exe"),
        || {
            std::env::var_os("LOCALAPPDATA").is_some_and(|base| {
                let exe = PathBuf::from(base).join("AnthropicClaude").join("claude.exe");
                exe.is_file() && Command::new(exe).spawn().is_ok()
            })
        },
        activate_claude_protocol,
    )
}

fn open_claude_with(
    focus: impl FnOnce() -> bool,
    squirrel: impl FnOnce() -> bool,
    protocol: impl FnOnce() -> bool,
) -> bool {
    focus() || squirrel() || protocol()
}

/// The MSIX manifest registers `claude`; let Windows resolve its current install.
fn activate_claude_protocol() -> bool {
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        // A Tauri thread may already use MTA. Keep that apartment: the Shell can
        // delegate activation, and we must not uninitialize someone else's COM.
        if initialized.is_err() && initialized != RPC_E_CHANGED_MODE {
            return false;
        }
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC,
            lpVerb: w!("open"),
            lpFile: w!("claude://"),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        let opened = ShellExecuteExW(&mut info).is_ok();
        if initialized.is_ok() {
            CoUninitialize();
        }
        opened
    }
}

#[cfg(test)]
mod claude_launch_tests {
    use super::open_claude_with;
    use std::cell::RefCell;

    #[test]
    fn tries_launchers_in_order_and_stops_after_success() {
        for (focus, squirrel, protocol, expected, calls) in [
            (true, true, true, true, vec!["focus"]),
            (false, true, true, true, vec!["focus", "squirrel"]),
            (false, false, true, true, vec!["focus", "squirrel", "protocol"]),
            (false, false, false, false, vec!["focus", "squirrel", "protocol"]),
        ] {
            let seen = RefCell::new(Vec::new());
            let result = open_claude_with(
                || {
                    seen.borrow_mut().push("focus");
                    focus
                },
                || {
                    seen.borrow_mut().push("squirrel");
                    squirrel
                },
                || {
                    seen.borrow_mut().push("protocol");
                    protocol
                },
            );
            assert_eq!(result, expected);
            assert_eq!(seen.into_inner(), calls);
        }
    }
}

// ── Global shortcuts ──────────────────────────────────────────────────────────

/// RegisterHotKey works in every Windows session.
pub fn global_shortcuts_blocked() -> Option<&'static str> {
    None
}

/// The character Ctrl+Alt(+Shift)+`vk` types on one of the installed keyboard
/// layouts, if any. Windows reads Ctrl+Alt as AltGr, so registering such a
/// combination as a global shortcut would stop the user typing that character
/// (AltGr+E is € on most European layouts, AltGr+A is ą in Polish).
pub fn ctrl_alt_types(vk: u16, shift: bool) -> Option<String> {
    let mut layouts = [HKL::default(); 32];
    let count = unsafe { GetKeyboardLayoutList(Some(&mut layouts[..])) };
    let count = usize::try_from(count).unwrap_or(0).min(layouts.len());

    let mut state = [0u8; 256];
    state[VK_CONTROL.0 as usize] = 0x80;
    state[VK_MENU.0 as usize] = 0x80;
    if shift {
        state[VK_SHIFT.0 as usize] = 0x80;
    }
    for &layout in &layouts[..count] {
        let scan = unsafe { MapVirtualKeyExW(u32::from(vk), MAPVK_VK_TO_VSC, Some(layout)) };
        let mut buf = [0u16; 8];
        // Flag 0x4: leave the keyboard state alone, so a dead key met here
        // doesn't change what the user types next (Windows 10 1607 and later).
        let n = unsafe { ToUnicodeEx(u32::from(vk), scan, &state, &mut buf, 0x4, Some(layout)) };
        let typed = match n {
            // A dead key (´ ^ ¨…) is still something the user types with it.
            n if n < 0 => String::from_utf16_lossy(&buf[..1]),
            0 => continue,
            n => String::from_utf16_lossy(&buf[..(n as usize).min(buf.len())]),
        };
        if typed.chars().any(|c| !c.is_control()) {
            return Some(typed);
        }
    }
    None
}

// ── Desktop Mochi window ──────────────────────────────────────────────────────

/// Windows places a window wherever it is asked, and the cursor poll is there.
pub fn desktop_mode() -> super::DesktopMode {
    super::DesktopMode::Poll
}

/// Same window styles as the island: never takes focus, never in Alt-Tab (it
/// is created without a taskbar button).
pub fn prepare_desktop_window(win: &WebviewWindow, _mode: super::DesktopMode) -> bool {
    make_non_activating(win);
    true
}

/// The cursor poll toggles WS_EX_TRANSPARENT from the body hit test instead.
pub fn set_desktop_shape(_win: &WebviewWindow, _shape: super::MouseShape) {}

/// Layer-shell only (Linux).
pub fn set_layer_margins(_win: &WebviewWindow, _x: f64, _y: f64) {}

/// Layer-shell only (Linux).
pub fn set_layer_overlay(_win: &WebviewWindow, _on: bool) {}

/// Layer-shell only (Linux): the logical size of the display the island is on.
pub fn layer_display(_island: &WebviewWindow, _mochi: &WebviewWindow) -> Option<(f64, f64)> {
    None
}

// ── Flow dictation's pill ─────────────────────────────────────────────────────
// On Windows the pill is placed with set_position and a cursor poll decides
// click-through (flow/pill.rs), like the island.

pub fn prepare_pill_window(win: &WebviewWindow, _mode: super::DesktopMode) -> bool {
    make_non_activating(win);
    true
}

pub fn set_pill_edge(_win: &WebviewWindow, _edge: &str) {}

pub fn set_pill_input(_win: &WebviewWindow, _rects: Vec<(f64, f64, f64, f64)>) {}

// ── Battery & Power status ───────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BatteryStatus {
    pub charging: bool,
    pub level: u8,
    pub has_battery: bool,
}

pub fn battery_status() -> BatteryStatus {
    #[repr(C)]
    struct SystemPowerStatus {
        ac_line_status: u8,
        battery_flag: u8,
        battery_life_percent: u8,
        system_status_flag: u8,
        battery_life_time: u32,
        battery_full_life_time: u32,
    }

    extern "system" {
        fn GetSystemPowerStatus(lp: *mut SystemPowerStatus) -> i32;
    }

    let mut sps = SystemPowerStatus {
        ac_line_status: 255,
        battery_flag: 255,
        battery_life_percent: 255,
        system_status_flag: 0,
        battery_life_time: 0,
        battery_full_life_time: 0,
    };

    let ok = unsafe { GetSystemPowerStatus(&mut sps) != 0 };
    if !ok {
        return BatteryStatus::default();
    }

    let has_battery = sps.battery_flag != 128 && sps.battery_life_percent <= 100;
    let charging = sps.ac_line_status == 1;
    let level = if sps.battery_life_percent <= 100 {
        sps.battery_life_percent
    } else {
        100
    };

    BatteryStatus {
        charging,
        level,
        has_battery,
    }
}

// ── Bluetooth status ──────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BluetoothStatus {
    pub connected: bool,
    pub name: String,
    pub level: Option<u8>,
    pub device_type: String,
}

pub fn bluetooth_status() -> BluetoothStatus {
    // 1. Query WinRT for connected Bluetooth or BLE devices
    let winrt = winrt_bluetooth_status().ok();

    // 2. Query PnP / SetupAPI for present Bluetooth devices with battery percentage (DEVPKEY_Device_BatteryPercent)
    let pnp_devices = get_pnp_bluetooth_devices();

    if let Some(mut status) = winrt.filter(|s| s.connected) {
        // If WinRT didn't extract battery percentage, look up from PnP device properties
        if status.level.is_none() {
            for (pnp_name, pnp_level) in &pnp_devices {
                if pnp_level.is_some() {
                    let cleaned = clean_bt_name(pnp_name);
                    if cleaned.eq_ignore_ascii_case(&status.name)
                        || status.name.to_lowercase().contains(&cleaned.to_lowercase())
                        || cleaned.to_lowercase().contains(&status.name.to_lowercase())
                    {
                        status.level = *pnp_level;
                        break;
                    }
                }
            }
            // If still no level matched by name, check if any single PnP Bluetooth device reported battery
            if status.level.is_none() {
                if let Some((_, Some(lvl))) = pnp_devices.iter().find(|(_, lvl)| lvl.is_some()) {
                    status.level = Some(*lvl);
                }
            }
        }
        return status;
    }

    // 3. If WinRT didn't return connected, check if PnP has a currently active/present Bluetooth device
    for (pnp_name, pnp_level) in pnp_devices {
        let cleaned = clean_bt_name(&pnp_name);
        if !cleaned.is_empty() && !cleaned.eq_ignore_ascii_case("unknown") {
            let device_type = classify_bt_device(&cleaned);
            return BluetoothStatus {
                connected: true,
                name: cleaned,
                level: pnp_level,
                device_type,
            };
        }
    }

    BluetoothStatus::default()
}

fn get_pnp_bluetooth_devices() -> Vec<(String, Option<u8>)> {
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
        SetupDiGetDevicePropertyW, DIGCF_ALLCLASSES, DIGCF_PRESENT,
        SP_DEVINFO_DATA,
    };
    use windows::Win32::Devices::Properties::DEVPROPTYPE;
    use windows::Win32::Foundation::DEVPROPKEY;
    use windows::core::PCWSTR;

    // DEVPKEY_Device_BatteryPercent: {104ea319-6ee2-4701-bd47-8ddbf425bbe5}, 2
    let pkey_battery = DEVPROPKEY {
        fmtid: windows::core::GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5),
        pid: 2,
    };
    // DEVPKEY_NAME: {b725f130-47ef-101a-a5f1-02608c9eebac}, 10
    let pkey_name = DEVPROPKEY {
        fmtid: windows::core::GUID::from_u128(0xb725f130_47ef_101a_a5f1_02608c9eebac),
        pid: 10,
    };
    // DEVPKEY_Device_FriendlyName: {a45c254e-df1c-4efd-8020-67d146a850e0}, 14
    let pkey_friendly_name = DEVPROPKEY {
        fmtid: windows::core::GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 14,
    };

    let mut results = Vec::new();

    for enumerator in &["BTHENUM", "BTHLE", "BTHHFENUM"] {
        let enum_u16: Vec<u16> = enumerator.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let hdev = match SetupDiGetClassDevsW(
                None,
                PCWSTR(enum_u16.as_ptr()),
                None,
                DIGCF_PRESENT | DIGCF_ALLCLASSES,
            ) {
                Ok(h) => h,
                Err(_) => continue,
            };

            let mut dev_data = SP_DEVINFO_DATA {
                cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
                ..Default::default()
            };

            let mut idx = 0;
            while SetupDiEnumDeviceInfo(hdev, idx, &mut dev_data).is_ok() {
                idx += 1;

                let mut prop_type = DEVPROPTYPE(0);
                let mut buf = [0u8; 512];
                let mut req_size = 0u32;

                let mut dev_name = String::new();
                if SetupDiGetDevicePropertyW(
                    hdev,
                    &dev_data,
                    &pkey_friendly_name,
                    &mut prop_type,
                    Some(&mut buf),
                    Some(&mut req_size),
                    0,
                ).is_ok() || SetupDiGetDevicePropertyW(
                    hdev,
                    &dev_data,
                    &pkey_name,
                    &mut prop_type,
                    Some(&mut buf),
                    Some(&mut req_size),
                    0,
                ).is_ok() {
                    let len = (req_size as usize) / 2;
                    if len > 0 && len <= buf.len() / 2 {
                        let u16_slice = std::slice::from_raw_parts(buf.as_ptr() as *const u16, len);
                        dev_name = String::from_utf16_lossy(u16_slice)
                            .trim_matches(char::from(0))
                            .trim()
                            .to_string();
                    }
                }

                if dev_name.is_empty() || dev_name.eq_ignore_ascii_case("unknown") {
                    continue;
                }

                // Check battery property
                let mut bat_type = DEVPROPTYPE(0);
                let mut bat_buf = [0u8; 16];
                let mut bat_req_size = 0u32;
                let mut level = None;
                if SetupDiGetDevicePropertyW(
                    hdev,
                    &dev_data,
                    &pkey_battery,
                    &mut bat_type,
                    Some(&mut bat_buf),
                    Some(&mut bat_req_size),
                    0,
                ).is_ok() && bat_req_size > 0 {
                    let b = bat_buf[0];
                    if b <= 100 {
                        level = Some(b);
                    }
                }

                results.push((dev_name, level));
            }

            let _ = SetupDiDestroyDeviceInfoList(hdev);
        }
    }

    // Sort devices so those with a valid battery reading come first
    results.sort_by(|a, b| b.1.is_some().cmp(&a.1.is_some()));
    results
}

fn clean_bt_name(raw: &str) -> String {
    let mut s = raw.trim();
    for suffix in &[
        " Hands-Free AG",
        " Hands-Free HF Audio",
        " Hands-Free HF",
        " Hands-Free",
        " Avrcp Transport",
        " A2DP SNK",
        " Audio Gateway Service",
        " Personal Area Network Service",
        " Phonebook Access Pse Service",
    ] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            s = stripped.trim();
        }
    }
    s.to_string()
}

fn winrt_bluetooth_status() -> windows::core::Result<BluetoothStatus> {
    use windows::core::HSTRING;
    use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothDevice, BluetoothLEDevice};
    use windows::Devices::Enumeration::DeviceInformation;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };

    // 1. Check Classic Bluetooth connected devices
    if let Ok(selector) = BluetoothDevice::GetDeviceSelectorFromConnectionStatus(BluetoothConnectionStatus::Connected) {
        if let Ok(op) = DeviceInformation::FindAllAsyncAqsFilter(&selector) {
            if let Ok(collection) = op.get() {
                let count = collection.Size().unwrap_or(0);
                for i in 0..count {
                    if let Ok(item) = collection.GetAt(i) {
                        let name = item.Name().unwrap_or_default().to_string();
                        if !name.is_empty() && name != "Unknown" {
                            let level = extract_battery_level(&item);
                            let device_type = classify_bt_device(&name);
                            return Ok(BluetoothStatus {
                                connected: true,
                                name,
                                level,
                                device_type,
                            });
                        }
                    }
                }
            }
        }
    }

    // 2. Check BLE (Bluetooth Low Energy) connected devices
    if let Ok(selector) = BluetoothLEDevice::GetDeviceSelectorFromConnectionStatus(BluetoothConnectionStatus::Connected) {
        if let Ok(op) = DeviceInformation::FindAllAsyncAqsFilter(&selector) {
            if let Ok(collection) = op.get() {
                let count = collection.Size().unwrap_or(0);
                for i in 0..count {
                    if let Ok(item) = collection.GetAt(i) {
                        let name = item.Name().unwrap_or_default().to_string();
                        if !name.is_empty() && name != "Unknown" {
                            let level = extract_battery_level(&item);
                            let device_type = classify_bt_device(&name);
                            return Ok(BluetoothStatus {
                                connected: true,
                                name,
                                level,
                                device_type,
                            });
                        }
                    }
                }
            }
        }
    }

    // 3. Fallback: Query AEP with corrected Windows Bluetooth Classic & BLE GUIDs
    let aep_filter = HSTRING::from(
        "System.Devices.Aep.IsConnected:=System.StructuredQueryType.Boolean#True \
         AND (System.Devices.Aep.ProtocolId:=\"{E0CBF06C-CD8B-4647-BB8A-263B43F0F974}\" \
              OR System.Devices.Aep.ProtocolId:=\"{BB7BB05E-5972-42B5-94FC-76EAA7084D49}\")",
    );
    if let Ok(op) = DeviceInformation::FindAllAsyncAqsFilter(&aep_filter) {
        if let Ok(collection) = op.get() {
            let count = collection.Size().unwrap_or(0);
            for i in 0..count {
                if let Ok(item) = collection.GetAt(i) {
                    let name = item.Name().unwrap_or_default().to_string();
                    if !name.is_empty() && name != "Unknown" {
                        let level = extract_battery_level(&item);
                        let device_type = classify_bt_device(&name);
                        return Ok(BluetoothStatus {
                            connected: true,
                            name,
                            level,
                            device_type,
                        });
                    }
                }
            }
        }
    }

    Ok(BluetoothStatus::default())
}

fn extract_battery_level(item: &windows::Devices::Enumeration::DeviceInformation) -> Option<u8> {
    use windows::core::{Interface, HSTRING};
    use windows::Foundation::IPropertyValue;

    if let Ok(props) = item.Properties() {
        let key1 = HSTRING::from("{104ea319-6ee2-4701-bd47-8ddbf425bbe5} 2");
        let key2 = HSTRING::from("System.Devices.BatteryPercent");
        if let Ok(inspectable) = props.Lookup(&key1).or_else(|_| props.Lookup(&key2)) {
            if let Ok(pv) = inspectable.cast::<IPropertyValue>() {
                if let Ok(v) = pv.GetUInt8() {
                    return Some(v.min(100));
                } else if let Ok(v) = pv.GetInt32() {
                    return Some((v.clamp(0, 100)) as u8);
                }
            }
        }
    }
    None
}

fn classify_bt_device(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.contains("bud")
        || lower.contains("pod")
        || lower.contains("headphone")
        || lower.contains("headset")
        || lower.contains("ear")
        || lower.contains("tws")
        || lower.contains("air")
        || lower.contains("audio")
    {
        "headphones".to_string()
    } else {
        "audio".to_string()
    }
}

// ── Privacy & Hardware Indicators (Webcam / Mic / Global Mute) ───────────

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyStatus {
    pub mic_active: bool,
    pub camera_active: bool,
    pub mic_muted: bool,
    pub active_mic_apps: Vec<String>,
    pub active_camera_apps: Vec<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DownloadStatus {
    pub active: bool,
    pub filename: String,
    pub speed_bytes_per_sec: u64,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub percent: Option<u8>,
    #[serde(default)]
    pub target_path: Option<String>,
}

pub fn get_mic_mute() -> bool {
    prepare_environment();
    unsafe {
        let Ok(devices) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else {
            return false;
        };
        let Ok(device) = devices.GetDefaultAudioEndpoint(eCapture, eConsole) else {
            return false;
        };
        let Ok(volume) = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) else {
            return false;
        };
        volume.GetMute().map(|b| b.as_bool()).unwrap_or(false)
    }
}

pub fn toggle_mic_mute() -> bool {
    prepare_environment();
    unsafe {
        let Ok(devices) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else {
            return false;
        };
        let Ok(device) = devices.GetDefaultAudioEndpoint(eCapture, eConsole) else {
            return false;
        };
        let Ok(volume) = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) else {
            return false;
        };
        let current = volume.GetMute().map(|b| b.as_bool()).unwrap_or(false);
        let new_state = !current;
        let _ = volume.SetMute(new_state, std::ptr::null());
        new_state
    }
}

pub fn change_system_volume(delta: i32) -> i32 {
    prepare_environment();
    unsafe {
        let Ok(devices) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else {
            return 50;
        };
        let Ok(device) = devices.GetDefaultAudioEndpoint(eRender, eMultimedia) else {
            return 50;
        };
        let Ok(volume) = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) else {
            return 50;
        };
        let current = volume.GetMasterVolumeLevelScalar().unwrap_or(0.5);
        let new_vol = (current + (delta as f32 / 100.0)).clamp(0.0, 1.0);
        let _ = volume.SetMasterVolumeLevelScalar(new_vol, std::ptr::null());
        if delta > 0 {
            let _ = volume.SetMute(false, std::ptr::null());
        }
        (new_vol * 100.0).round() as i32
    }
}

static CURRENT_BRIGHTNESS: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(70);

pub fn change_system_brightness(delta: i32) -> i32 {
    let cur = CURRENT_BRIGHTNESS.load(std::sync::atomic::Ordering::Relaxed);
    let new_val = (cur + delta).clamp(10, 100);
    CURRENT_BRIGHTNESS.store(new_val, std::sync::atomic::Ordering::Relaxed);

    std::thread::spawn(move || {
        let cmd = format!(
            "(Get-WmiObject -Namespace root/WMI -Class WmiMonitorBrightnessMethods).WmiSetBrightness(1, {})",
            new_val
        );
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &cmd]);
        #[cfg(windows)]
        c.creation_flags(0x08000000); // CREATE_NO_WINDOW
        let _ = c.output();
    });

    new_val
}

fn check_key_children(subpath: &str, active: &mut bool, apps: &mut Vec<String>) {
    let subpath_u16: Vec<u16> = subpath.encode_utf16().chain(std::iter::once(0)).collect();
    let mut hkey = HKEY::default();
    unsafe {
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            ::windows::core::PCWSTR(subpath_u16.as_ptr()),
            Some(0),
            KEY_READ,
            &mut hkey,
        )
        .is_err()
        {
            return;
        }

        let mut index = 0;
        let mut name_buf = [0u16; 512];
        loop {
            let mut name_len = name_buf.len() as u32;
            let res = RegEnumKeyExW(
                hkey,
                index,
                Some(PWSTR(name_buf.as_mut_ptr())),
                &mut name_len,
                None,
                None,
                None,
                None,
            );
            if res.is_err() {
                break;
            }
            index += 1;
            let child_name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
            if child_name == "NonPackaged" {
                continue;
            }

            let mut child_key = HKEY::default();
            let child_u16: Vec<u16> = child_name.encode_utf16().chain(std::iter::once(0)).collect();
            if RegOpenKeyExW(
                hkey,
                ::windows::core::PCWSTR(child_u16.as_ptr()),
                Some(0),
                KEY_READ,
                &mut child_key,
            )
            .is_ok()
            {
                let mut start: u64 = 0;
                let mut stop: u64 = 0;
                let mut len = std::mem::size_of::<u64>() as u32;

                let val_start = w!("LastUsedTimeStart");
                let val_stop = w!("LastUsedTimeStop");

                let q_start = RegQueryValueExW(
                    child_key,
                    val_start,
                    None,
                    None,
                    Some(&mut start as *mut u64 as *mut u8),
                    Some(&mut len),
                );
                len = std::mem::size_of::<u64>() as u32;
                let q_stop = RegQueryValueExW(
                    child_key,
                    val_stop,
                    None,
                    None,
                    Some(&mut stop as *mut u64 as *mut u8),
                    Some(&mut len),
                );

                let _ = RegCloseKey(child_key);

                if q_start.is_ok() && q_stop.is_ok() {
                    if stop == 0 || stop < start {
                        *active = true;
                        let app_name = clean_app_name(&child_name);
                        if !app_name.is_empty() {
                            apps.push(app_name);
                        }
                    }
                }
            }
        }
        let _ = RegCloseKey(hkey);
    }
}

fn clean_app_name(raw: &str) -> String {
    if raw.contains('#') {
        let exe = raw.rsplit('#').next().unwrap_or(raw);
        let name = exe.strip_suffix(".exe").unwrap_or(exe);
        match name.to_lowercase().as_str() {
            "chrome" => "Google Chrome".into(),
            "msedge" => "Microsoft Edge".into(),
            "firefox" => "Firefox".into(),
            "zoom" => "Zoom".into(),
            "teams" => "Microsoft Teams".into(),
            "discord" => "Discord".into(),
            "obs64" | "obs32" | "obs" => "OBS Studio".into(),
            "code" => "VS Code".into(),
            _ => {
                let mut c = name.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                }
            }
        }
    } else {
        let part = raw.split('_').next().unwrap_or(raw);
        let name = part.split('.').last().unwrap_or(part);
        name.to_string()
    }
}

fn check_consent_store(subpath: &str) -> (bool, Vec<String>) {
    let mut active = false;
    let mut apps = Vec::new();

    check_key_children(subpath, &mut active, &mut apps);

    let non_pkg = format!("{}\\NonPackaged", subpath);
    check_key_children(&non_pkg, &mut active, &mut apps);

    apps.sort();
    apps.dedup();
    (active, apps)
}

pub fn privacy_status() -> PrivacyStatus {
    let (mic_active, active_mic_apps) = check_consent_store(
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone",
    );
    let (camera_active, active_camera_apps) = check_consent_store(
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam",
    );
    let mic_muted = get_mic_mute();

    PrivacyStatus {
        mic_active,
        camera_active,
        mic_muted,
        active_mic_apps,
        active_camera_apps,
    }
}

pub fn downloads_dir() -> Option<PathBuf> {
    // 1. Check registry User Shell Folders for official Downloads location
    let subpath = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\User Shell Folders";
    let subpath_u16: Vec<u16> = subpath.encode_utf16().chain(std::iter::once(0)).collect();
    let val_name = "{374DE290-123F-4565-9164-39C4925E467B}";
    let val_u16: Vec<u16> = val_name.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            ::windows::core::PCWSTR(subpath_u16.as_ptr()),
            Some(0),
            KEY_READ,
            &mut hkey,
        ).is_ok() {
            let mut buf = [0u16; 512];
            let mut buf_len = (buf.len() * 2) as u32;
            if RegQueryValueExW(
                hkey,
                ::windows::core::PCWSTR(val_u16.as_ptr()),
                None,
                None,
                Some(buf.as_mut_ptr() as *mut u8),
                Some(&mut buf_len),
            ).is_ok() {
                let len = (buf_len / 2) as usize;
                let s = String::from_utf16_lossy(&buf[..len.saturating_sub(1)]);
                let resolved = if s.starts_with("%USERPROFILE%") {
                    if let Some(user) = std::env::var_os("USERPROFILE") {
                        s.replacen("%USERPROFILE%", &user.to_string_lossy(), 1)
                    } else {
                        s
                    }
                } else {
                    s
                };
                let p = PathBuf::from(resolved);
                if p.is_dir() {
                    let _ = RegCloseKey(hkey);
                    return Some(p);
                }
            }
            let _ = RegCloseKey(hkey);
        }
    }

    // 2. Standard USERPROFILE/Downloads
    if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
        let p = home.join("Downloads");
        if p.is_dir() {
            return Some(p);
        }
    }

    // 3. OneDrive/Downloads
    if let Some(onedrive) = std::env::var_os("OneDrive").map(PathBuf::from) {
        let p = onedrive.join("Downloads");
        if p.is_dir() {
            return Some(p);
        }
    }

    None
}

struct ActiveCandidate {
    display_name: String,
    target_path: PathBuf,
    len: u64,
    modified: std::time::SystemTime,
    is_unconfirmed: bool,
}

pub fn check_download_status() -> DownloadStatus {
    let Some(dir) = downloads_dir() else {
        return DownloadStatus::default();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return DownloadStatus::default();
    };

    let now = std::time::SystemTime::now();
    let mut candidates = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        let is_crdownload = name.ends_with(".crdownload");
        let is_part = name.ends_with(".part");
        let is_opdownload = name.ends_with(".opdownload");
        let is_download = name.ends_with(".download");
        let is_partial = name.ends_with(".partial");

        if !is_crdownload && !is_part && !is_opdownload && !is_download && !is_partial {
            continue;
        }

        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        let Ok(elapsed) = now.duration_since(modified) else {
            continue;
        };

        // An active download MUST have been written to recently.
        // Stale or dead files (> 30s since last write) are ignored.
        if elapsed > std::time::Duration::from_secs(30) {
            continue;
        }

        let (clean_name, target_file) = if is_crdownload {
            let clean = name.strip_suffix(".crdownload").unwrap_or(name);
            (clean, dir.join(clean))
        } else if is_part {
            let clean = name.strip_suffix(".part").unwrap_or(name);
            (clean, dir.join(clean))
        } else if is_opdownload {
            let clean = name.strip_suffix(".opdownload").unwrap_or(name);
            (clean, dir.join(clean))
        } else if is_partial {
            let clean = name.strip_suffix(".partial").unwrap_or(name);
            (clean, dir.join(clean))
        } else {
            let clean = name.strip_suffix(".download").unwrap_or(name);
            (clean, dir.join(clean))
        };

        let is_unconfirmed = clean_name.starts_with("Unconfirmed ");
        let display_name = if is_unconfirmed {
            "Mengunduh file...".to_string()
        } else {
            clean_name.to_string()
        };

        candidates.push(ActiveCandidate {
            display_name,
            target_path: target_file,
            len: metadata.len(),
            modified,
            is_unconfirmed,
        });
    }

    if candidates.is_empty() {
        return DownloadStatus::default();
    }

    // Sort: confirmed filenames take priority over "Unconfirmed ...", then newest modified time first.
    candidates.sort_by(|a, b| {
        if a.is_unconfirmed != b.is_unconfirmed {
            return a.is_unconfirmed.cmp(&b.is_unconfirmed);
        }
        b.modified.cmp(&a.modified)
    });

    let best = &candidates[0];
    DownloadStatus {
        active: true,
        filename: best.display_name.clone(),
        speed_bytes_per_sec: 0,
        downloaded_bytes: best.len,
        total_bytes: None,
        percent: None,
        target_path: Some(best.target_path.to_string_lossy().to_string()),
    }
}


//! Stage-A feasibility spike for TaskbarIsland v0.11.0.
//!
//! Question: is a plain GDI child window embedded in the Win11 taskbar
//! (`Shell_TrayWnd`) stable on this machine - visible, above the XAML bridge
//! (`Windows.UI.Composition.DesktopWindowContentBridge`), surviving Start /
//! flyouts, and re-embedding after `explorer.exe` restarts?
//!
//! Design notes
//! - Raw Win32 FFI, zero dependencies: the spike must build `--offline` and
//!   stay independent of the app's `windows` crate feature set.
//! - `WS_CHILD` windows never receive `HWND_BROADCAST`, so a *hidden top-level
//!   relay window* owns the `TaskbarCreated` registered message, the 100 ms
//!   observability poll and the 120 s self-terminate timer.
//! - Hard safety cap: the process kills itself after 120 s (driver.ps1 also
//!   has a Stop-Process backstop).
//! - Log: `log.txt` next to the script (override with env `SPIKE_LOG`).
//!
//! Every log line is `[+<ms since start>][epoch=<unix ms>] <text>` so the
//! driver can correlate its own wall-clock actions with spike events.

#![windows_subsystem = "windows"]
#![allow(non_snake_case, dead_code)]

use std::cell::RefCell;
use std::ffi::c_void;
use std::io::Write;
use std::ptr;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ------------------------------------------------------------------ FFI types
type HWND = *mut c_void;
type HINSTANCE = *mut c_void;
type HMODULE = *mut c_void;
type HICON = *mut c_void;
type HCURSOR = *mut c_void;
type HBRUSH = *mut c_void;
type HMENU = *mut c_void;
type HDC = *mut c_void;
type WPARAM = usize;
type LPARAM = isize;
type LRESULT = isize;
type UINT = u32;
type DWORD = u32;
type BOOL = i32;
type LONG = i32;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RECT {
    left: LONG,
    top: LONG,
    right: LONG,
    bottom: LONG,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct POINT {
    x: LONG,
    y: LONG,
}

#[repr(C)]
struct MSG {
    hwnd: HWND,
    message: UINT,
    wParam: WPARAM,
    lParam: LPARAM,
    time: DWORD,
    pt: POINT,
    lPrivate: DWORD,
}

#[repr(C)]
struct PAINTSTRUCT {
    hdc: HDC,
    fErase: BOOL,
    rcPaint: RECT,
    fRestore: BOOL,
    fIncUpdate: BOOL,
    rgbReserved: [u8; 32],
}

type WNDPROC = Option<unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT>;

#[repr(C)]
struct WNDCLASSEXW {
    cbSize: UINT,
    style: UINT,
    lpfnWndProc: WNDPROC,
    cbClsExtra: i32,
    cbWndExtra: i32,
    hInstance: HINSTANCE,
    hIcon: HICON,
    hCursor: HCURSOR,
    hbrBackground: HBRUSH,
    lpszMenuName: *const u16,
    lpszClassName: *const u16,
    hIconSm: HICON,
}

#[link(name = "user32")]
extern "system" {
    fn FindWindowW(cls: *const u16, title: *const u16) -> HWND;
    fn RegisterClassExW(wc: *const WNDCLASSEXW) -> u16;
    fn CreateWindowExW(
        ex: DWORD,
        cls: *const u16,
        title: *const u16,
        style: DWORD,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: HWND,
        menu: HMENU,
        hinstance: HINSTANCE,
        param: *mut c_void,
    ) -> HWND;
    fn DefWindowProcW(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
    fn DestroyWindow(hwnd: HWND) -> BOOL;
    fn SetParent(child: HWND, new_parent: HWND) -> HWND;
    fn GetParent(child: HWND) -> HWND;
    fn SetWindowLongPtrW(hwnd: HWND, idx: i32, val: isize) -> isize;
    fn GetWindowLongPtrW(hwnd: HWND, idx: i32) -> isize;
    fn SetWindowPos(
        hwnd: HWND,
        insert_after: HWND,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: UINT,
    ) -> BOOL;
    fn GetWindowRect(hwnd: HWND, r: *mut RECT) -> BOOL;
    fn GetClientRect(hwnd: HWND, r: *mut RECT) -> BOOL;
    fn IsWindow(hwnd: HWND) -> BOOL;
    fn IsWindowVisible(hwnd: HWND) -> BOOL;
    fn GetWindow(hwnd: HWND, cmd: UINT) -> HWND;
    fn GetClassNameW(hwnd: HWND, buf: *mut u16, max: i32) -> i32;
    fn GetForegroundWindow() -> HWND;
    fn SendMessageTimeoutW(
        hwnd: HWND,
        msg: UINT,
        wparam: WPARAM,
        lparam: LPARAM,
        flags: UINT,
        timeout: UINT,
        result: *mut usize,
    ) -> LRESULT;
    fn RegisterWindowMessageW(name: *const u16) -> UINT;
    fn GetMessageW(msg: *mut MSG, hwnd: HWND, min: UINT, max: UINT) -> BOOL;
    fn TranslateMessage(msg: *const MSG) -> BOOL;
    fn DispatchMessageW(msg: *const MSG) -> LRESULT;
    fn PostMessageW(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> BOOL;
    fn PostQuitMessage(code: i32);
    fn SetTimer(hwnd: HWND, id: usize, elapse: UINT, callback: usize) -> usize;
    fn KillTimer(hwnd: HWND, id: usize) -> BOOL;
    fn GetDC(hwnd: HWND) -> HDC;
    fn ReleaseDC(hwnd: HWND, dc: HDC) -> i32;
    fn InvalidateRect(hwnd: HWND, r: *const RECT, erase: BOOL) -> BOOL;
    fn UpdateWindow(hwnd: HWND) -> BOOL;
    fn SetProcessDpiAwarenessContext(value: *mut c_void) -> BOOL;
    fn SetProcessDPIAware() -> BOOL;
}

#[link(name = "gdi32")]
extern "system" {
    fn BeginPaint(hwnd: HWND, ps: *mut PAINTSTRUCT) -> HDC;
    fn EndPaint(hwnd: HWND, ps: *const PAINTSTRUCT) -> BOOL;
    fn FillRect(dc: HDC, r: *const RECT, brush: HBRUSH) -> i32;
    fn CreateSolidBrush(color: u32) -> HBRUSH;
    fn DeleteObject(obj: *mut c_void) -> BOOL;
    fn GetPixel(dc: HDC, x: i32, y: i32) -> u32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> HMODULE;
    fn LoadLibraryW(name: *const u16) -> HMODULE;
    fn GetProcAddress(module: HMODULE, name: *const u8) -> *mut c_void;
    fn GetCurrentProcessId() -> DWORD;
    fn SetLastError(code: DWORD);
}

// ------------------------------------------------------------------ constants
const WS_POPUP: DWORD = 0x8000_0000;
const WS_VISIBLE: DWORD = 0x1000_0000;
const WS_CHILD: DWORD = 0x4000_0000;
const WS_EX_NOACTIVATE: DWORD = 0x0800_0000;
const GWL_STYLE: i32 = -16;
const GWL_EXSTYLE: i32 = -20;
const SWP_NOSIZE: UINT = 0x0001;
const SWP_NOMOVE: UINT = 0x0002;
const SWP_NOZORDER: UINT = 0x0004;
const SWP_NOACTIVATE: UINT = 0x0010;
const SWP_FRAMECHANGED: UINT = 0x0020;
const SWP_SHOWWINDOW: UINT = 0x0040;
const WS_EX_TOPMOST: DWORD = 0x0000_0008;
// HWND_TOP as an insert-after value for SetWindowPos: per the official
// win32metadata HWND_TOP = 0 (HWND_TOPMOST = -1, which is a NO-OP for sibling
// order on a WS_CHILD - measured: ok=true, no WM_WINDOWPOSCHANGING, no move).
fn hwnd_top() -> HWND {
    ptr::null_mut()
}
const GW_CHILD: UINT = 5;
const GW_HWNDNEXT: UINT = 2;
const GW_HWNDPREV: UINT = 3;
const WM_DESTROY: UINT = 0x0002;
const WM_GETTEXT: UINT = 0x000D;
const WM_PAINT: UINT = 0x000F;
const WM_CLOSE: UINT = 0x0010;
const WM_ERASEBKGND: UINT = 0x0014;
const WM_NCDESTROY: UINT = 0x0082;
const WM_TIMER: UINT = 0x0113;
const SMTO_ABORTIFHUNG: UINT = 0x0002;
const TIMER_POLL: usize = 1;
const TIMER_KILL: usize = 2;
const POLL_MS: UINT = 100;
const KILL_MS: UINT = 120_000;
const HEARTBEAT: Duration = Duration::from_secs(5);
const CHILD_W: i32 = 240;
const CHILD_H: i32 = 34;
const CHILD_X: i32 = 8; // "empty left area" per spec
const BRIDGE_CLASS: &str = "Windows.UI.Composition.DesktopWindowContentBridge";

// ------------------------------------------------------------------- logging
static START: OnceLock<Instant> = OnceLock::new();
static LOGPATH: OnceLock<String> = OnceLock::new();

fn now_ms() -> u128 {
    START.get().expect("start").elapsed().as_millis()
}

fn epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Append one line to the spike log. Never touches shared state, so it is safe
/// from inside any RefCell borrow.
fn log(msg: &str) {
    let line = format!("[+{}ms][epoch={}] {}\n", now_ms(), epoch_ms(), msg);
    let path = LOGPATH.get().expect("logpath");
    for _ in 0..10 {
        match std::fs::OpenOptions::new().create(true).append(true).open(path) {
            Ok(mut f) => {
                let _ = f.write_all(line.as_bytes());
                let _ = f.flush();
                return;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

// ------------------------------------------------------------------ state
struct State {
    relay: HWND,
    child: HWND,
    taskbar: HWND,
    z_method: String,
    taskbar_created_msg: UINT,
    tbc_t0: Option<Instant>,
    poll_deadline: Option<Instant>,
    last_state: String,
    last_state_log: Instant,
    children_logged: bool,
    paints: u32,
    paint_logged: bool,
    msg_trace: u32,
    heals: u32,
}

thread_local! {
    static ST: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Scoped borrow of the shared state. Closures must NOT call into Win32 APIs
/// that can dispatch messages back into `wnd_proc` (re-entrancy panic).
fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    ST.with(|c| {
        let mut b = c.borrow_mut();
        f(b.as_mut().expect("spike state not initialised"))
    })
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn hinst() -> HINSTANCE {
    unsafe { GetModuleHandleW(ptr::null()) as HINSTANCE }
}

fn find_taskbar() -> HWND {
    unsafe { FindWindowW(wide("Shell_TrayWnd").as_ptr(), ptr::null()) }
}

fn io_err() -> String {
    std::io::Error::last_os_error().to_string()
}

unsafe fn class_of(h: HWND) -> String {
    if h.is_null() {
        return String::new();
    }
    let mut buf = [0u16; 256];
    let n = GetClassNameW(h, buf.as_mut_ptr(), buf.len() as i32);
    if n <= 0 {
        String::new()
    } else {
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

/// Cross-process `GetWindowTextW` can hang on a wedged owner, which would stall
/// the poll and break the 120 s cap - so use `WM_GETTEXT` with a timeout.
unsafe fn title_of(h: HWND) -> String {
    if h.is_null() {
        return String::new();
    }
    let mut buf = [0u16; 256];
    let mut res: usize = 0;
    let r = SendMessageTimeoutW(
        h,
        WM_GETTEXT,
        buf.len(),
        buf.as_mut_ptr() as LPARAM,
        SMTO_ABORTIFHUNG,
        100,
        &mut res,
    );
    if r == 0 {
        return String::new();
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

/// Depth-first search for the XAML bridge among the taskbar's descendants.
unsafe fn find_bridge(h: HWND, depth: u32) -> HWND {
    if h.is_null() || depth > 5 {
        return ptr::null_mut();
    }
    let mut c = GetWindow(h, GW_CHILD);
    let mut i = 0;
    while !c.is_null() && i < 64 {
        if class_of(c) == BRIDGE_CLASS {
            return c;
        }
        c = GetWindow(c, GW_HWNDNEXT);
        i += 1;
    }
    let mut c = GetWindow(h, GW_CHILD);
    let mut i = 0;
    while !c.is_null() && i < 64 {
        let r = find_bridge(c, depth + 1);
        if !r.is_null() {
            return r;
        }
        c = GetWindow(c, GW_HWNDNEXT);
        i += 1;
    }
    ptr::null_mut()
}

/// Classes of the siblings stacked above `h` (GW_HWNDPREV walk, max 64).
unsafe fn siblings_above(h: HWND) -> Vec<String> {
    let mut out = Vec::new();
    let mut p = GetWindow(h, GW_HWNDPREV);
    let mut i = 0;
    while !p.is_null() && i < 64 {
        out.push(class_of(p));
        p = GetWindow(p, GW_HWNDPREV);
        i += 1;
    }
    out
}

/// Spec's literal check: walk GW_HWNDPREV from the *taskbar* looking for our
/// window. For a WS_CHILD this never matches (children are not top-level
/// siblings) - kept for fidelity, `zok` is the meaningful metric.
unsafe fn above_taskbar_toplevel(taskbar: HWND, h: HWND) -> bool {
    let mut p = GetWindow(taskbar, GW_HWNDPREV);
    let mut i = 0;
    while !p.is_null() && i < 64 {
        if p == h {
            return true;
        }
        p = GetWindow(p, GW_HWNDPREV);
        i += 1;
    }
    false
}

unsafe fn taskbar_children(taskbar: HWND) -> Vec<String> {
    let mut out = Vec::new();
    let mut c = GetWindow(taskbar, GW_CHILD);
    let mut i = 0;
    while !c.is_null() && i < 32 {
        out.push(class_of(c));
        c = GetWindow(c, GW_HWNDNEXT);
        i += 1;
    }
    out
}

// ------------------------------------------------------- SHQueryUserNotificationState
type QunsFn = unsafe extern "system" fn(*mut i32) -> i32;

fn quns() -> i32 {
    static F: OnceLock<Option<QunsFn>> = OnceLock::new();
    let f = *F.get_or_init(|| unsafe {
        let h = LoadLibraryW(wide("shell32.dll").as_ptr());
        if h.is_null() {
            return None;
        }
        let p = GetProcAddress(h, b"SHQueryUserNotificationState\0".as_ptr());
        if p.is_null() {
            None
        } else {
            Some(std::mem::transmute::<*mut c_void, QunsFn>(p))
        }
    });
    match f {
        Some(f) => unsafe {
            let mut v = -99i32;
            let hr = f(&mut v);
            if hr == 0 {
                v
            } else {
                -1
            }
        },
        None => -2,
    }
}

// ------------------------------------------------------------- pixel sampling
/// Ground truth for "is our green rect actually on top": read the composited
/// screen pixel at the child's centre. Green (0,255,0) => we are on top.
unsafe fn centre_pixel(r: RECT) -> (i32, i32, i32) {
    if r.right <= r.left || r.bottom <= r.top {
        return (-1, -1, -1);
    }
    let dc = GetDC(ptr::null_mut());
    if dc.is_null() {
        return (-2, -2, -2);
    }
    let cx = (r.left + r.right) / 2;
    let cy = (r.top + r.bottom) / 2;
    let c = GetPixel(dc, cx, cy);
    ReleaseDC(ptr::null_mut(), dc);
    if c == 0xFFFF_FFFF {
        return (-3, -3, -3);
    }
    ((c & 0xFF) as i32, ((c >> 8) & 0xFF) as i32, ((c >> 16) & 0xFF) as i32)
}

// ------------------------------------------------------------------ painting
unsafe fn paint(hwnd: HWND) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    if hdc.is_null() {
        return;
    }
    let r = RECT {
        left: 0,
        top: 0,
        right: CHILD_W,
        bottom: CHILD_H,
    };
    let brush = CreateSolidBrush(0x0000_FF00); // solid GREEN (COLORREF 0x00BBGGRR)
    FillRect(hdc, &r, brush);
    DeleteObject(brush as *mut c_void);
    EndPaint(hwnd, &ps);
}

// ------------------------------------------------------------------- embedding
/// Create the 240x34 window as a plain popup, reparent it into the taskbar,
/// force `WS_CHILD|WS_VISIBLE` (drop WS_POPUP / WS_EX_TOPMOST), place it at
/// x=8 vertically centred, and climb above the XAML bridge.
/// Returns the child HWND, or null (already logged).
unsafe fn create_child(taskbar: HWND) -> HWND {
    let cls = wide("TbiSpikeClass");
    let title = wide("TbiSpikeRect");
    let h = CreateWindowExW(
        // WS_EX_NOACTIVATE only while it is still a popup: a plain WS_POPUP
        // grabs the foreground the moment it exists, which would poison the
        // driver's Start/foreground checks. Cleared with the other ex-styles
        // right after reparenting.
        WS_EX_NOACTIVATE,
        cls.as_ptr(),
        title.as_ptr(),
        WS_POPUP, // not visible yet: no flash at (0,0)
        0,
        0,
        CHILD_W,
        CHILD_H,
        ptr::null_mut(),
        ptr::null_mut(),
        hinst(),
        ptr::null_mut(),
    );
    if h.is_null() {
        log(&format!("error CreateWindowExW failed: {}", io_err()));
        return ptr::null_mut();
    }
    log(&format!("event created hwnd={:p}", h));
    // Publish the handle immediately so WM_PAINT dispatched during the embed
    // sequence below is counted/attributed (and re-arm first-paint logging).
    st(|s| {
        s.child = h;
        s.paint_logged = false;
    });

    // Reparent. SetParent returns the previous parent (null here), so a real
    // failure is only detectable through GetLastError - clear it first.
    unsafe { SetLastError(0) };
    let prev = SetParent(h, taskbar);
    let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
    if prev.is_null() && err != 0 {
        log(&format!("error SetParent failed: {}", io_err()));
        DestroyWindow(h);
        return ptr::null_mut();
    }

    // Styles: WS_CHILD | WS_VISIBLE, no ex-styles (WS_EX_TOPMOST/TOOLWINDOW gone).
    SetWindowLongPtrW(h, GWL_STYLE, (WS_CHILD | WS_VISIBLE) as isize);
    SetWindowLongPtrW(h, GWL_EXSTYLE, 0);

    // Position in the taskbar's client space: empty left area, centred.
    let mut rc = RECT::default();
    GetClientRect(taskbar, &mut rc);
    let th = rc.bottom - rc.top;
    let y = if th > CHILD_H { (th - CHILD_H) / 2 } else { 0 };

    const FLAGS: UINT = SWP_SHOWWINDOW | SWP_FRAMECHANGED | SWP_NOACTIVATE;
    // Attempt 1: HWND_TOP (top of the taskbar's child list).
    SetWindowPos(h, ptr::null_mut(), CHILD_X, y, CHILD_W, CHILD_H, FLAGS);

    let bridge = find_bridge(taskbar, 0);
    let method: String;
    if !bridge.is_null() {
        let above = siblings_above(h).iter().any(|c| c == BRIDGE_CLASS);
        if above {
            // Attempt 2: insert ourselves directly after the bridge => above it.
            let ok = SetWindowPos(h, bridge, CHILD_X, y, CHILD_W, CHILD_H, FLAGS);
            let still = siblings_above(h).iter().any(|c| c == BRIDGE_CLASS);
            method = if ok != 0 && !still {
                "insert-after-bridge (HWND_TOP was not enough)".to_string()
            } else {
                "FAILED: still below the bridge".to_string()
            };
        } else {
            method = "HWND_TOP (above bridge first try)".to_string();
        }
    } else {
        method = "HWND_TOP (bridge class not found among children)".to_string();
    }

    let above_tl = above_taskbar_toplevel(taskbar, h);
    let vis = IsWindowVisible(h) != 0;
    // Force the first WM_PAINT: a window shown via SetWindowPos after its
    // WS_VISIBLE bit was already flipped through SetWindowLongPtr can end up
    // with an empty update region and never repaint.
    InvalidateRect(h, ptr::null(), 1);
    UpdateWindow(h);
    log(&format!(
        "event parented hwnd={:p} taskbar={:p} pos=({}, {}) size={}x{} taskbarH={} z='{}' aboveTl={} visibleAfter={} bridge={:p}",
        h, taskbar, CHILD_X, y, CHILD_W, CHILD_H, th, method, above_tl as u8, vis as u8, bridge
    ));
    let z = method.clone();
    st(|s| {
        s.child = h;
        s.taskbar = taskbar;
        s.z_method = z;
    });
    h
}

/// (Re)embed after the taskbar (or our child) went away. `t0` is when the
/// cause was observed; total latency is logged as `re-parented ... latency=`.
unsafe fn recover(t0: Instant, why: &str) {
    // Wait up to 1 s for Shell_TrayWnd to exist again.
    let mut taskbar = find_taskbar();
    let mut waited = 0u128;
    while taskbar.is_null() && waited < 1000 {
        std::thread::sleep(Duration::from_millis(25));
        waited += 25;
        taskbar = find_taskbar();
    }
    if taskbar.is_null() {
        log(&format!(
            "event recover-failed why={} reason=no-Shell_TrayWnd after {}ms",
            why, waited
        ));
        return;
    }

    let child = st(|s| s.child);
    if !child.is_null() && IsWindow(child) != 0 && GetParent(child) == taskbar {
        log(&format!("event recover-skip why={} already-embedded", why));
        st(|s| s.poll_deadline = None);
        return;
    }
    if !child.is_null() && IsWindow(child) != 0 {
        DestroyWindow(child); // logs `event destroyed`
    }
    let waited_total = t0.elapsed().as_millis();
    if create_child(taskbar).is_null() {
        log(&format!(
            "event recover-failed why={} latency={}ms",
            why, waited_total
        ));
        return;
    }
    st(|s| s.poll_deadline = None);
    log(&format!(
        "event re-parented why={} latency={}ms taskbarWait={}ms",
        why, waited_total, waited
    ));
}

// ------------------------------------------------------------------ poll (100 ms)
unsafe fn poll() {
    let mut taskbar = st(|s| s.taskbar);
    if taskbar.is_null() || IsWindow(taskbar) == 0 {
        let fresh = find_taskbar();
        if !fresh.is_null() && fresh != taskbar {
            log(&format!(
                "event taskbar-handle-changed old={:p} new={:p}",
                taskbar, fresh
            ));
        }
        taskbar = fresh;
        st(|s| s.taskbar = taskbar);
    }

    let child = st(|s| s.child);
    let child_ok = !child.is_null() && IsWindow(child) != 0;

    // Fallback if the TaskbarCreated broadcast is missed (measured on this
    // machine: explorer broadcasts ~2.9 s AFTER Shell_TrayWnd exists, so the
    // poll is normally the fast path): 200 ms grace for an immediate
    // broadcast, then embed from the poll itself.
    if !taskbar.is_null() && (!child_ok || GetParent(child) != taskbar) {
        let deadline = match st(|s| s.poll_deadline) {
            Some(d) => d,
            None => {
                let d = Instant::now() + Duration::from_millis(200);
                st(|s| s.poll_deadline = Some(d));
                d
            }
        };
        if Instant::now() >= deadline {
            st(|s| s.poll_deadline = None);
            log("event poll-detect needs (re)embed");
            recover(Instant::now(), "poll-detect");
        }
    } else {
        st(|s| s.poll_deadline = None);
    }

    // Re-read after the possible recover above.
    let child = st(|s| s.child);
    let taskbar = st(|s| s.taskbar);
    let child_ok = !child.is_null() && IsWindow(child) != 0;

    let vis = child_ok && IsWindowVisible(child) != 0;
    let tbvis = !taskbar.is_null() && IsWindowVisible(taskbar) != 0;
    let mut rect = RECT::default();
    if child_ok {
        GetWindowRect(child, &mut rect);
    }
    let above_tl = if !taskbar.is_null() && child_ok {
        above_taskbar_toplevel(taskbar, child)
    } else {
        false
    };

    let bridge = if !taskbar.is_null() {
        find_bridge(taskbar, 0)
    } else {
        ptr::null_mut()
    };
    let sib = if child_ok {
        siblings_above(child)
    } else {
        Vec::new()
    };
    let bridge_above = !bridge.is_null() && sib.iter().any(|c| c == BRIDGE_CLASS);
    let bridge_found = !bridge.is_null();
    // "above the taskbar" for a WS_CHILD == above the XAML bridge (or, when no
    // bridge class exists, top of the child list).
    let zok = if bridge_found {
        !bridge_above
    } else {
        sib.is_empty()
    };

    let fg = GetForegroundWindow();
    let fg_cls = class_of(fg);
    let fg_title = title_of(fg);
    let start = fg_title.trim().eq_ignore_ascii_case("Start") || fg_cls.contains("Start");
    let q = quns();
    let (pr, pg, pb) = centre_pixel(rect);
    let sib_show: Vec<&str> = sib.iter().take(4).map(|s| s.as_str()).collect();

    let line = format!(
        "STATE vis={} tb={} rect={},{},{},{} aboveTl={} zok={} bridge={} sibAbove={} sib=[{}] start={} quns={} px={},{},{} fg={}|{}",
        vis as u8,
        tbvis as u8,
        rect.left,
        rect.top,
        rect.right,
        rect.bottom,
        above_tl as u8,
        zok as u8,
        bridge_found as u8,
        sib.len(),
        sib_show.join(","),
        start as u8,
        q,
        pr,
        pg,
        pb,
        fg_cls,
        fg_title
    );

    let heartbeat = st(|s| {
        if line != s.last_state || s.last_state_log.elapsed() >= HEARTBEAT {
            s.last_state = line.clone();
            s.last_state_log = Instant::now();
            true
        } else {
            false
        }
    });
    if heartbeat {
        log(&line);
    }

    // Z-order self-heal: explorer creates the XAML bridge AFTER our embed on a
    // fresh shell, and inserts it above us. Two attempts, each verified with an
    // immediate GW_HWNDPREV re-walk (SetWindowPos is synchronous for z-order,
    // so the walk right after the call is ground truth):
    //   1. insert_after=bridge - hWndInsertAfter names the window that ends up
    //      ABOVE the moved window, so this can only ever land us directly
    //      BELOW the bridge (kept for the log to prove the semantics);
    //   2. insert_after=HWND_TOP - top of Shell_TrayWnd's child list, which is
    //      the state the initial embed achieved and held stable for 40 s.
    if child_ok && bridge_found && bridge_above {
        const ZF: UINT = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        let ok1 = SetWindowPos(child, bridge, 0, 0, 0, 0, ZF);
        let after1 = siblings_above(child);
        let mut ok2 = false;
        let mut method = "insert-after-bridge";
        if after1.iter().any(|c| c == BRIDGE_CLASS) {
            ok2 = SetWindowPos(child, hwnd_top(), 0, 0, 0, 0, ZF) != 0;
            method = "hwnd-top";
        }
        let after2 = siblings_above(child);
        let n = st(|s| {
            s.heals += 1;
            s.heals
        });
        log(&format!(
            "event z-heal #{} hwnd={:p} method={} ok1={} after1=[{}] ok2={} after2=[{}]",
            n,
            child,
            method,
            ok1 != 0,
            after1.join(","),
            ok2,
            after2.join(",")
        ));
    }

    // One-shot diagnostics: the taskbar's child stack, for the report.
    if !taskbar.is_null() {
        let done = st(|s| s.children_logged);
        if !done {
            let kids = taskbar_children(taskbar);
            log(&format!(
                "info taskbar-children[{}]: {}",
                kids.len(),
                kids.join(",")
            ));
            st(|s| s.children_logged = true);
        }
    }
}

// ------------------------------------------------------------------ shutdown
unsafe fn shutdown(reason: &str) {
    log(&format!("event self-terminate reason={}", reason));
    let child = st(|s| s.child);
    if !child.is_null() && IsWindow(child) != 0 {
        DestroyWindow(child);
    }
    let relay = st(|s| s.relay);
    if !relay.is_null() && IsWindow(relay) != 0 {
        DestroyWindow(relay); // WM_DESTROY -> PostQuitMessage
    } else {
        PostQuitMessage(0);
    }
}

// ------------------------------------------------------------------ window proc
unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Diagnostic trace: the first non-timer messages any of our windows see
    // (this covers WM_NCCREATE/WM_PAINT/WM_SHOWWINDOW during CreateWindowExW,
    // before the child handle is stored in state).
    if msg != WM_TIMER {
        let traced = st(|s| s.msg_trace);
        if traced < 80 {
            st(|s| s.msg_trace += 1);
            log(&format!(
                "trace hwnd={:p} msg={:#06x} w={:#x} l={:#x}",
                hwnd, msg, wparam, lparam
            ));
        }
    }
    // `TaskbarCreated` is a registered message; never block the broadcast
    // (explorer uses SMTO_ABORTIFHUNG - a slow handler means a missed event).
    let tbc = st(|s| s.taskbar_created_msg);
    if tbc != 0 && msg == tbc {
        if wparam == 0 {
            st(|s| s.tbc_t0 = Some(Instant::now()));
            log("event TaskbarCreated broadcast received (deferred)");
            PostMessageW(hwnd, tbc, 1, 0);
            return 0;
        }
        let t0 = st(|s| s.tbc_t0.take()).unwrap_or_else(Instant::now);
        log(&format!(
            "event TaskbarCreated dispatch-latency={}ms",
            t0.elapsed().as_millis()
        ));
        recover(t0, "TaskbarCreated");
        return 0;
    }

    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            paint(hwnd);
            let child = st(|s| s.child);
            if hwnd == child {
                let n = st(|s| {
                    s.paints += 1;
                    (s.paints, s.paint_logged)
                });
                if !n.1 {
                    log(&format!("event first-paint hwnd={:p} count={}", hwnd, n.0));
                    st(|s| s.paint_logged = true);
                } else if n.0 % 100 == 0 {
                    log(&format!("event paint-count hwnd={:p} count={}", hwnd, n.0));
                }
            }
            0
        }
        WM_TIMER => match wparam {
            TIMER_POLL => {
                poll();
                0
            }
            TIMER_KILL => {
                shutdown("timeout-120s");
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        },
        WM_DESTROY => {
            let relay = st(|s| s.relay);
            if hwnd == relay {
                PostQuitMessage(0);
            }
            // Child destruction is logged from WM_NCDESTROY below: when the
            // taskbar dies, children only get WM_NCDESTROY (observed), so this
            // is not a reliable single source.
            0
        }
        WM_NCDESTROY => {
            let child = st(|s| s.child);
            if hwnd == child {
                log(&format!(
                    "event destroyed hwnd={:p} (own teardown or parent died)",
                    hwnd
                ));
                st(|s| s.child = ptr::null_mut());
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn register_class(name: &str) -> Result<(), String> {
    let cls = wide(name);
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as UINT,
        style: 0,
        lpfnWndProc: Some(wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinst(),
        hIcon: ptr::null_mut(),
        hCursor: ptr::null_mut(),
        hbrBackground: ptr::null_mut(),
        lpszMenuName: ptr::null(),
        lpszClassName: cls.as_ptr(),
        hIconSm: ptr::null_mut(),
    };
    let atom = unsafe { RegisterClassExW(&wc) };
    if atom == 0 {
        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        if err == 1410 {
            // ERROR_CLASS_ALREADY_EXISTS: a previous instance left it behind.
            return Ok(());
        }
        return Err(format!("RegisterClassExW('{}') failed: {}", name, io_err()));
    }
    Ok(())
}

fn main() {
    let code = run();
    // ST is only initialised once the taskbar was found, so read defensively.
    let paints = ST.with(|c| c.borrow().as_ref().map(|s| s.paints).unwrap_or(0));
    log(&format!("event spike-exit code={} paints={}", code, paints));
    std::process::exit(code);
}

fn run() -> i32 {
    START.get_or_init(Instant::now);
    LOGPATH.get_or_init(|| {
        std::env::var("SPIKE_LOG").unwrap_or_else(|_| "log.txt".to_string())
    });

    // Physical pixels everywhere (125% display): PMv2 first, system-aware as
    // the fallback. Must run before the first window/DC.
    unsafe {
        if SetProcessDpiAwarenessContext((-4isize) as *mut c_void) == 0 {
            SetProcessDPIAware();
        }
    }

    log(&format!(
        "event spike-start pid={}",
        unsafe { GetCurrentProcessId() }
    ));

    // 1. Find the taskbar.
    let taskbar = find_taskbar();
    if taskbar.is_null() {
        log("error Shell_TrayWnd not found; exiting non-zero");
        return 2;
    }
    let mut tbrc = RECT::default();
    unsafe { GetWindowRect(taskbar, &mut tbrc) };
    log(&format!(
        "info Shell_TrayWnd={:p} screenRect={},{},{},{}",
        taskbar, tbrc.left, tbrc.top, tbrc.right, tbrc.bottom
    ));

    // Registered broadcast message.
    let tbc = unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) };
    if tbc == 0 {
        log("error RegisterWindowMessageW(TaskbarCreated) failed; exiting");
        return 3;
    }
    log(&format!("info TaskbarCreated message id={:#x}", tbc));

    ST.with(|c| {
        *c.borrow_mut() = Some(State {
            relay: ptr::null_mut(),
            child: ptr::null_mut(),
            taskbar,
            z_method: String::new(),
            taskbar_created_msg: tbc,
            tbc_t0: None,
            poll_deadline: None,
            last_state: String::new(),
            last_state_log: Instant::now(),
            children_logged: false,
            paints: 0,
            paint_logged: false,
            msg_trace: 0,
            heals: 0,
        });
    });

    // 2/3. Classes: a hidden top-level relay (broadcasts + timers live here)
    // and the green child class.
    if let Err(e) = register_class("TbiSpikeRelay") {
        log(&format!("error {}", e));
        return 3;
    }
    if let Err(e) = register_class("TbiSpikeClass") {
        log(&format!("error {}", e));
        return 3;
    }

    let relay_cls = wide("TbiSpikeRelay");
    let relay_title = wide("TbiSpikeRelay");
    let relay = unsafe {
        CreateWindowExW(
            0,
            relay_cls.as_ptr(),
            relay_title.as_ptr(),
            0, // WS_OVERLAPPED top-level, never shown (still gets broadcasts)
            0,
            0,
            0,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            hinst(),
            ptr::null_mut(),
        )
    };
    if relay.is_null() {
        log(&format!("error relay CreateWindowExW failed: {}", io_err()));
        return 3;
    }
    st(|s| s.relay = relay);
    log(&format!("event relay-created hwnd={:p}", relay));

    // 4. Embed the green child.
    let child = unsafe { create_child(taskbar) };
    if child.is_null() {
        log("error could not embed child; exiting non-zero");
        return 4;
    }

    // Timers: 100 ms observability poll + 120 s hard self-terminate.
    unsafe {
        if SetTimer(relay, TIMER_POLL, POLL_MS, 0) == 0 {
            log(&format!("error SetTimer(poll) failed: {}", io_err()));
            return 5;
        }
        if SetTimer(relay, TIMER_KILL, KILL_MS, 0) == 0 {
            log(&format!("error SetTimer(kill) failed: {}", io_err()));
            return 5;
        }
    }
    log(&format!(
        "event ready z='{}' poll={}ms kill={}ms",
        st(|s| s.z_method.clone()),
        POLL_MS,
        KILL_MS
    ));

    // 11. Message loop until WM_QUIT (destroyed window or 120 s cap).
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    loop {
        let r = unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) };
        if r == 0 {
            break;
        }
        if r == -1 {
            log(&format!("error GetMessageW failed: {}", io_err()));
            return 6;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    let relay = st(|s| s.relay);
    unsafe {
        let _ = KillTimer(relay, TIMER_POLL);
        let _ = KillTimer(relay, TIMER_KILL);
    }
    0
}

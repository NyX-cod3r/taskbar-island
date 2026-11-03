//! prototype.rs — interactive test panel (opt-in with `--prototype`).
//!
//! The panel is developer tooling only; the shipping pill gets all of its
//! content from real Windows sources. Buttons:
//! [ Simulate file drag ] [ Collapse island ]

use std::mem::size_of;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, LoadCursorW, PostMessageW, RegisterClassExW,
    ShowWindow, CS_HREDRAW, CS_VREDRAW, HMENU, IDC_ARROW, SW_SHOW,
    WM_COMMAND, WM_DESTROY, WNDCLASSEXW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

pub const PROTOTYPE_CLASS: PCWSTR = w!("TaskbarIslandPrototypeClass");
pub const WM_APP_PROTOTYPE_CMD: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 20;

pub const CMD_SIMULATE_DRAG: usize = 6;
pub const CMD_COLLAPSE: usize = 7;

static mut G_PILL_HWND: isize = 0;

pub fn show_prototype_window(pill_hwnd: HWND) {
    unsafe {
        G_PILL_HWND = pill_hwnd.0 as isize;
        let hinstance = GetModuleHandleW(None).unwrap().into();

        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(proto_wnd_proc),
            hInstance: hinstance,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
            lpszClassName: PROTOTYPE_CLASS,
            ..Default::default()
        };
        RegisterClassExW(&class);

        let hwnd = CreateWindowExW(
            Default::default(),
            PROTOTYPE_CLASS,
            w!("Taskbar Island - Test Panel"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            100,
            100,
            340,
            190,
            None,
            None,
            Some(hinstance),
            None,
        )
        .unwrap();

        // Developer-only controls: no demo data, just geometry/interaction.
        create_button(hwnd, w!("Simulate file drag"), 20, 40, 150, 32, CMD_SIMULATE_DRAG);
        create_button(hwnd, w!("Collapse island"), 180, 40, 130, 32, CMD_COLLAPSE);

        let _ = ShowWindow(hwnd, SW_SHOW);
    }
}

unsafe fn create_button(parent: HWND, text: PCWSTR, x: i32, y: i32, w: i32, h: i32, id: usize) {
    let hinstance = GetModuleHandleW(None).unwrap().into();
    let _ = CreateWindowExW(
        Default::default(),
        w!("BUTTON"),
        text,
        windows::Win32::UI::WindowsAndMessaging::WS_TABSTOP
            | windows::Win32::UI::WindowsAndMessaging::WS_VISIBLE
            | windows::Win32::UI::WindowsAndMessaging::WS_CHILD
            | windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(windows::Win32::UI::WindowsAndMessaging::BS_PUSHBUTTON as u32),
        x,
        y,
        w,
        h,
        Some(parent),
        Some(HMENU(id as *mut _)),
        Some(hinstance),
        None,
    );
}

unsafe extern "system" fn proto_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = wparam.0 & 0xFFFF;
            if id == CMD_SIMULATE_DRAG || id == CMD_COLLAPSE {
                let pill = HWND(G_PILL_HWND as *mut _);
                if !pill.0.is_null() {
                    let _ = PostMessageW(Some(pill), WM_APP_PROTOTYPE_CMD, WPARAM(id), LPARAM(0));
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

//! Windows moves the frame itself during an edge drag. Sample the current client size at most
//! once per frame interval, then flush the last size when the user releases the edge.

use rune::raw_window_handle::RawWindowHandle;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::InvalidateRect;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetClientRect, KillTimer, SIZE_RESTORED, SetTimer, WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_NCDESTROY, WM_PAINT, WM_SIZE, WM_TIMER,
};

const IDLE: usize = 0;
const DRAGGING: usize = 1;
const PENDING: usize = 2;

pub(super) fn install(handle: &rune::WindowHandle) {
    let _ = handle.with_raw_handle(|handle| {
        if let RawWindowHandle::Win32(w) = handle.as_raw() {
            // SAFETY: on_window runs on the window thread. The subclass owns no pointers.
            unsafe { SetWindowSubclass(w.hwnd.get() as HWND, Some(resize), resize as *const () as usize, IDLE) };
        }
    });
}

unsafe extern "system" fn resize(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, id: usize, state: usize) -> LRESULT {
    // SAFETY: Windows supplied the live window and subclass ID. The reference data is one of
    // the three states above. The same ID names the window's resize timer.
    unsafe {
        match msg {
            WM_ENTERSIZEMOVE => {
                SetWindowSubclass(hwnd, Some(resize), id, DRAGGING);
            }
            WM_SIZE if state != IDLE && wp == SIZE_RESTORED as usize && lp != 0 => {
                if state == PENDING {
                    return 0;
                }
                if SetTimer(hwnd, id, 32, None) != 0 {
                    SetWindowSubclass(hwnd, Some(resize), id, PENDING);
                    return 0;
                }
            }
            // A paint for the old swap chain can arrive before the size timer. Windows
            // validates it here. Ask for one fresh paint after the drawable matches the frame.
            WM_PAINT if state == PENDING => return DefWindowProcW(hwnd, msg, wp, lp),
            WM_TIMER if wp == id => {
                if state == PENDING {
                    KillTimer(hwnd, id);
                    SetWindowSubclass(hwnd, Some(resize), id, DRAGGING);
                    deliver_size(hwnd);
                }
                return 0;
            }
            WM_EXITSIZEMOVE => {
                SetWindowSubclass(hwnd, Some(resize), id, IDLE);
                if state == PENDING {
                    KillTimer(hwnd, id);
                    deliver_size(hwnd);
                }
            }
            // A maximize, minimize or zero-sized client takes effect immediately.
            WM_SIZE if state == PENDING => {
                KillTimer(hwnd, id);
                SetWindowSubclass(hwnd, Some(resize), id, DRAGGING);
            }
            WM_NCDESTROY => {
                if state == PENDING {
                    KillTimer(hwnd, id);
                }
                RemoveWindowSubclass(hwnd, Some(resize), id);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}

unsafe fn deliver_size(hwnd: HWND) {
    // SAFETY: called from this window's subclass. Forward the current client dimensions using
    // WM_SIZE's two unsigned 16-bit fields.
    unsafe {
        let mut rect = RECT::default();
        if GetClientRect(hwnd, &mut rect) != 0 {
            let size = (rect.right as u32 & 0xffff) | ((rect.bottom as u32) << 16);
            DefSubclassProc(hwnd, WM_SIZE, SIZE_RESTORED as usize, size as LPARAM);
            InvalidateRect(hwnd, std::ptr::null(), 0);
        }
    }
}

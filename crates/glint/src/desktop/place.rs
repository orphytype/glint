//! The window reopens where it was closed: its frame, its size and whether it was maximized are
//! saved (`window.json`) when it closes, and given to RUNE before it opens next time, unless that
//! place is no longer on any display (a monitor unplugged since).

use std::cell::RefCell;

use crate::store::{SavedWindow, Store};

thread_local! {
    /// The app's window, kept when it is created (`App::on_window`), to ask where it is.
    static WINDOW: RefCell<Option<rune::WindowHandle>> = const { RefCell::new(None) };
}

pub fn keep_window(handle: &rune::WindowHandle) {
    WINDOW.with(|w| *w.borrow_mut() = Some(handle.clone()));
}

/// Saves where the window is now; a maximized or minimized one saves the frame restoring it
/// returns to. Call it on the UI thread.
pub fn save(store: &Store) {
    let Some(p) = WINDOW.with(|w| w.borrow().as_ref().and_then(|h| h.placement())) else { return };
    if p.full_screen {
        return;
    }
    if p.minimized && p.restore.is_none() {
        return;
    }
    let (position, size) = p.restore.unwrap_or((p.position, p.content_size));
    store.save_window(&SavedWindow { position, size, maximized: p.maximized });
}

/// Whether a saved frame's top band (where it is grabbed) is still on a display.
#[cfg(windows)]
pub fn on_screen(w: &SavedWindow) -> bool {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromRect};
    let (x, y) = w.position;
    let band = RECT { left: x + 40, top: y, right: x + (w.size.0 as i32 - 40).max(41), bottom: y + 40 };
    // SAFETY: a plain query with a valid rectangle.
    !unsafe { MonitorFromRect(&band, MONITOR_DEFAULTTONULL) }.is_invalid()
}

#[cfg(not(windows))]
pub fn on_screen(_: &SavedWindow) -> bool {
    true
}

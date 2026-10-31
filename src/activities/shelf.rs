//! File shelf: drag files onto the pill to park them; click the pill to open
//! everything that is parked and clear the shelf.
//!
//! Everything is event driven: OLE `IDropTarget` callbacks only arrive on the
//! UI thread while a drag is actually over the pill, and every hover change
//! or drop is one `WM_APP_SHELF` post. The COM target never touches `App`
//! state — it fills a shared slot and posts, exactly like the media bridge
//! thread does, so the app can never be re-entered from a COM storm.

use std::cell::Cell;
use std::path::Path;
use std::sync::{Arc, Mutex};

use windows::core::{implement, Ref};
use windows::Win32::Foundation::{HWND, LPARAM, POINTL, WPARAM};
use windows::Win32::System::Com::{
    DVASPECT_CONTENT, FORMATETC, IDataObject, STGMEDIUM, TYMED_HGLOBAL,
};
use windows::Win32::System::Ole::{
    CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, IDropTarget, IDropTarget_Impl, RegisterDragDrop,
    ReleaseStgMedium,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::state::{Activity, ActivityId, Priority};

/// Posts from the drop target to the UI. `wparam` selects the event; the
/// dropped paths ride a shared slot because a message can't carry a Vec.
pub const WM_APP_SHELF: u32 = WM_APP + 4;
pub const SHELF_DROP: u32 = 0;
pub const SHELF_HOVER_ENTER: u32 = 1;
pub const SHELF_HOVER_LEAVE: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShelfFileTile {
    pub name: String,
    pub badge: String,
    pub size_str: String,
    pub path: String,
}

/// Display strings for the shelved batch, built once per shelf change —
/// never on the draw path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShelfView {
    pub title: String,
    pub artist: String,
    pub count: usize,
    pub files: Vec<ShelfFileTile>,
}

impl ShelfView {
    pub fn demo() -> Self {
        Self {
            title: "Invoice.pdf".into(),
            artist: "3 files shelved".into(),
            count: 3,
            files: vec![
                ShelfFileTile {
                    name: "Invoice.pdf".into(),
                    badge: "PDF".into(),
                    size_str: "240 KB".into(),
                    path: "Invoice.pdf".into(),
                },
                ShelfFileTile {
                    name: "Screenshot.png".into(),
                    badge: "PNG".into(),
                    size_str: "1.2 MB".into(),
                    path: "Screenshot.png".into(),
                },
                ShelfFileTile {
                    name: "TechHub.zip".into(),
                    badge: "ZIP".into(),
                    size_str: "8.4 MB".into(),
                    path: "TechHub.zip".into(),
                },
            ],
        }
    }
}

/// The shelved batch as an activity: active while at least one file waits.
/// A click on the pill opens all of them and empties the shelf.
#[derive(Debug, Default, Clone)]
pub struct ShelfActivity {
    pub count: usize,
}

impl Activity for ShelfActivity {
    fn id(&self) -> ActivityId {
        ActivityId::Shelf
    }
    fn priority(&self) -> Priority {
        Priority::Shelf
    }
    fn active(&self) -> bool {
        self.count > 0
    }
}

/// Builds the two display lines for the shelved batch: a single file is
/// shown by its name over its parent folder; a batch shows the first file
/// over the batch count.
pub fn view(paths: &[String]) -> Option<ShelfView> {
    if paths.is_empty() {
        return None;
    }
    let title = base_name(&paths[0]);
    let artist = if paths.len() == 1 {
        // Name the folder it was picked out of ("Downloads") — more useful
        // than a "shelved" filler when just one item is parked.
        Path::new(&paths[0])
            .parent()
            .and_then(|parent| parent.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "shelved".into())
    } else {
        format!("{} files shelved", paths.len())
    };
    let mut files = Vec::new();
    for p in paths.iter().take(3) {
        let name = base_name(p);
        let ext = Path::new(p)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("FILE")
            .to_uppercase();
        let size_str = std::fs::metadata(p)
            .map(|m| {
                let len = m.len();
                if len >= 1_048_576 {
                    format!("{:.1} MB", len as f64 / 1_048_576.0)
                } else if len >= 1024 {
                    format!("{} KB", len / 1024)
                } else {
                    format!("{} B", len)
                }
            })
            .unwrap_or_else(|_| "1.2 MB".into());
        files.push(ShelfFileTile {
            name,
            badge: ext,
            size_str,
            path: p.clone(),
        });
    }
    Some(ShelfView {
        title,
        artist,
        count: paths.len(),
        files,
    })
}

/// Last path segment; falls back to the full string for exotic paths that
/// have no file name (drive roots).
fn base_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// The OLE drop target installed on the pill window. Purely a feeder: it
/// copies the dropped paths into the shared slot and posts `WM_APP_SHELF`.
/// `App` never borrows its state inside these callbacks, so a big drag loop
/// can't re-enter the UI (see the wnd_proc reentrancy note in main.rs).
#[implement(IDropTarget)]
struct FileDropTarget {
    hwnd: isize,
    slot: Arc<Mutex<Vec<String>>>,
    hover: Cell<bool>,
}

impl FileDropTarget_Impl {
    /// Post only on transition — DragOver fires continuously while moving.
    fn set_hover(&self, on: bool) -> windows::core::Result<()> {
        if self.hover.replace(on) != on {
            unsafe {
                post_wm_app_shelf(
                    self.hwnd,
                    if on { SHELF_HOVER_ENTER } else { SHELF_HOVER_LEAVE },
                );
            }
        }
        Ok(())
    }

    fn drop_files(&self, data: Option<&IDataObject>) -> windows::core::Result<()> {
        let claim = |paths: Vec<String>| {
            if paths.is_empty() {
                self.set_hover(false) // nothing usable dropped: just unhighlight
            } else {
                *self.slot.lock().expect("shelf slot mutex poisoned") = paths;
                unsafe { post_wm_app_shelf(self.hwnd, SHELF_DROP) };
                self.set_hover(false)
            }
        };
        match data {
            Some(data) => match unsafe { extract_files(data) } {
                Some(paths) => claim(paths),
                None => self.set_hover(false),
            },
            None => self.set_hover(false),
        }
    }
}

impl IDropTarget_Impl for FileDropTarget_Impl {
    fn DragEnter(
        &self,
        _dataobject: Ref<'_, IDataObject>,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        // Copy semantics everywhere: the shelf parks a reference, the source
        // keeps its file.
        unsafe { *pdweffect = DROPEFFECT_COPY };
        self.set_hover(true)
    }

    fn DragOver(
        &self,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        unsafe { *pdweffect = DROPEFFECT_COPY };
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        self.set_hover(false)
    }

    fn Drop(
        &self,
        _dataobject: Ref<'_, IDataObject>,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        unsafe { *pdweffect = DROPEFFECT_COPY };
        self.drop_files(_dataobject.as_ref())
    }
}

/// Installs the drop target on the pill window. The returned COM object must
/// stay alive for the whole window lifetime (the caller holds it in `run`).
///
/// # Safety
/// OLE COM calls; run once on the UI thread before the message loop starts.
pub unsafe fn register_drop_target(
    hwnd: HWND,
    slot: Arc<Mutex<Vec<String>>>,
) -> windows::core::Result<IDropTarget> {
    let target: IDropTarget = FileDropTarget {
        hwnd: hwnd.0 as isize,
        slot,
        hover: Cell::new(false),
    }
    .into();
    RegisterDragDrop(hwnd, &target)?;
    Ok(target)
}

unsafe fn post_wm_app_shelf(hwnd: isize, wparam: u32) {
    let _ = PostMessageW(
        Some(HWND(hwnd as *mut _)),
        WM_APP_SHELF,
        WPARAM(wparam as usize),
        LPARAM(0),
    );
}

/// # Safety
/// Reads the CF_HDROP global of `data_object` per the OLE/Win32 contract;
/// the medium is released on every path before returning.
unsafe fn extract_files(data_object: &IDataObject) -> Option<Vec<String>> {
    let format = FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    // GetData hands us an owned medium; ReleaseStgMedium below frees it.
    let mut medium: STGMEDIUM = data_object.GetData(&format).ok()?;
    let result = (|| {
        if medium.tymed != TYMED_HGLOBAL.0 as u32 {
            return None;
        }
        // Union read: the variant is selected by the tymed check above.
        let hdrop = HDROP(unsafe { medium.u.hGlobal.0 });
        let count = DragQueryFileW(hdrop, u32::MAX, None);
        let mut paths = Vec::with_capacity(count as usize);
        for index in 0..count {
            // None buffer → text length (without the terminating nul).
            let len = DragQueryFileW(hdrop, index, None) as usize;
            let mut buf = vec![0u16; len + 1];
            let written = DragQueryFileW(hdrop, index, Some(&mut buf)) as usize;
            buf.truncate(written);
            if !buf.is_empty() {
                paths.push(String::from_utf16_lossy(&buf));
            }
        }
        Some(paths)
    })();
    ReleaseStgMedium(&mut medium);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn empty_shelf_is_inactive_and_unviewable() {
        assert!(view(&[]).is_none());
        assert!(!ShelfActivity::default().active());
    }

    #[test]
    fn single_file_uses_name_and_parent_folder() {
        let view = view(&paths(&["D:\\Downloads\\report.pdf"])).unwrap();
        assert_eq!(view.title, "report.pdf");
        assert_eq!(view.artist, "Downloads");
        assert_eq!(view.count, 1);
        assert!(ShelfActivity { count: 1 }.active());
    }

    #[test]
    fn batch_shows_first_file_and_count() {
        let view = view(&paths(&["C:\\a\\one.txt", "C:\\a\\two.txt"])).unwrap();
        assert_eq!(view.title, "one.txt");
        assert_eq!(view.artist, "2 files shelved");
        assert_eq!(view.count, 2);
    }

    #[test]
    fn file_without_a_parent_falls_back() {
        let view = view(&paths(&["tmp.bin"])).unwrap();
        assert_eq!(view.title, "tmp.bin");
        assert_eq!(view.artist, "shelved");
    }
}

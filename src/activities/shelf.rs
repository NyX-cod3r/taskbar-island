//! shelf.rs — parked file references, persistence, the OLE drop target and the
//! drag-out source.
//!
//! Data layer only. Nothing here draws; `view.rs` turns the path list into
//! tiles, and `render.rs` turns those tiles into pixels.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use windows::core::{implement, Error, Ref, Result, HRESULT};
use windows::Win32::Foundation::{
    DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, E_NOTIMPL, HGLOBAL, HWND,
    LPARAM, POINTL, WPARAM,
};
use windows::Win32::System::Com::DATADIR_GET;
use windows::Win32::System::Com::{
    IAdviseSink, IDataObject, IDataObject_Impl, IEnumFORMATETC, IEnumSTATDATA, DVASPECT_CONTENT,
    FORMATETC, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{
    DoDragDrop, IDropSource, IDropSource_Impl, IDropTarget, IDropTarget_Impl, RegisterDragDrop,
    ReleaseStgMedium, RevokeDragDrop,
};
use windows::Win32::System::SystemServices::{MK_CONTROL, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows::Win32::UI::Shell::{DragQueryFileW, SHCreateStdEnumFmtEtc, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::json::{self, Json};
use crate::settings;

pub const WM_APP_SHELF: u32 = WM_APP + 4;
pub const SHELF_DROP: u32 = 0;
pub const SHELF_HOVER_ENTER: u32 = 1;
pub const SHELF_HOVER_LEAVE: u32 = 2;
/// Ctrl was held when the files were dropped: open the converter instead of
/// parking them on the shelf.
pub const SHELF_DROP_CTRL: u32 = 3;
/// Ctrl is held while the pointer is over the island.
pub const SHELF_HOVER_CONVERT: u32 = 4;
/// Pointer position during a drag, packed into LPARAM. The converter card is
/// spring-loaded under the pointer, so the drop can land on an option tile.
pub const SHELF_DRAG_AT: u32 = 5;

/// Pack a screen point into an LPARAM. Both halves are 32 bit, which covers
/// the whole virtual desktop.
pub fn pack_point(x: i32, y: i32) -> LPARAM {
    LPARAM(((x as u32 as u64) | ((y as u32 as u64) << 32)) as isize)
}

/// The shelf accepts far more than the grid can show; older entries fall off
/// silently and the toolbar's count tooltip warns when that happened.
pub const MAX_ENTRIES: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragMode {
    Idle,
    Shelf,
    Convert,
}

impl DragMode {
    fn for_keys(keys: MODIFIERKEYS_FLAGS) -> Self {
        if keys.contains(MK_CONTROL) {
            Self::Convert
        } else {
            Self::Shelf
        }
    }
}

/// Persistent, newest-first list of parked paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShelfStore {
    paths: Vec<String>,
}

impl ShelfStore {
    pub fn load() -> Self {
        let path = settings::shelf_path();
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        let Ok(value) = json::parse(&text) else {
            return Self::default();
        };
        let mut paths: Vec<String> = value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        paths.retain(|p| Path::new(p).exists());
        paths.truncate(MAX_ENTRIES);
        Self { paths }
    }

    pub fn paths(&self) -> &[String] {
        &self.paths
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    /// Newest first, deduplicated, case-insensitive (Windows paths). Files and
    /// folders are both parked; folders keep their whole subtree. Returns
    /// whether the soft cap pushed entries out.
    pub fn add<I: IntoIterator<Item = String>>(&mut self, incoming: I) -> bool {
        for path in incoming {
            if !Path::new(&path).exists() {
                continue;
            }
            if self.paths.iter().any(|p| p.eq_ignore_ascii_case(&path)) {
                self.paths.retain(|p| !p.eq_ignore_ascii_case(&path));
            }
            self.paths.insert(0, path);
        }
        let overflow = self.paths.len() > MAX_ENTRIES;
        self.paths.truncate(MAX_ENTRIES);
        self.save();
        overflow
    }

    /// Removes parked references only. The files on disk are never touched.
    pub fn remove(&mut self, index: usize) -> Option<String> {
        if index >= self.paths.len() {
            return None;
        }
        let removed = self.paths.remove(index);
        self.save();
        Some(removed)
    }

    /// Removes several parked references at once; indices may arrive in any
    /// order and duplicates are ignored.
    pub fn remove_indices(&mut self, indices: &[usize]) {
        if indices.is_empty() {
            return;
        }
        let mut unique: Vec<usize> = indices.to_vec();
        unique.sort_unstable();
        unique.dedup();
        for index in unique.into_iter().rev() {
            if index < self.paths.len() {
                self.paths.remove(index);
            }
        }
        self.save();
    }

    /// Removes every parked reference. The files on disk are never touched.
    pub fn clear(&mut self) {
        self.paths.clear();
        self.save();
    }

    /// The toolbar's Clear action: selected references only when there is a
    /// selection, otherwise the whole shelf. Never touches files on disk.
    pub fn remove_or_clear(&mut self, selected: &[usize]) {
        if selected.is_empty() {
            self.clear();
        } else {
            self.remove_indices(selected);
        }
    }

    /// Drops the entries that were dragged out with a move effect.
    pub fn remove_paths(&mut self, gone: &[String]) {
        let before = self.paths.len();
        self.paths
            .retain(|p| !gone.iter().any(|g| g.eq_ignore_ascii_case(p)));
        if self.paths.len() != before {
            self.save();
        }
    }

    /// Drops entries whose file or folder disappeared.
    pub fn prune(&mut self) -> bool {
        let before = self.paths.len();
        self.paths.retain(|p| Path::new(p).exists());
        if self.paths.len() != before {
            self.save();
            true
        } else {
            false
        }
    }

    pub fn save(&self) {
        // Unit tests must never rewrite the signed-in user's real shelf.
        #[cfg(test)]
        let _ = self;
        #[cfg(not(test))]
        {
            let path = settings::shelf_path();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let doc = Json::Arr(self.paths.iter().map(|p| Json::Str(p.clone())).collect());
            let _ = std::fs::write(path, json::to_string_pretty(&doc));
        }
    }
}

/// The OLE drop target installed on the island window. It is a feeder: the
/// COM callback fills a shared slot and posts `WM_APP_SHELF`; `App` never
/// runs inside a COM callback.
#[implement(IDropTarget)]
struct FileDropTarget {
    hwnd: isize,
    slot: Arc<Mutex<Vec<String>>>,
    mode: Cell<DragMode>,
    /// Whether the open convert card has at least one option for the batch.
    /// A mixed or unsupported set turns the cursor into "no drop".
    convert_accepts: Arc<AtomicBool>,
}

impl FileDropTarget_Impl {
    fn set_mode(&self, mode: DragMode) {
        if self.mode.replace(mode) == mode {
            return;
        }
        let event = match mode {
            DragMode::Idle => SHELF_HOVER_LEAVE,
            DragMode::Shelf => SHELF_HOVER_ENTER,
            DragMode::Convert => SHELF_HOVER_CONVERT,
        };
        let _ = post_shelf(self.hwnd, event);
    }

    fn accept(data: Option<&IDataObject>) -> bool {
        let Some(data) = data else {
            return false;
        };
        // SAFETY: QueryGetData only inspects the caller-owned data object.
        let format = hdrop_format();
        unsafe { data.QueryGetData(&format) }.is_ok()
    }

    /// The convert card is spring-loaded but offers nothing for this batch.
    fn convert_refuses(&self) -> bool {
        !self.convert_accepts.load(Ordering::Relaxed)
    }
}

impl IDropTarget_Impl for FileDropTarget_Impl {
    fn DragEnter(
        &self,
        dataobject: Ref<'_, IDataObject>,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut windows::Win32::System::Ole::DROPEFFECT,
    ) -> Result<()> {
        let accepted = Self::accept(dataobject.as_ref());
        // SAFETY: OLE owns the out pointer for the duration of the call.
        unsafe {
            if accepted {
                *pdweffect = windows::Win32::System::Ole::DROPEFFECT_COPY;
            } else {
                *pdweffect = windows::Win32::System::Ole::DROPEFFECT_NONE;
                self.set_mode(DragMode::Idle);
                return Ok(());
            }
        }
        // The paths are read once on enter so the UI can spring-load the
        // converter card while the drag is still in flight. Ctrl may be
        // pressed later, and DragOver never sees the data object.
        if let Some(data) = dataobject.as_ref() {
            // SAFETY: the data object belongs to the caller for this call.
            if let Some(paths) = unsafe { extract_files(data) } {
                if !paths.is_empty() {
                    *self.slot.lock().expect("shelf slot mutex poisoned") = paths;
                }
            }
        }
        self.set_mode(DragMode::for_keys(grfkeystate));
        let _ = post_shelf_at(self.hwnd, SHELF_DRAG_AT, pt.x, pt.y);
        Ok(())
    }

    fn DragOver(
        &self,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut windows::Win32::System::Ole::DROPEFFECT,
    ) -> Result<()> {
        // Ctrl can be pressed or released without leaving the island, so the
        // mode is refreshed on every DragOver rather than only on enter.
        if self.mode.get() != DragMode::Idle {
            self.set_mode(DragMode::for_keys(grfkeystate));
        }
        if self.mode.get() == DragMode::Convert {
            let _ = post_shelf_at(self.hwnd, SHELF_DRAG_AT, pt.x, pt.y);
        }
        // SAFETY: OLE owns the out pointer for the duration of the call.
        unsafe {
            let copy = match self.mode.get() {
                DragMode::Idle => false,
                DragMode::Shelf => true,
                DragMode::Convert => !self.convert_refuses(),
            };
            *pdweffect = if copy {
                windows::Win32::System::Ole::DROPEFFECT_COPY
            } else {
                windows::Win32::System::Ole::DROPEFFECT_NONE
            };
        }
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        self.set_mode(DragMode::Idle);
        Ok(())
    }

    fn Drop(
        &self,
        dataobject: Ref<'_, IDataObject>,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut windows::Win32::System::Ole::DROPEFFECT,
    ) -> Result<()> {
        // SAFETY: OLE owns the out pointer; extract_files releases its medium.
        let paths = dataobject
            .as_ref()
            .and_then(|data| unsafe { extract_files(data) })
            .unwrap_or_default();
        let ctrl = grfkeystate.contains(MK_CONTROL);
        // A convert drop with nothing on offer is refused outright: the file
        // is not parked on the shelf either.
        let refused = ctrl && self.convert_refuses();
        // SAFETY: see above.
        unsafe {
            *pdweffect = if paths.is_empty() || refused {
                windows::Win32::System::Ole::DROPEFFECT_NONE
            } else {
                windows::Win32::System::Ole::DROPEFFECT_COPY
            };
        }
        if !paths.is_empty() && !refused {
            *self.slot.lock().expect("shelf slot mutex poisoned") = paths;
            let event = if ctrl { SHELF_DROP_CTRL } else { SHELF_DROP };
            // The drop point decides whether the file landed on an option.
            let _ = post_shelf_at(self.hwnd, event, pt.x, pt.y);
        }
        self.set_mode(DragMode::Idle);
        Ok(())
    }
}

/// Installs the drop target. Keep the returned object alive for the window
/// lifetime; revoke with `revoke_drop_target` on shutdown.
///
/// # Safety
/// OLE calls; run on the UI thread after `OleInitialize`.
pub unsafe fn register_drop_target(
    hwnd: HWND,
    slot: Arc<Mutex<Vec<String>>>,
    convert_accepts: Arc<AtomicBool>,
) -> Result<IDropTarget> {
    let target: IDropTarget = FileDropTarget {
        hwnd: hwnd.0 as isize,
        slot,
        mode: Cell::new(DragMode::Idle),
        convert_accepts,
    }
    .into();
    // SAFETY: caller guarantees OLE is initialized on this thread.
    unsafe { RegisterDragDrop(hwnd, &target)? };
    Ok(target)
}

/// # Safety
/// The window must own a registered drop target.
pub unsafe fn revoke_drop_target(hwnd: HWND) {
    // SAFETY: caller owns the window registration.
    let _ = unsafe { RevokeDragDrop(hwnd) };
}

fn hdrop_format() -> FORMATETC {
    FORMATETC {
        cfFormat: windows::Win32::System::Ole::CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

fn post_shelf(hwnd: isize, event: u32) -> Result<()> {
    post_shelf_at(hwnd, event, 0, 0)
}

fn post_shelf_at(hwnd: isize, event: u32, x: i32, y: i32) -> Result<()> {
    // SAFETY: posting to a window owned by this process is always safe.
    unsafe {
        PostMessageW(
            Some(HWND(hwnd as *mut _)),
            WM_APP_SHELF,
            WPARAM(event as usize),
            pack_point(x, y),
        )
    }
}

/// # Safety
/// The caller passes an OLE data object; the returned medium is released on
/// every path before returning.
unsafe fn extract_files(data_object: &IDataObject) -> Option<Vec<String>> {
    let format = hdrop_format();
    // SAFETY: GetData returns an owned medium, released immediately below.
    let mut medium: STGMEDIUM = unsafe { data_object.GetData(&format) }.ok()?;
    let result = (|| {
        if medium.tymed != TYMED_HGLOBAL.0 as u32 {
            return None;
        }
        // SAFETY: union branch selected by the tymed check.
        let hdrop = HDROP(unsafe { medium.u.hGlobal.0 });
        // SAFETY: hdrop is a valid CF_HDROP handle for this medium.
        let count = unsafe { DragQueryFileW(hdrop, u32::MAX, None) };
        let mut paths = Vec::with_capacity(count as usize);
        for index in 0..count {
            // SAFETY: same handle; None asks for the length only.
            let len = unsafe { DragQueryFileW(hdrop, index, None) } as usize;
            let mut buf = vec![0u16; len + 1];
            // SAFETY: buf is sized for the path plus its terminator.
            let written = unsafe { DragQueryFileW(hdrop, index, Some(&mut buf)) } as usize;
            buf.truncate(written);
            if !buf.is_empty() {
                paths.push(String::from_utf16_lossy(&buf));
            }
        }
        Some(paths)
    })();
    // SAFETY: medium was produced by GetData and is released exactly once.
    unsafe { ReleaseStgMedium(&mut medium) };
    result
}

// ------------------------------------------------------------- drag out

#[implement(IDataObject)]
struct FileDataObject {
    paths: Vec<String>,
}

impl FileDataObject_Impl {
    unsafe fn build_hglobal(&self) -> Result<HGLOBAL> {
        let mut wide: Vec<u16> = Vec::new();
        for path in &self.paths {
            wide.extend(path.encode_utf16());
            wide.push(0);
        }
        wide.push(0);
        let header = std::mem::size_of::<windows::Win32::UI::Shell::DROPFILES>();
        let bytes = header + wide.len() * 2;
        // SAFETY: allocating a moveable global block of the computed size.
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes)? };
        // SAFETY: handle is valid and the block is at least `bytes` long.
        let base = unsafe { GlobalLock(handle) };
        if base.is_null() {
            return Err(Error::from_hresult(HRESULT(-2147467259))); // E_FAIL
        }
        // SAFETY: base points to `bytes` writable bytes. DROPFILES is packed,
        // so write it bytewise through a local copy.
        unsafe {
            let header_value = windows::Win32::UI::Shell::DROPFILES {
                pFiles: header as u32,
                pt: windows::Win32::Foundation::POINT::default(),
                fNC: false.into(),
                fWide: true.into(),
            };
            std::ptr::copy_nonoverlapping(
                &header_value as *const _ as *const u8,
                base as *mut u8,
                header,
            );
            std::ptr::copy_nonoverlapping(
                wide.as_ptr() as *const u8,
                (base as *mut u8).add(header),
                wide.len() * 2,
            );
        }
        // SAFETY: unlocks the block allocated above.
        let _ = unsafe { GlobalUnlock(handle) };
        Ok(handle)
    }
}

impl IDataObject_Impl for FileDataObject_Impl {
    fn GetData(&self, format: *const FORMATETC) -> Result<STGMEDIUM> {
        // SAFETY: OLE passes a valid FORMATETC pointer.
        let format = unsafe { &*format };
        if format.cfFormat != windows::Win32::System::Ole::CF_HDROP.0
            || (format.tymed as u32 & TYMED_HGLOBAL.0 as u32) == 0
        {
            return Err(Error::from_hresult(
                windows::Win32::Foundation::DV_E_FORMATETC,
            ));
        }
        // SAFETY: builds a fresh HGLOBAL owned by the returned medium.
        let handle = unsafe { self.build_hglobal()? };
        Ok(STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: handle },
            pUnkForRelease: std::mem::ManuallyDrop::new(None),
        })
    }

    fn GetDataHere(&self, _format: *const FORMATETC, _medium: *mut STGMEDIUM) -> Result<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn QueryGetData(&self, format: *const FORMATETC) -> HRESULT {
        // SAFETY: OLE passes a valid FORMATETC pointer.
        let format = unsafe { &*format };
        if format.cfFormat == windows::Win32::System::Ole::CF_HDROP.0
            && (format.tymed as u32 & TYMED_HGLOBAL.0 as u32) != 0
        {
            HRESULT(0)
        } else {
            windows::Win32::Foundation::DV_E_FORMATETC
        }
    }

    fn GetCanonicalFormatEtc(
        &self,
        _in_format: *const FORMATETC,
        _out_format: *mut FORMATETC,
    ) -> HRESULT {
        E_NOTIMPL
    }

    fn SetData(
        &self,
        _format: *const FORMATETC,
        _medium: *const STGMEDIUM,
        _release: windows::core::BOOL,
    ) -> Result<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn EnumFormatEtc(&self, direction: u32) -> Result<IEnumFORMATETC> {
        if direction != DATADIR_GET.0 as u32 {
            return Err(Error::from_hresult(E_NOTIMPL));
        }
        let formats = [hdrop_format()];
        // SAFETY: shell copies the FORMATETC array into a new enumerator.
        unsafe { SHCreateStdEnumFmtEtc(&formats) }
    }

    fn DAdvise(
        &self,
        _format: *const FORMATETC,
        _flags: u32,
        _sink: Ref<IAdviseSink>,
    ) -> Result<u32> {
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn DUnadvise(&self, _connection: u32) -> Result<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
        Err(Error::from_hresult(E_NOTIMPL))
    }
}

#[implement(IDropSource)]
struct FileDropSource;

impl IDropSource_Impl for FileDropSource_Impl {
    fn QueryContinueDrag(&self, escape: windows::core::BOOL, _keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() {
            DRAGDROP_S_CANCEL
        } else if unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } as u16 & 0x8000 == 0 {
            DRAGDROP_S_DROP
        } else {
            HRESULT(0)
        }
    }

    fn GiveFeedback(&self, _effect: windows::Win32::System::Ole::DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// What the drop target did with a parked entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragOutcome {
    /// A copy was made; the parked entry is still valid.
    Copy,
    /// The target took the item away; drop it from the shelf.
    Move,
    /// Escape or the target refused the data.
    Cancel,
}

/// Starts an OLE drag for one or more parked paths. Blocking (modal drag loop),
/// so call it after releasing the borrow on the app state. Offering both
/// effects lets Explorer copy (Ctrl, cross-volume) or move (Shift, same volume).
///
/// # Safety
/// OLE must be initialized on this thread.
pub unsafe fn start_drag(paths: &[String]) -> DragOutcome {
    if paths.is_empty() {
        return DragOutcome::Cancel;
    }
    let data: IDataObject = FileDataObject {
        paths: paths.to_vec(),
    }
    .into();
    let source: IDropSource = FileDropSource.into();
    let mut effect = windows::Win32::System::Ole::DROPEFFECT_NONE;
    // SAFETY: data object and drop source outlive the modal drag loop.
    let result = unsafe {
        DoDragDrop(
            &data,
            &source,
            windows::Win32::System::Ole::DROPEFFECT_COPY
                | windows::Win32::System::Ole::DROPEFFECT_MOVE,
            &mut effect,
        )
    };
    if result.is_err() {
        return DragOutcome::Cancel;
    }
    if effect.contains(windows::Win32::System::Ole::DROPEFFECT_MOVE) {
        DragOutcome::Move
    } else if effect != windows::Win32::System::Ole::DROPEFFECT_NONE {
        DragOutcome::Copy
    } else {
        DragOutcome::Cancel
    }
}

/// Best-effort launch of a shelved file.
pub fn open(path: &str) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let operation = HSTRING::from("open");
    let file = HSTRING::from(path);
    // SAFETY: strings are NUL-terminated by HSTRING; the return value only
    // reports whether the shell accepted the launch.
    let result = unsafe {
        ShellExecuteW(
            None,
            &operation,
            &file,
            windows::core::PCWSTR::null(),
            windows::core::PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    result.0 as usize > 32
}

/// Folder that contains the file, for the single-file subtitle.
pub fn parent_name(path: &str) -> Option<String> {
    Path::new(path)
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
}

/// A stable display key for the tray/menu and tests.
pub fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

#[allow(dead_code)]
fn _path_buf_marker(_: PathBuf) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_and_deduped() {
        let dir = std::env::temp_dir().join("taskbar-island-shelf-store-test");
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.txt");
        let b = dir.join("b.txt");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let mut store = ShelfStore::default();
        store.paths = vec![a.display().to_string(), b.display().to_string()];
        store.add(vec![a.display().to_string()]);
        assert_eq!(store.len(), 2);
        assert!(store.paths()[0].ends_with("a.txt"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_files_do_not_enter_the_shelf() {
        let mut store = ShelfStore::default();
        store.add(vec!["Z:\\definitely\\missing\\file.txt".into()]);
        assert!(store.is_empty());
    }

    #[test]
    fn folders_are_parked_and_moved_away() {
        let dir = std::env::temp_dir().join("taskbar-island-shelf-folder-test");
        let folder = dir.join("invoices");
        std::fs::create_dir_all(&folder).unwrap();
        let mut store = ShelfStore::default();
        store.add(vec![folder.display().to_string()]);
        assert_eq!(store.len(), 1, "a folder should be accepted");
        store.remove_paths(&[folder.display().to_string()]);
        assert!(store.is_empty(), "a moved folder leaves the shelf");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn names_and_parents_are_display_ready() {
        assert_eq!(file_name(r"C:\Users\me\Invoice.pdf"), "Invoice.pdf");
        assert_eq!(
            parent_name(r"C:\Users\me\Invoice.pdf").as_deref(),
            Some("me")
        );
    }

    #[test]
    fn removing_references_never_deletes_files() {
        let dir = std::env::temp_dir().join("taskbar-island-shelf-clear-test");
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("keep-a.txt");
        let b = dir.join("keep-b.txt");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let mut store = ShelfStore::default();
        store.add(vec![b.display().to_string(), a.display().to_string()]);
        assert_eq!(store.len(), 2);
        store.remove_indices(&[0]);
        assert_eq!(store.len(), 1);
        store.clear();
        assert!(store.is_empty());
        assert!(a.exists(), "Clear must not delete the file");
        assert!(b.exists(), "Clear must not delete the file");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn remove_indices_accepts_unsorted_and_duplicate_indices() {
        let dir = std::env::temp_dir().join("taskbar-island-shelf-indices-test");
        std::fs::create_dir_all(&dir).unwrap();
        let paths: Vec<String> = (0..4)
            .map(|n| {
                let p = dir.join(format!("{n}.txt"));
                std::fs::write(&p, b"x").unwrap();
                p.display().to_string()
            })
            .collect();
        let mut store = ShelfStore::default();
        store.add(paths);
        store.remove_indices(&[2, 0, 2]);
        assert_eq!(store.len(), 2);
        // Newest first: the original order 0..3 arrives as [3, 2, 1, 0], and
        // indices 2 and 0 (1.txt and 3.txt) are the ones removed.
        assert!(store.paths()[0].ends_with("2.txt"));
        assert!(store.paths()[1].ends_with("0.txt"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn clear_removes_selection_when_present_otherwise_everything() {
        let dir = std::env::temp_dir().join("taskbar-island-shelf-remove-or-clear-test");
        std::fs::create_dir_all(&dir).unwrap();
        let paths: Vec<String> = (0..3)
            .map(|n| {
                let p = dir.join(format!("{n}.txt"));
                std::fs::write(&p, b"x").unwrap();
                p.display().to_string()
            })
            .collect();
        let mut store = ShelfStore::default();
        store.add(paths.clone());
        store.remove_or_clear(&[1]);
        assert_eq!(store.len(), 2, "a selection removes only those tiles");
        store.remove_or_clear(&[]);
        assert!(store.is_empty(), "no selection clears the shelf");
        for path in &paths {
            assert!(Path::new(path).exists(), "the file on disk survives");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn add_reports_soft_cap_overflow() {
        let dir = std::env::temp_dir().join("taskbar-island-shelf-cap-test");
        std::fs::create_dir_all(&dir).unwrap();
        let before = (0..MAX_ENTRIES)
            .map(|n| {
                let p = dir.join(format!("{n}.txt"));
                std::fs::write(&p, b"x").unwrap();
                p.display().to_string()
            })
            .collect::<Vec<_>>();
        let mut store = ShelfStore::default();
        store.add(before);
        assert_eq!(store.len(), MAX_ENTRIES);
        assert!(!store.add(Vec::new()), "no overflow without new items");
        let extra = dir.join("extra.txt");
        std::fs::write(&extra, b"x").unwrap();
        assert!(store.add(vec![extra.display().to_string()]));
        assert_eq!(store.len(), MAX_ENTRIES, "soft cap holds");
        assert!(store.paths()[0].ends_with("extra.txt"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The Clear path and the per-tile remove path remove references only.
    /// The source of this module must not contain a file-deletion call.
    #[test]
    fn shelf_source_has_no_disk_deletion() {
        let source = include_str!("shelf.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("test module follows production code");
        let forbidden = [["remove_", "file"].concat(), ["Delete", "File"].concat()];
        for needle in forbidden {
            assert!(
                !production.contains(&needle),
                "shelf.rs must never delete files from disk ({needle})"
            );
        }
    }
}

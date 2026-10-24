//! Dragging converted images out of the list, into Discord, a browser or a
//! folder (Windows).
//!
//! The drag runs in the shell's own loop (`SHDoDragDrop`), which only
//! returns once the files are dropped. It can't start inside a frame,
//! since the window's events would then arrive while the app is busy, so
//! [`start`] posts the files to a hidden window whose message the event
//! loop handles between frames, with the mouse button still held.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Whether this platform can drag files out of the window.
pub(crate) const SUPPORTED: bool = cfg!(windows);

/// The files being dragged, so that dropping them back onto the window
/// doesn't add them to the list.
static DRAGGING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Starts dragging `files` once the current frame is done. Call it while
/// the mouse button is held, as in response to `drag_started`.
#[cfg_attr(not(windows), allow(unused_variables))]
pub(crate) fn start(files: Vec<PathBuf>) {
    #[cfg(windows)]
    {
        *dragging() = files.clone();
        if !imp::post(files) {
            dragging().clear();
        }
    }
}

/// Whether files are being dragged out of the window.
pub(crate) fn active() -> bool {
    !dragging().is_empty()
}

/// Whether `path` is one of the files being dragged out.
pub(crate) fn is_dragged(path: &Path) -> bool {
    dragging().iter().any(|p| p == path)
}

fn dragging() -> std::sync::MutexGuard<'static, Vec<PathBuf>> {
    DRAGGING.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::sync::OnceLock;

    use windows_sys::Win32::Foundation::{
        DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, E_NOINTERFACE, HWND,
        LPARAM, LRESULT, POINT, S_OK, WPARAM,
    };
    use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Ole::DROPEFFECT_COPY;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetCapture;
    use windows_sys::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows_sys::Win32::UI::Shell::{
        BHID_DataObject, CIDLData_CreateFromIDArray, ILCreateFromPathW, ILFree,
        SHCreateShellItemArrayFromIDLists, SHDoDragDrop,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, GetCursorPos, HWND_MESSAGE, PostMessageW, RegisterClassW,
        WM_APP, WM_LBUTTONUP, WM_MOUSEMOVE, WNDCLASSW,
    };
    use windows_sys::core::{BOOL, GUID, HRESULT};

    /// The message carrying the files, as a `Box<Vec<PathBuf>>` in `lparam`.
    const DRAG: u32 = WM_APP + 2;
    const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
    const IID_IDATAOBJECT: GUID = GUID::from_u128(0x0000010e_0000_0000_c000_000000000046);
    const IID_IDROPSOURCE: GUID = GUID::from_u128(0x00000121_0000_0000_c000_000000000046);
    const MK_LBUTTON: u32 = 1;

    /// Hands `files` to the hidden window; false if there is none.
    pub fn post(files: Vec<PathBuf>) -> bool {
        let Some(hwnd) = window() else {
            return false;
        };
        let files = Box::into_raw(Box::new(files));
        // SAFETY: the window procedure takes the box back; if the message
        // can't be posted, it is taken back here.
        unsafe {
            if PostMessageW(hwnd as HWND, DRAG, 0, files as LPARAM) == 0 {
                drop(Box::from_raw(files));
                return false;
            }
        }
        true
    }

    /// The hidden window, made on first use. A message-only window, as it
    /// needs no broadcasts.
    fn window() -> Option<isize> {
        static WINDOW: OnceLock<Option<isize>> = OnceLock::new();
        *WINDOW.get_or_init(|| {
            let class: Vec<u16> = "hdrshot drag".encode_utf16().chain([0]).collect();
            // SAFETY: plain Win32 calls; the class name outlives them.
            unsafe {
                let instance = GetModuleHandleW(std::ptr::null());
                RegisterClassW(&WNDCLASSW {
                    lpfnWndProc: Some(window_proc),
                    hInstance: instance,
                    lpszClassName: class.as_ptr(),
                    ..std::mem::zeroed()
                });
                let hwnd = CreateWindowExW(
                    0,
                    class.as_ptr(),
                    std::ptr::null(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    HWND_MESSAGE,
                    std::ptr::null_mut(),
                    instance,
                    std::ptr::null(),
                );
                (!hwnd.is_null()).then_some(hwnd as isize)
            }
        })
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message != DRAG {
            // SAFETY: the default handling for any other message.
            return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
        }
        // SAFETY: `post` sent a boxed list and nothing else sends DRAG.
        let files = unsafe { Box::from_raw(lparam as *mut Vec<PathBuf>) };
        // The app's window holds the mouse while the button is down.
        // SAFETY: a plain Win32 call.
        let window = unsafe { GetCapture() };
        drag(&files);
        super::dragging().clear();
        if !window.is_null() {
            release_button(window);
        }
        0
    }

    /// Drags `files` until they are dropped, offering to copy them.
    fn drag(files: &[PathBuf]) {
        let items: Vec<*mut ITEMIDLIST> = files
            .iter()
            .filter_map(|file| {
                let path: Vec<u16> = file.as_os_str().encode_wide().chain([0]).collect();
                // SAFETY: `path` is a zero-terminated wide string.
                let item = unsafe { ILCreateFromPathW(path.as_ptr()) };
                (!item.is_null()).then_some(item)
            })
            .collect();
        // The data object keeps copies of the items.
        let data = data_object(&items);
        for item in items {
            // SAFETY: made by ILCreateFromPathW above.
            unsafe { ILFree(item) };
        }
        let Some(data) = data else { return };
        let source = &DROP_SOURCE as *const DropSource as *mut c_void;
        let mut effect = 0;
        // SAFETY: `data` is a live data object, released once the files
        // are dropped, and the drop source is never freed.
        unsafe {
            SHDoDragDrop(
                std::ptr::null_mut(),
                data,
                source,
                DROPEFFECT_COPY,
                &mut effect,
            );
            release(data);
        }
    }

    /// The shell's data object for the absolute `items`, which may be in
    /// different folders: it offers them as files, with their thumbnails as
    /// the drag image. Made through a shell item array, as
    /// `SHCreateDataObject` only takes items of one folder.
    fn data_object(items: &[*mut ITEMIDLIST]) -> Option<*mut c_void> {
        if items.is_empty() {
            return None;
        }
        let count = items.len() as u32;
        let items = items.as_ptr() as *const *const ITEMIDLIST;
        let mut data: *mut c_void = std::ptr::null_mut();
        // SAFETY: plain shell calls; the array is released once it made
        // the data object.
        unsafe {
            let mut array: *mut c_void = std::ptr::null_mut();
            if SHCreateShellItemArrayFromIDLists(count, items, &mut array) >= 0 && !array.is_null()
            {
                let vtbl = &**(array as *const *const ShellItemArrayVtbl);
                let made = (vtbl.bind_to_handler)(
                    array,
                    std::ptr::null_mut(),
                    &BHID_DataObject,
                    &IID_IDATAOBJECT,
                    &mut data,
                );
                release(array);
                if made >= 0 && !data.is_null() {
                    return Some(data);
                }
            }
            // Wine can't make that one; its older call takes the items as
            // relative to the desktop.
            data = std::ptr::null_mut();
            let desktop = ITEMIDLIST::default();
            if CIDLData_CreateFromIDArray(&desktop, count, items, &mut data) < 0 {
                return None;
            }
        }
        (!data.is_null()).then_some(data)
    }

    /// The start of `IShellItemArray`'s vtable.
    #[repr(C)]
    struct ShellItemArrayVtbl {
        unknown: [usize; 3],
        bind_to_handler: unsafe extern "system" fn(
            *mut c_void,
            *mut c_void,
            *const GUID,
            *const GUID,
            *mut *mut c_void,
        ) -> HRESULT,
    }

    /// Ends the press that started the drag in the app's `window`: the
    /// drag took the button's release, which the window would otherwise
    /// never get, leaving the list thinking the button is still down. The
    /// pointer is moved to where it is first, as the release only counts
    /// with the pointer known, and it may have left the window.
    fn release_button(window: HWND) {
        let mut point = POINT { x: 0, y: 0 };
        // SAFETY: plain Win32 calls on a window of this thread.
        unsafe {
            GetCursorPos(&mut point);
            ScreenToClient(window, &mut point);
            let position = (point.x as u16 as u32 | (point.y as u16 as u32) << 16) as LPARAM;
            PostMessageW(window, WM_MOUSEMOVE, 0, position);
            PostMessageW(window, WM_LBUTTONUP, 0, position);
        }
    }

    /// The drop source, a COM object: the drag goes on while the left
    /// button is held, and Escape cancels it. There is one, never freed,
    /// so it needs no reference count.
    #[repr(C)]
    struct DropSource {
        vtbl: &'static DropSourceVtbl,
    }

    #[repr(C)]
    struct DropSourceVtbl {
        query_interface:
            unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
        add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        query_continue_drag: unsafe extern "system" fn(*mut c_void, BOOL, u32) -> HRESULT,
        give_feedback: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    }

    static DROP_SOURCE: DropSource = DropSource {
        vtbl: &DropSourceVtbl {
            query_interface,
            add_ref,
            release: add_ref,
            query_continue_drag,
            give_feedback,
        },
    };

    unsafe extern "system" fn query_interface(
        this: *mut c_void,
        iid: *const GUID,
        object: *mut *mut c_void,
    ) -> HRESULT {
        // SAFETY: COM passes a valid interface id and out pointer.
        unsafe {
            let key = |id: &GUID| (id.data1, id.data2, id.data3, id.data4);
            let known = [IID_IUNKNOWN, IID_IDROPSOURCE]
                .iter()
                .any(|id| key(id) == key(&*iid));
            *object = if known { this } else { std::ptr::null_mut() };
            if known { S_OK } else { E_NOINTERFACE }
        }
    }

    unsafe extern "system" fn add_ref(_: *mut c_void) -> u32 {
        1
    }

    unsafe extern "system" fn query_continue_drag(
        _: *mut c_void,
        escape: BOOL,
        keys: u32,
    ) -> HRESULT {
        if escape != 0 {
            DRAGDROP_S_CANCEL
        } else if keys & MK_LBUTTON == 0 {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    unsafe extern "system" fn give_feedback(_: *mut c_void, _: u32) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }

    /// Releases a COM object.
    ///
    /// # Safety
    /// `object` must be a live COM object owned by the caller.
    unsafe fn release(object: *mut c_void) {
        #[repr(C)]
        struct Unknown {
            query_interface: usize,
            add_ref: usize,
            release: unsafe extern "system" fn(*mut c_void) -> u32,
        }
        // SAFETY: a COM object starts with a pointer to its vtable, which
        // starts with IUnknown's methods.
        unsafe {
            let vtbl = &**(object as *const *const Unknown);
            (vtbl.release)(object);
        }
    }
}

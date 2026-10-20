//! Conversion progress on the taskbar button (Windows), so a long batch
//! can be followed while the window is in the background.

use winit::window::Window;

/// Shows `progress` (0 to 1) on the window's taskbar button, or clears it
/// with `None`. Does nothing where the platform has no such display.
#[derive(Default)]
pub struct Taskbar {
    #[cfg(windows)]
    list: Option<imp::TaskbarList>,
}

impl Taskbar {
    #[cfg_attr(not(windows), allow(unused_variables, clippy::unused_self))]
    pub fn set_progress(&mut self, window: &Window, progress: Option<f32>) {
        #[cfg(windows)]
        {
            use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            let Ok(handle) = window.window_handle() else {
                return;
            };
            let RawWindowHandle::Win32(handle) = handle.as_raw() else {
                return;
            };
            if self.list.is_none() {
                self.list = imp::TaskbarList::new();
            }
            if let Some(list) = &self.list {
                list.set_progress(handle.hwnd.get() as _, progress);
            }
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
    use windows_sys::Win32::UI::Shell::{TBPF_NOPROGRESS, TBPF_NORMAL, TaskbarList as CLSID};
    use windows_sys::core::{GUID, HRESULT};

    const IID_ITASKBARLIST3: GUID = GUID::from_u128(0xea1afb91_9e28_4b86_90e9_9e9f8a5eefaf);

    /// The start of `ITaskbarList3`'s vtable, up to the methods used here.
    #[repr(C)]
    struct Vtbl {
        query_interface: usize,
        add_ref: usize,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        // ITaskbarList
        hr_init: unsafe extern "system" fn(*mut c_void) -> HRESULT,
        add_tab: usize,
        delete_tab: usize,
        activate_tab: usize,
        set_active_alt: usize,
        // ITaskbarList2
        mark_fullscreen_window: usize,
        // ITaskbarList3
        set_progress_value: unsafe extern "system" fn(*mut c_void, HWND, u64, u64) -> HRESULT,
        set_progress_state: unsafe extern "system" fn(*mut c_void, HWND, i32) -> HRESULT,
    }

    pub struct TaskbarList(*mut c_void);

    impl TaskbarList {
        /// `None` if the shell offers no taskbar (e.g. Windows Server Core).
        /// COM is already set up on the UI thread by winit (for drag and
        /// drop).
        pub fn new() -> Option<Self> {
            let mut list = std::ptr::null_mut();
            // SAFETY: plain COM object creation; the pointer is only used if
            // it succeeded.
            let created = unsafe {
                CoCreateInstance(
                    &CLSID,
                    std::ptr::null_mut(),
                    CLSCTX_INPROC_SERVER,
                    &IID_ITASKBARLIST3,
                    &mut list,
                )
            };
            if created < 0 || list.is_null() {
                return None;
            }
            let list = Self(list);
            // SAFETY: `list` is a live ITaskbarList3.
            (unsafe { (list.vtbl().hr_init)(list.0) } >= 0).then_some(list)
        }

        fn vtbl(&self) -> &Vtbl {
            // SAFETY: a COM object starts with a pointer to its vtable.
            unsafe { &**(self.0 as *const *const Vtbl) }
        }

        pub fn set_progress(&self, hwnd: HWND, progress: Option<f32>) {
            const TOTAL: u64 = 1000;
            // SAFETY: `self.0` is a live ITaskbarList3 and `hwnd` is the
            // window's handle.
            unsafe {
                match progress {
                    Some(p) => {
                        (self.vtbl().set_progress_state)(self.0, hwnd, TBPF_NORMAL);
                        let done = (p.clamp(0.0, 1.0) * TOTAL as f32) as u64;
                        (self.vtbl().set_progress_value)(self.0, hwnd, done, TOTAL);
                    }
                    None => {
                        (self.vtbl().set_progress_state)(self.0, hwnd, TBPF_NOPROGRESS);
                    }
                }
            }
        }
    }

    impl Drop for TaskbarList {
        fn drop(&mut self) {
            // SAFETY: releases the reference taken by CoCreateInstance.
            unsafe { (self.vtbl().release)(self.0) };
        }
    }
}

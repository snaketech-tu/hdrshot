//! The icon in the notification area (the system tray) on Windows, shown
//! while hdrshot keeps running with its window closed. Clicking it opens
//! the window, its menu pauses watching or quits, and notifications come
//! from it.

use std::sync::mpsc::Sender;

/// Whether this platform has a tray icon.
pub(crate) const SUPPORTED: bool = cfg!(windows);

/// What the user did with the icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) enum TrayEvent {
    /// Clicked the icon or a notification, or chose Open hdrshot.
    Open,
    /// Chose Pause watching or Resume watching.
    TogglePause,
    Quit,
}

/// The watch folder item of the icon's menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WatchItem {
    /// No item, as the watch folder is off.
    None,
    Pause,
    Resume,
}

pub(crate) struct Tray {
    #[cfg(windows)]
    icon: imp::Icon,
}

impl Tray {
    /// Adds the icon, which sends what the user does with it to `events`
    /// and repaints `ctx`. `None` where there is no tray.
    #[cfg_attr(not(windows), allow(unused_variables))]
    pub(crate) fn new(events: Sender<TrayEvent>, ctx: egui::Context) -> Option<Self> {
        #[cfg(windows)]
        return imp::Icon::new(events, ctx).map(|icon| Self { icon });
        #[cfg(not(windows))]
        None
    }

    /// Sets the icon's tooltip and the watch folder item of its menu.
    #[cfg_attr(not(windows), allow(unused_variables, clippy::unused_self))]
    pub(crate) fn update(&self, tip: &str, watch: WatchItem) {
        #[cfg(windows)]
        self.icon.update(tip, watch);
    }

    /// Shows a notification. Windows shows it as from hdrshot, and
    /// clicking it sends [`TrayEvent::Open`].
    #[cfg_attr(not(windows), allow(unused_variables, clippy::unused_self))]
    pub(crate) fn notify(&self, title: &str, text: &str) {
        #[cfg(windows)]
        self.icon.notify(title, text);
    }
}

#[cfg(windows)]
mod imp {
    use std::cell::{Cell, RefCell};
    use std::sync::mpsc::Sender;

    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_LARGE_ICON,
        NIIF_RESPECT_QUIET_TIME, NIIF_USER, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION,
        NIN_BALLOONUSERCLICK, NIN_SELECT, NOTIFY_ICON_DATA_FLAGS, NOTIFY_ICON_MESSAGE,
        NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu,
        DestroyWindow, GWLP_USERDATA, GetSystemMetrics, GetWindowLongPtrW, HICON, IDI_APPLICATION,
        IMAGE_ICON, LR_DEFAULTCOLOR, LoadIconW, LoadImageW, MF_SEPARATOR, MF_STRING, PostMessageW,
        RegisterClassW, RegisterWindowMessageW, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON,
        SM_MENUDROPALIGNMENT, SetForegroundWindow, SetMenuDefaultItem, SetWindowLongPtrW,
        TPM_LEFTALIGN, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTALIGN, TPM_RIGHTBUTTON,
        TrackPopupMenuEx, WM_APP, WM_CONTEXTMENU, WM_NULL, WNDCLASSW, WS_OVERLAPPED,
    };

    use super::{TrayEvent, WatchItem};

    /// The message the icon sends its window.
    const CALLBACK: u32 = WM_APP + 1;
    /// NIN_SELECT from the keyboard (Enter or Space), not in `windows-sys`.
    const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
    /// The icon's id within its window.
    const ICON_ID: u32 = 1;
    /// The app icon's resource id, as `winresource` embeds it.
    const APP_ICON: u16 = 1;
    /// Menu command ids.
    const OPEN: usize = 1;
    const TOGGLE: usize = 2;
    const QUIT: usize = 3;

    /// The icon and the hidden window that receives its messages.
    pub struct Icon {
        hwnd: HWND,
        state: *mut State,
    }

    /// What the window procedure needs. Only touched on the UI thread,
    /// briefly, so the cells are never borrowed twice.
    struct State {
        events: Sender<TrayEvent>,
        ctx: egui::Context,
        watch: Cell<WatchItem>,
        tip: RefCell<String>,
        small: HICON,
        large: HICON,
        /// Sent to every window when Explorer (re)starts, which drops the
        /// icons of the session before.
        taskbar_created: u32,
    }

    impl Icon {
        pub fn new(events: Sender<TrayEvent>, ctx: egui::Context) -> Option<Self> {
            let class = wide("hdrshot tray");
            // SAFETY: plain Win32 calls; the strings outlive them, and the
            // window is destroyed in `drop` before `state` is freed.
            unsafe {
                let instance = GetModuleHandleW(std::ptr::null());
                let class_info = WNDCLASSW {
                    lpfnWndProc: Some(window_proc),
                    hInstance: instance,
                    lpszClassName: class.as_ptr(),
                    ..std::mem::zeroed()
                };
                // Fails harmlessly when a tray was made before and the class
                // exists already.
                RegisterClassW(&class_info);
                // A hidden top-level window rather than a message-only one,
                // which wouldn't get `TaskbarCreated`.
                let hwnd = CreateWindowExW(
                    0,
                    class.as_ptr(),
                    wide("hdrshot").as_ptr(),
                    WS_OVERLAPPED,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    instance,
                    std::ptr::null(),
                );
                if hwnd.is_null() {
                    return None;
                }
                let load = |w, h| {
                    let icon = LoadImageW(
                        instance,
                        APP_ICON as usize as *const u16,
                        IMAGE_ICON,
                        GetSystemMetrics(w),
                        GetSystemMetrics(h),
                        LR_DEFAULTCOLOR,
                    );
                    if icon.is_null() {
                        LoadIconW(std::ptr::null_mut(), IDI_APPLICATION)
                    } else {
                        icon
                    }
                };
                let state = Box::into_raw(Box::new(State {
                    events,
                    ctx,
                    watch: Cell::new(WatchItem::None),
                    tip: RefCell::new("hdrshot".to_owned()),
                    small: load(SM_CXSMICON, SM_CYSMICON),
                    large: load(SM_CXICON, SM_CYICON),
                    taskbar_created: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
                }));
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
                // While Explorer is still starting, as at sign-in, this
                // fails, and `TaskbarCreated` adds the icon later.
                add(hwnd, &*state);
                Some(Self { hwnd, state })
            }
        }

        fn state(&self) -> &State {
            // SAFETY: freed only in `drop`.
            unsafe { &*self.state }
        }

        pub fn update(&self, tip: &str, watch: WatchItem) {
            let state = self.state();
            state.watch.set(watch);
            if *state.tip.borrow() == tip {
                return;
            }
            *state.tip.borrow_mut() = tip.to_owned();
            let mut data = data(self.hwnd, NIF_TIP | NIF_SHOWTIP);
            copy(&mut data.szTip, tip);
            notify_icon(NIM_MODIFY, &data);
        }

        pub fn notify(&self, title: &str, text: &str) {
            let mut data = data(self.hwnd, NIF_INFO);
            copy(&mut data.szInfoTitle, title);
            copy(&mut data.szInfo, text);
            data.dwInfoFlags = NIIF_USER | NIIF_LARGE_ICON | NIIF_RESPECT_QUIET_TIME;
            data.hBalloonIcon = self.state().large;
            notify_icon(NIM_MODIFY, &data);
        }
    }

    impl Drop for Icon {
        fn drop(&mut self) {
            notify_icon(NIM_DELETE, &data(self.hwnd, 0));
            // SAFETY: the window procedure stops using `state` once the
            // pointer is cleared, and nothing else holds it.
            unsafe {
                SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
                DestroyWindow(self.hwnd);
                let state = Box::from_raw(self.state);
                DestroyIcon(state.small);
                DestroyIcon(state.large);
            }
        }
    }

    /// Adds the icon to the notification area.
    fn add(hwnd: HWND, state: &State) {
        let mut data = data(hwnd, NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP);
        data.uCallbackMessage = CALLBACK;
        data.hIcon = state.small;
        copy(&mut data.szTip, &state.tip.borrow());
        if notify_icon(NIM_ADD, &data) {
            // For the newer messages: NIN_SELECT, WM_CONTEXTMENU with the
            // position, and the tooltip shown with NIF_SHOWTIP.
            data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            notify_icon(NIM_SETVERSION, &data);
        }
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // SAFETY: the pointer is set right after the window is made and
        // cleared before `State` is freed.
        let state = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State).as_ref() };
        let Some(state) = state else {
            // SAFETY: the default handling for any other message.
            return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
        };
        if message == CALLBACK {
            match lparam as u32 & 0xffff {
                NIN_SELECT | NIN_KEYSELECT | NIN_BALLOONUSERCLICK => state.send(TrayEvent::Open),
                WM_CONTEXTMENU => {
                    let x = (wparam & 0xffff) as i16 as i32;
                    let y = ((wparam >> 16) & 0xffff) as i16 as i32;
                    if let Some(event) = menu(hwnd, state.watch.get(), x, y) {
                        state.send(event);
                    }
                }
                _ => {}
            }
            return 0;
        }
        if message == state.taskbar_created {
            add(hwnd, state);
            return 0;
        }
        // SAFETY: the default handling for any other message.
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    impl State {
        fn send(&self, event: TrayEvent) {
            if self.events.send(event).is_ok() {
                self.ctx.request_repaint();
            }
        }
    }

    /// Shows the icon's menu at `x`, `y` and returns the choice.
    fn menu(hwnd: HWND, watch: WatchItem, x: i32, y: i32) -> Option<TrayEvent> {
        // SAFETY: plain Win32 calls on a menu made and destroyed here.
        unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return None;
            }
            AppendMenuW(menu, MF_STRING, OPEN, wide("Open hdrshot").as_ptr());
            let watch_label = match watch {
                WatchItem::None => None,
                WatchItem::Pause => Some("Pause watching"),
                WatchItem::Resume => Some("Resume watching"),
            };
            if let Some(label) = watch_label {
                AppendMenuW(menu, MF_STRING, TOGGLE, wide(label).as_ptr());
            }
            AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
            AppendMenuW(menu, MF_STRING, QUIT, wide("Quit hdrshot").as_ptr());
            SetMenuDefaultItem(menu, OPEN as u32, 0);
            let align = if GetSystemMetrics(SM_MENUDROPALIGNMENT) != 0 {
                TPM_RIGHTALIGN
            } else {
                TPM_LEFTALIGN
            };
            // Without this the menu stays open when clicking elsewhere.
            SetForegroundWindow(hwnd);
            let choice = TrackPopupMenuEx(
                menu,
                align | TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
                x,
                y,
                hwnd,
                std::ptr::null(),
            );
            PostMessageW(hwnd, WM_NULL, 0, 0);
            DestroyMenu(menu);
            match choice as usize {
                OPEN => Some(TrayEvent::Open),
                TOGGLE => Some(TrayEvent::TogglePause),
                QUIT => Some(TrayEvent::Quit),
                _ => None,
            }
        }
    }

    fn data(hwnd: HWND, flags: NOTIFY_ICON_DATA_FLAGS) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: ICON_ID,
            uFlags: flags,
            ..Default::default()
        }
    }

    fn notify_icon(message: NOTIFY_ICON_MESSAGE, data: &NOTIFYICONDATAW) -> bool {
        // SAFETY: `data` is a complete NOTIFYICONDATAW.
        unsafe { Shell_NotifyIconW(message, data) != 0 }
    }

    /// Copies `text` into a fixed-size field, cut short to fit with its
    /// terminating zero.
    fn copy(field: &mut [u16], text: &str) {
        let mut units: Vec<u16> = text.encode_utf16().take(field.len() - 1).collect();
        // Don't leave half of a surrogate pair at the end.
        if units.last().is_some_and(|u| (0xd800..0xdc00).contains(u)) {
            units.pop();
        }
        field[..units.len()].copy_from_slice(&units);
        field[units.len()] = 0;
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }

    #[cfg(test)]
    mod tests {
        use super::copy;

        #[test]
        fn text_is_cut_to_fit() {
            let mut field = [1; 8];
            copy(&mut field, "hdrshot: watching");
            assert_eq!(String::from_utf16(&field[..7]).unwrap(), "hdrshot");
            assert_eq!(field[7], 0);
            // No half of a character made of two UTF-16 units.
            let mut field = [1; 4];
            copy(&mut field, "ab\u{1F600}");
            assert_eq!(field[..3], [u16::from(b'a'), u16::from(b'b'), 0]);
        }
    }
}

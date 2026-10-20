//! Files opened from Finder ("Open With", double-click, dropping onto the
//! Dock icon). macOS doesn't pass them as arguments: it sends them to the
//! running app, starting it first if needed, as an "open documents" event
//! that reaches the application delegate. winit's delegate doesn't handle
//! it, so the method is added to winit's delegate class here.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::mpsc::Sender;

use objc2::MainThreadMarker;
use objc2::runtime::{AnyObject, Imp, Sel};
use objc2::sel;
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSArray, NSURL};

/// Where opened files go.
static OPENED: OnceLock<(Sender<Vec<PathBuf>>, egui::Context)> = OnceLock::new();

/// Sends files opened from Finder to `opened`. Must run on the main thread
/// after winit's event loop is created (which sets its delegate) and before
/// it runs, so the files the app is launched with arrive too.
pub fn receive_opened_files(opened: Sender<Vec<PathBuf>>, ctx: egui::Context) {
    if OPENED.set((opened, ctx)).is_err() {
        return;
    }
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() else {
        return;
    };
    let delegate: &AnyObject = delegate.as_ref();
    let class = delegate.class();
    // SAFETY: the signature matches the type encoding: returns void, takes
    // self, the selector and two objects (the application and the URLs).
    // Adding a method fails harmlessly if the class already has one.
    unsafe {
        objc2::ffi::class_addMethod(
            class as *const _ as *mut _,
            sel!(application:openURLs:),
            std::mem::transmute::<OpenUrls, Imp>(open_urls),
            c"v@:@@".as_ptr(),
        );
    }
}

type OpenUrls =
    unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut NSArray<NSURL>);

/// `-[NSApplicationDelegate application:openURLs:]`
unsafe extern "C-unwind" fn open_urls(
    _this: *mut AnyObject,
    _cmd: Sel,
    _application: *mut AnyObject,
    urls: *mut NSArray<NSURL>,
) {
    // SAFETY: AppKit passes a valid array of URLs, or nil.
    let Some(urls) = (unsafe { urls.as_ref() }) else {
        return;
    };
    let files: Vec<PathBuf> = urls.iter().filter_map(|url| url.to_file_path()).collect();
    if let Some((opened, ctx)) = OPENED.get()
        && !files.is_empty()
        && opened.send(files).is_ok()
    {
        ctx.request_repaint();
    }
}

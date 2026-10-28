//! Decoding one AV1 image, as an AVIF item holds, with `rav1d`, the Rust
//! port of dav1d. It is built without its assembly, so no assembler is
//! needed, and used through its dav1d-compatible C interface: that is the
//! only interface it offers, so this module is the one place with `unsafe`
//! code, kept to those calls.
//!
//! A panic inside that interface can't unwind out of it and would abort
//! the whole program, and `rav1d` panics on some damaged files. So each
//! image is decoded on a thread of its own, and a panic there is turned
//! into an error for that file (see [`decode`]).

use std::cell::Cell;
use std::mem::MaybeUninit;
use std::ptr::NonNull;
use std::sync::Once;
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

use rav1d::include::dav1d::data::Dav1dData;
use rav1d::include::dav1d::dav1d::{Dav1dContext, Dav1dSettings};
use rav1d::include::dav1d::headers::{
    DAV1D_PIXEL_LAYOUT_I400, DAV1D_PIXEL_LAYOUT_I420, DAV1D_PIXEL_LAYOUT_I422,
};
use rav1d::include::dav1d::picture::Dav1dPicture;
use rav1d::src::lib::{
    dav1d_close, dav1d_data_create, dav1d_data_unref, dav1d_default_settings, dav1d_get_picture,
    dav1d_open, dav1d_picture_unref, dav1d_send_data,
};

use crate::{Error, Result};

/// A decoded picture: planar YUV (or monochrome) samples.
pub(super) struct Planes {
    pub width: usize,
    pub height: usize,
    pub bit_depth: u32,
    /// Horizontal and vertical chroma subsampling, as shifts; `None` for
    /// monochrome.
    pub subsampling: Option<(u32, u32)>,
    pub y: Vec<u16>,
    pub u: Vec<u16>,
    pub v: Vec<u16>,
    /// Color description from the AV1 sequence header: primaries, transfer,
    /// matrix and full range.
    pub color: (u8, u8, u8, bool),
}

impl Planes {
    pub fn chroma_width(&self) -> usize {
        self.subsampling
            .map_or(0, |(x, _)| (self.width + (1 << x) - 1) >> x)
    }

    pub fn chroma_height(&self) -> usize {
        self.subsampling
            .map_or(0, |(_, y)| (self.height + (1 << y) - 1) >> y)
    }
}

/// An open decoder, closed when dropped.
struct Context(Option<Dav1dContext>);

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: the context came from `dav1d_open` and is closed once.
        unsafe { dav1d_close(Some(NonNull::from(&mut self.0))) };
    }
}

/// A decoded picture, released when dropped.
struct Picture(Dav1dPicture);

impl Drop for Picture {
    fn drop(&mut self) {
        // SAFETY: the picture was filled in by `dav1d_get_picture`, or is
        // the default (empty) one, which unref ignores.
        unsafe { dav1d_picture_unref(Some(NonNull::from(&mut self.0))) };
    }
}

fn check(res: rav1d::Dav1dResult, what: &str) -> Result<()> {
    if res.0 == 0 {
        Ok(())
    } else {
        Err(Error::Decode(format!("AV1 {what} failed ({})", res.0)))
    }
}

thread_local! {
    /// On a decoding thread, where its result goes.
    static RESULT: Cell<Option<Sender<Result<Planes>>>> = const { Cell::new(None) };
}

/// Longest time one image may take, far more than even a large photo
/// needs, in case a damaged file sends the decoder into a loop.
const TIMEOUT: Duration = Duration::from_secs(120);

/// Decodes `obus`, the AV1 data of one image, with its configuration OBUs
/// first.
///
/// The decoding runs on a new thread. If `rav1d` panics there, a panic
/// hook reports the file as damaged and parks the thread for good instead
/// of letting the panic reach the C interface, where it would abort the
/// program. The parked thread and its memory are lost, which is rare and
/// far better than losing the app with every other file being converted.
///
/// The parked thread keeps the standard library's lock on the panic hook,
/// so a program reading AVIFs must not call `std::panic::set_hook` or
/// `take_hook` after a damaged one: it would wait forever. That includes
/// the end of a `cargo test` run, so tests of this run hdrshot as a
/// separate program.
pub(super) fn decode(obus: &[u8]) -> Result<Planes> {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| match RESULT.take() {
            Some(result) => {
                let _ = result.send(Err(Error::Decode("the AV1 data is damaged".into())));
                loop {
                    std::thread::park();
                }
            }
            None => previous(info),
        }));
    });
    let (tx, rx) = channel();
    let obus = obus.to_vec();
    std::thread::Builder::new()
        .name("hdrshot-av1".into())
        .spawn(move || {
            RESULT.set(Some(tx));
            let result = decode_here(&obus);
            if let Some(tx) = RESULT.take() {
                let _ = tx.send(result);
            }
        })
        .map_err(|e| Error::Decode(format!("could not start the AV1 decoder: {e}")))?;
    rx.recv_timeout(TIMEOUT)
        .unwrap_or_else(|_| Err(Error::Decode("the AV1 decoder stopped responding".into())))
}

/// Decodes `obus` on this thread; see [`decode`].
fn decode_here(obus: &[u8]) -> Result<Planes> {
    // SAFETY: each call gets pointers to live, initialized values of the
    // types it expects; the context, data and picture are released by the
    // guards above or by `dav1d_data_unref`.
    unsafe {
        let mut settings = MaybeUninit::<Dav1dSettings>::uninit();
        dav1d_default_settings(NonNull::from(&mut settings).cast());
        let settings = settings.assume_init_mut();
        // No worker threads, so that a panic on a damaged file always
        // happens on this thread, where `decode` catches it; a 4K image
        // still takes about a second. One frame at a time, and only the
        // final layer of a layered image. Film grain is part of the picture.
        settings.n_threads = 1;
        settings.max_frame_delay = 1;
        settings.all_layers = 0;
        settings.apply_grain = 1;
        let mut ctx = Context(None);
        check(
            dav1d_open(
                Some(NonNull::from(&mut ctx.0)),
                Some(NonNull::from(settings)),
            ),
            "decoder setup",
        )?;
        let c = ctx
            .0
            .ok_or_else(|| Error::Decode("AV1 decoder setup failed".into()))?;

        let mut data = Dav1dData::default();
        let buf = dav1d_data_create(Some(NonNull::from(&mut data)), obus.len());
        let Some(buf) = NonNull::new(buf) else {
            return Err(Error::Decode("out of memory".into()));
        };
        std::ptr::copy_nonoverlapping(obus.as_ptr(), buf.as_ptr(), obus.len());

        let mut picture = Picture(Dav1dPicture::default());
        let mut have_picture = false;
        let sent = loop {
            let res = dav1d_send_data(Some(c), Some(NonNull::from(&mut data)));
            if res.0 == 0 {
                break Ok(());
            }
            // The decoder holds a picture and wants it taken first.
            let mut pending = Picture(Dav1dPicture::default());
            if dav1d_get_picture(Some(c), Some(NonNull::from(&mut pending.0))).0 != 0 {
                break check(res, "decoding");
            }
            picture = pending;
            have_picture = true;
        };
        dav1d_data_unref(Some(NonNull::from(&mut data)));
        sent?;
        loop {
            let mut next = Picture(Dav1dPicture::default());
            if dav1d_get_picture(Some(c), Some(NonNull::from(&mut next.0))).0 != 0 {
                break;
            }
            picture = next;
            have_picture = true;
        }
        if !have_picture {
            return Err(Error::Decode("the AV1 data holds no picture".into()));
        }
        copy_planes(&picture.0)
    }
}

/// Copies the samples out of a decoded picture.
///
/// # Safety
///
/// `pic` must be a picture returned by `dav1d_get_picture`.
unsafe fn copy_planes(pic: &Dav1dPicture) -> Result<Planes> {
    let (width, height) = (pic.p.w.max(0) as usize, pic.p.h.max(0) as usize);
    let bit_depth = pic.p.bpc as u32;
    let subsampling = match pic.p.layout {
        DAV1D_PIXEL_LAYOUT_I400 => None,
        DAV1D_PIXEL_LAYOUT_I420 => Some((1, 1)),
        DAV1D_PIXEL_LAYOUT_I422 => Some((1, 0)),
        _ => Some((0, 0)),
    };
    let color = pic
        .seq_hdr
        // SAFETY: the sequence header lives as long as the picture.
        .map(|h| unsafe {
            let h = h.as_ref();
            (h.pri as u8, h.trc as u8, h.mtrx as u8, h.color_range != 0)
        })
        .unwrap_or((2, 2, 2, false));
    let mut planes = Planes {
        width,
        height,
        bit_depth,
        subsampling,
        y: Vec::new(),
        u: Vec::new(),
        v: Vec::new(),
        color,
    };
    let (cw, ch) = (planes.chroma_width(), planes.chroma_height());
    let read = |plane: usize, w: usize, h: usize| -> Result<Vec<u16>> {
        let ptr = pic.data[plane]
            .ok_or_else(|| Error::Decode("AV1 picture without data".into()))?
            .cast::<u8>();
        let stride = pic.stride[plane.min(1)];
        let mut out = Vec::with_capacity(w * h);
        for row in 0..h as isize {
            // SAFETY: each row lies within the plane, `stride` bytes apart;
            // high bit depths store 16-bit samples.
            unsafe {
                let start = ptr.as_ptr().offset(row * stride);
                if bit_depth > 8 {
                    let row = std::slice::from_raw_parts(start, w * 2);
                    out.extend(
                        row.as_chunks::<2>()
                            .0
                            .iter()
                            .map(|b| u16::from_ne_bytes(*b)),
                    );
                } else {
                    out.extend(
                        std::slice::from_raw_parts(start, w)
                            .iter()
                            .map(|&b| u16::from(b)),
                    );
                }
            }
        }
        Ok(out)
    };
    planes.y = read(0, width, height)?;
    if subsampling.is_some() {
        planes.u = read(1, cw, ch)?;
        planes.v = read(2, cw, ch)?;
    }
    Ok(planes)
}

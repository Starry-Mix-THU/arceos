use core::{fmt, slice};

use addr2line::Context;
use alloc::{borrow::Cow, sync::Arc};
use paste::paste;

pub type DwarfReader = gimli::EndianSlice<'static, gimli::RunTimeEndian>;

static mut CONTEXT: Option<Arc<Context<DwarfReader>>> = None;

macro_rules! generate_sections {
    ($($name:ident),*) => {
        unsafe extern "C" {
            safe static _debug_start: [u8; 0];
            paste! {
                $(
                    safe static [<_ $name _end>]: [u8; 0];
                )*
            }
        }

        let current = _debug_start.as_ptr();
        paste! {
            $(
                let $name = DwarfReader::new(
                    unsafe {
                        core::slice::from_raw_parts(
                            current,
                            [<_ $name _end>]
                                .as_ptr()
                                .offset_from_unsigned(current),
                        )
                    },
                    gimli::RunTimeEndian::default(),
                );
                #[allow(unused_variables)]
                let current = [<_ $name _end>].as_ptr();
            )*
        }
    };
}

pub fn init() {
    generate_sections!(
        debug_abbrev,
        debug_addr,
        debug_aranges,
        debug_info,
        debug_line,
        debug_line_str,
        debug_ranges,
        debug_rnglists,
        debug_str,
        debug_str_offsets
    );

    let default_section = DwarfReader::new(&[], gimli::RunTimeEndian::default());

    match Context::from_sections(
        debug_abbrev.into(),
        debug_addr.into(),
        debug_aranges.into(),
        debug_info.into(),
        debug_line.into(),
        debug_line_str.into(),
        debug_ranges.into(),
        debug_rnglists.into(),
        debug_str.into(),
        debug_str_offsets.into(),
        default_section,
    ) {
        Ok(ctx) => {
            unsafe {
                CONTEXT = Some(Arc::new(ctx));
            }
            axlog::info!("Initialized addr2line context successfully.");
        }
        Err(e) => {
            axlog::error!("Failed to initialize addr2line context: {e}");
        }
    }
}

fn fmt_frame<R: gimli::Reader>(
    f: &mut fmt::Formatter<'_>,
    frame: &addr2line::Frame<R>,
) -> fmt::Result {
    let func = frame
        .function
        .as_ref()
        .and_then(|func| func.demangle().ok())
        .unwrap_or(Cow::Borrowed("<unknown>"));
    writeln!(f, ": {func}")?;

    let Some(location) = &frame.location else {
        return Ok(());
    };
    write!(f, "            at ")?;

    let Some(file) = &location.file else {
        return write!(f, "??");
    };
    write!(f, "{file}")?;
    let Some(line) = location.line else {
        return Ok(());
    };
    write!(f, ":{line}")?;
    let Some(col) = location.column else {
        return Ok(());
    };
    write!(f, ":{col}")?;

    Ok(())
}

/// An iterator over the stack frames in a captured backtrace.
///
/// See [`Backtrace::frames`].
///
/// [`Backtrace::frames`]: crate::Backtrace::frames
pub struct FrameIter<'a> {
    raw: slice::Iter<'a, crate::Frame>,
    inner: Option<addr2line::FrameIter<'static, DwarfReader>>,
}

impl<'a> FrameIter<'a> {
    pub(crate) fn new(frames: &'a [crate::Frame]) -> Self {
        let raw = frames.iter();
        Self { raw, inner: None }
    }
}

impl Iterator for FrameIter<'_> {
    type Item = addr2line::Frame<'static, DwarfReader>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(inner) = &mut self.inner {
            if let Ok(Some(frame)) = inner.next() {
                return Some(frame);
            }
            self.inner = None;
        }

        #[allow(static_mut_refs)]
        let ctx = unsafe { CONTEXT.as_ref()? };

        let mut frame = self.raw.next()?;
        loop {
            if let Ok(inner) = ctx.find_frames(frame.adjust_ip() as _).skip_all_loads() {
                self.inner = Some(inner);
                break;
            } else {
                frame = self.raw.next()?;
                continue;
            }
        }

        self.next()
    }
}

pub(crate) fn fmt_frames(f: &mut fmt::Formatter<'_>, frames: &[crate::Frame]) -> fmt::Result {
    #[allow(static_mut_refs)]
    if unsafe { CONTEXT.is_none() } {
        return write!(f, "Backtracing is not initialized.");
    }
    for (i, frame) in FrameIter::new(frames).enumerate() {
        write!(f, "{i:>4}")?;
        fmt_frame(f, &frame)?;
        writeln!(f)?;
    }

    Ok(())
}

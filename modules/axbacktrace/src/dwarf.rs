use core::{fmt, slice};

use addr2line::Context;
use alloc::{borrow::Cow, sync::Arc};
use gimli::DwarfSections;
use object::{Endianness, Object, ObjectSection, ReadCache, ReadCacheOps};
use thiserror::Error;

pub type DwarfReader = gimli::EndianArcSlice<gimli::RunTimeEndian>;

pub(crate) static mut CONTEXT: Option<Arc<Context<DwarfReader>>> = None;

#[derive(Debug, Error)]
pub enum DwarfError {
    #[error("gimli error: {0}")]
    Gimli(gimli::Error),

    #[error("object error: {0}")]
    Object(#[from] object::Error),
}

/// Set the DWARF sections for addr2line to use.
pub fn set_dwarf_sections(reader: impl ReadCacheOps) -> Result<(), DwarfError> {
    let cache = ReadCache::new(reader);
    let object = object::File::parse(&cache)?;
    assert_eq!(object.endianness(), Endianness::default());

    let dwarf_sections = DwarfSections::load(|id| {
        object::Result::Ok(match object.section_by_name(id.name()) {
            Some(section) => section.data()?,
            None => Default::default(),
        })
    })?;
    let dwarf = dwarf_sections
        .borrow(|section| DwarfReader::new((*section).into(), gimli::RunTimeEndian::default()));
    let context = Context::from_dwarf(dwarf).map_err(DwarfError::Gimli)?;

    unsafe {
        CONTEXT = Some(Arc::new(context));
    }

    Ok(())
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
    for (i, frame) in FrameIter::new(frames).enumerate() {
        write!(f, "{i:>4}")?;
        fmt_frame(f, &frame)?;
        writeln!(f)?;
    }

    Ok(())
}

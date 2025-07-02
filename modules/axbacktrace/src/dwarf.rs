use addr2line::Context;
use alloc::sync::Arc;
use gimli::DwarfSections;
use object::{Endianness, Object, ObjectSection, ReadCache, ReadCacheOps};
use thiserror::Error;

pub(crate) static mut CONTEXT: Option<Arc<Context<Reader>>> = None;

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

    let dwarf_sections = DwarfSections::load(|section| {
        object::Result::Ok(match object.section_by_name(section.name()) {
            Some(section) => Section {
                data: section.uncompressed_data()?.into(),
                relocations: section
                    .relocation_map()
                    .map(|it| RelocationMap(Arc::new(it)))?,
            },
            None => Default::default(),
        })
    })?;
    let dwarf =
        dwarf_sections.borrow(|section| borrow_section(section, gimli::RunTimeEndian::default()));
    let context = Context::from_dwarf(dwarf).map_err(DwarfError::Gimli)?;

    unsafe {
        CONTEXT = Some(Arc::new(context));
    }

    Ok(())
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RelocationMap(Arc<object::read::RelocationMap>);

impl gimli::read::Relocate for RelocationMap {
    fn relocate_address(&self, offset: usize, value: u64) -> gimli::Result<u64> {
        Ok(self.0.relocate(offset as u64, value))
    }

    fn relocate_offset(&self, offset: usize, value: usize) -> gimli::Result<usize> {
        <usize as gimli::ReaderOffset>::from_u64(self.0.relocate(offset as u64, value as u64))
    }
}

#[derive(Default)]
pub(crate) struct Section {
    data: Arc<[u8]>,
    relocations: RelocationMap,
}

type Reader = gimli::RelocateReader<gimli::EndianArcSlice<gimli::RunTimeEndian>, RelocationMap>;

fn borrow_section(section: &Section, endian: gimli::RunTimeEndian) -> Reader {
    let slice = gimli::EndianArcSlice::new(section.data.clone(), endian);
    gimli::RelocateReader::new(slice, section.relocations.clone())
}

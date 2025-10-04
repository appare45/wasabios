use core::fmt;
use core::mem::size_of;

use crate::hpet::HpetRegisters;
use crate::result::Result;

#[repr(packed)]
#[derive(Clone, Copy, Debug)]
struct SystemDescriptionTableHeader {
    signature: [u8; 4],
    length: u32,
    _unused: [u8; 28],
}
const _: () = assert!(size_of::<SystemDescriptionTableHeader>() == 36);

impl SystemDescriptionTableHeader {
    fn expect_signature(&self, sig: &'static [u8; 4]) {
        assert_eq!(self.signature, *sig);
    }
    fn signature(&self) -> &[u8; 4] {
        &self.signature
    }
}

#[repr(packed)]
struct Xsdt {
    header: SystemDescriptionTableHeader,
}

impl Xsdt {
    fn iter(&self) -> XsdtIterator {
        XsdtIterator::new(self)
    }

    // &'staticかも
    fn find_table(&self, sig: &'static [u8; 4]) -> Option<&SystemDescriptionTableHeader> {
        self.iter().find(|&e| e.signature() == sig)
    }

    fn header_size(&self) -> usize {
        size_of::<Self>()
    }
    fn num_of_entries(&self) -> usize {
        (self.header.length as usize - self.header_size()) / size_of::<*const u8>()
    }
    /**
    * index番目のエントリのポインタを返す
    |<-- self (構造体) --------------------->|
    | ヘッダー | エントリ配列（*const u8）...|
               ^           ^
               |           |
        header_size()   + index
    */
    unsafe fn entry(&self, index: usize) -> *const u8 {
        ((self as *const Self as *const u8).add(self.header_size()) as *const *const u8)
            .add(index)
            .read_unaligned()
    }
}

struct XsdtIterator<'a> {
    table: &'a Xsdt,
    index: usize,
}

impl<'a> XsdtIterator<'a> {
    pub fn new(table: &'a Xsdt) -> Self {
        Self { table, index: 0 }
    }
}

impl<'a> Iterator for XsdtIterator<'a> {
    type Item = &'static SystemDescriptionTableHeader;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.table.num_of_entries() {
            None
        } else {
            self.index += 1;
            Some(unsafe {
                &*(self.table.entry(self.index - 1) as *const SystemDescriptionTableHeader)
            })
        }
    }
}

trait AcpiTable {
    const SIGNATURE: &'static [u8; 4];
    type Table;
    fn new(header: &SystemDescriptionTableHeader) -> &Self::Table {
        header.expect_signature(Self::SIGNATURE);
        let mcfg: &Self::Table =
            unsafe { &*(header as *const SystemDescriptionTableHeader as *const Self::Table) };
        mcfg
    }
}

#[repr(packed)]
pub struct GenericAddress {
    address_space_id: u8,
    _unused: [u8; 3],
    address: u64,
}
const _: () = assert!(size_of::<GenericAddress>() == 12);

impl GenericAddress {
    pub fn address_in_memory_space(&self) -> Result<usize> {
        if self.address_space_id == 0 {
            Ok(self.address as usize)
        } else {
            Err("ACPI Generic Address is not in memory spasce")
        }
    }
}

#[repr(packed)]
pub struct AcpiHpetDescriptor {
    _header: SystemDescriptionTableHeader,
    _reserved0: u32,
    address: GenericAddress,
    _reserved1: u32,
}
impl AcpiTable for AcpiHpetDescriptor {
    const SIGNATURE: &'static [u8; 4] = b"HPET";
    type Table = Self;
}
impl AcpiHpetDescriptor {
    pub fn base_address(&self) -> Result<&'static mut HpetRegisters> {
        self.address
            .address_in_memory_space()
            .map(|addr| unsafe { &mut *(addr as *mut HpetRegisters) })
    }
}
const _: () = assert!(size_of::<AcpiHpetDescriptor>() == 56);

#[repr(C)]
#[derive(Debug)]
pub struct AcpiRsdp {
    signature: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    rsdt_address: u32,
    length: u32,
    xsdt_address: u64,
}
impl AcpiRsdp {
    fn xsdt(&self) -> &Xsdt {
        unsafe { &*(self.xsdt_address as *const Xsdt) }
    }
    pub fn hpet(&self) -> Option<&AcpiHpetDescriptor> {
        let xsdt = self.xsdt();
        xsdt.find_table(b"HPET").map(AcpiHpetDescriptor::new)
    }
    pub fn mcfg(&self) -> Option<&AcpiMcfgDescriptor> {
        let xsdt = self.xsdt();
        xsdt.find_table(b"MCFG").map(AcpiMcfgDescriptor::new)
    }
}

#[repr(C, packed)]
#[derive(Debug)]
#[allow(dead_code)]
pub struct AcpiMcfgDescriptor {
    header: SystemDescriptionTableHeader,
    _unused: [u8; 8],
}

impl AcpiTable for AcpiMcfgDescriptor {
    const SIGNATURE: &'static [u8; 4] = b"MCFG";
    type Table = Self;
}
const _: () = assert!(size_of::<AcpiMcfgDescriptor>() == 44);

impl AcpiMcfgDescriptor {
    pub fn header_size(&self) -> usize {
        size_of::<Self>()
    }
    pub fn num_of_entries(&self) -> usize {
        (self.header.length as usize - self.header_size()) / size_of::<EcamEntry>()
    }
    pub fn entry(&self, index: usize) -> Option<&EcamEntry> {
        if index >= self.num_of_entries() {
            None
        } else {
            Some(unsafe {
                &*((self as *const Self as *const u8).add(self.header_size()) as *const EcamEntry)
                    .add(index)
            })
        }
    }
}

#[repr(packed)]
pub struct EcamEntry {
    ecm_base_addr: u64,
    _pci_segment_group: u16,
    start_pci_bus: u8,
    end_pci_bus: u8,
    _reserved: u32,
}

impl EcamEntry {
    pub fn base_addr(&self) -> u64 {
        self.ecm_base_addr
    }
}

impl fmt::Display for EcamEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let base = self.ecm_base_addr;
        let bus_start = self.start_pci_bus;
        let bus_end = self.end_pci_bus;
        write!(
            f,
            "ECAM: BUS [{}..{}] is mapped at {:#X}",
            bus_start, bus_end, base
        )
    }
}

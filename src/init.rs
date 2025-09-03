extern crate alloc;
use alloc::boxed::Box;

use crate::uefi::EfiMemoryType;
use crate::x86::write_cr3;
use crate::x86::PageAttr;
use core::cmp::max;

use crate::allocator::ALLOCATOR;
use crate::uefi::exit_from_efi_boot_services;
use crate::uefi::EfiHandle;
use crate::uefi::EfiSystemTable;
use crate::uefi::MemoryMapHolder;
use crate::x86::PAGE_SIZE;
use crate::x86::PML4;

pub fn init_basic_runtime(
    image_handle: EfiHandle,
    efi_system_table: &EfiSystemTable,
) -> MemoryMapHolder {
    let mut memory_map = MemoryMapHolder::new();
    exit_from_efi_boot_services(image_handle, efi_system_table, &mut memory_map);
    ALLOCATOR.init_with_mmap(&memory_map);
    memory_map
}

pub fn init_paging(memory_map: &MemoryMapHolder) {
    let mut table = PML4::new();
    let mut end_of_mem = 0x1_0000_0000u64;
    for e in memory_map.iter() {
        match e.memory_type() {
            EfiMemoryType::CONVENTIONAL_MEMORY
            | EfiMemoryType::LOADER_CODE
            | EfiMemoryType::LOADER_DATA => {
                end_of_mem = max(
                    end_of_mem,
                    e.physical_start() + e.number_of_pages() * (PAGE_SIZE as u64),
                );
            }
            _ => {}
        }
    }
    table
        .create_mapping(0, end_of_mem, 0, PageAttr::ReadWriteKernel)
        .expect("create_mapping failed");
    unsafe {
        write_cr3(Box::into_raw(table));
    }
}

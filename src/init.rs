use crate::allocator::ALLOCATOR;
use crate::uefi::exit_from_efi_boot_services;
use crate::uefi::EfiHandle;
use crate::uefi::EfiSystemTable;
use crate::uefi::MemoryMapHolder;
// use crate::x86::PML4;

pub fn init_basic_runtime(
    image_handle: EfiHandle,
    efi_system_table: &EfiSystemTable,
) -> MemoryMapHolder {
    let mut memory_map = MemoryMapHolder::new();
    exit_from_efi_boot_services(image_handle, efi_system_table, &mut memory_map);
    ALLOCATOR.init_with_mmap(&memory_map);
    memory_map
}

// pub fn init_paging(memory_map: &MemoryMapHolder) {
//     let mut table = PML4::new();
// }

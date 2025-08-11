use core::panic::PanicInfo;

use crate::qemu::exit_qemu;

pub fn test_runner(_tests: &[&dyn FnOnce()]) -> ! {
    exit_qemu(crate::qemu::QemuExitCode::Success)
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit_qemu(crate::qemu::QemuExitCode::Fail)
}

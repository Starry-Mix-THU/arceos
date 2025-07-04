use memory_addr::VirtAddr;

use crate::mem::virt_to_phys;

/// The maximum number of bytes that can be read at once.
const MAX_RW_SIZE: usize = 256;

/// Writes a byte to the console.
pub fn putchar(c: u8) {
    sbi_rt::console_write_byte(c);
}

/// Tries to write bytes to the console from input u8 slice.
/// Returns the number of bytes written.
fn try_write_bytes(bytes: &[u8]) -> usize {
    sbi_rt::console_write(sbi_rt::Physical::new(
        // A maximum of 256 bytes can be written at a time
        // to prevent SBI from disabling IRQs for too long.
        bytes.len().min(MAX_RW_SIZE),
        virt_to_phys(VirtAddr::from_ptr_of(bytes.as_ptr())).as_usize(),
        0,
    ))
    .value
}

/// Writes bytes to the console from input u8 slice.
pub fn write_bytes(bytes: &[u8]) {
    let bytes = if cfg!(feature = "uspace")
        && (bytes.as_ptr() as usize) < axconfig::plat::PHYS_VIRT_OFFSET
    {
        // If the address is from userspace, we need to copy the bytes to kernel space.
        &bytes.to_vec()
    } else {
        bytes
    };

    let mut write_len = 0;
    while write_len < bytes.len() {
        let len = try_write_bytes(&bytes[write_len..]);
        if len == 0 {
            break;
        }
        write_len += len;
    }
}

fn read_bytes_impl(bytes: &mut [u8]) -> usize {
    sbi_rt::console_read(sbi_rt::Physical::new(
        bytes.len().min(MAX_RW_SIZE),
        virt_to_phys(VirtAddr::from_mut_ptr_of(bytes.as_mut_ptr())).as_usize(),
        0,
    ))
    .value
}

/// Reads bytes from the console into the given mutable slice.
/// Returns the number of bytes read.
pub fn read_bytes(bytes: &mut [u8]) -> usize {
    if cfg!(feature = "uspace") && (bytes.as_mut_ptr() as usize) < axconfig::plat::PHYS_VIRT_OFFSET
    {
        let mut temp = bytes.to_vec();
        let len = read_bytes_impl(&mut temp);
        bytes[..len].copy_from_slice(&temp[..len]);
        len
    } else {
        read_bytes_impl(bytes)
    }
}

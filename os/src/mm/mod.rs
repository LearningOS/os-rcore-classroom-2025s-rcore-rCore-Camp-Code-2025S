//! Memory management implementation
//!
//! SV39 page-based virtual-memory architecture for RV64 systems, and
//! everything about memory management, like frame allocator, page table,
//! map area and memory set, is implemented here.
//!
//! Every task or process has a memory_set to control its virtual memory.

mod address;
mod frame_allocator;
mod heap_allocator;
mod memory_set;
mod page_table;

pub use address::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum};
pub use address::{StepByOne, VPNRange};
pub use frame_allocator::{frame_alloc, FrameTracker};
pub use memory_set::remap_test;
pub use memory_set::{kernel_stack_position, MapPermission, MemorySet, KERNEL_SPACE};
pub use page_table::{translated_byte_buffer, PageTableEntry};
pub use page_table::{PTEFlags, PageTable};

/// initiate heap allocator, frame allocator and kernel space
pub fn init() {
    heap_allocator::init_heap();
    frame_allocator::init_frame_allocator();
    KERNEL_SPACE.exclusive_access().activate();
}

///
pub fn copy_to_user(user_token: usize, user_ptr: *mut u8, kernel_src: *const u8, len: usize) -> Result<(), ()> {
    let mut buffers = translated_byte_buffer(user_token, user_ptr, len);
    let mut offset = 0;

    for buf in buffers.iter_mut() {
        let copy_len = core::cmp::min(buf.len(), len - offset);
        if len == 0 || copy_len == 0 {
            return Err(());
        }
        unsafe {
            core::ptr::copy_nonoverlapping(kernel_src.add(offset), buf.as_mut_ptr(), copy_len);
        }
        offset += copy_len;
    }

    if offset == len {
        Ok(())
    } else {
        Err(())
    }
}

/// 
pub fn read_virtaddr(token: usize, virtaddr: usize) -> Result<isize, ()> {
    let pgtb = PageTable::from_token(token);
    let vpn = VirtAddr::from(virtaddr).floor();
    // 获取pte
    if let Some(pte) = pgtb.translate(vpn) {
        if !pte.is_valid() || !pte.readable() {
            //页表项无效或不可读
            Err(())
        } else {
            let buf = translated_byte_buffer(token, virtaddr as *const u8, 1);
            if buf.is_empty() {
                Err(())
            } else {
                let val = buf[0][0];
                Ok(val as isize)
            }
        }
    } else {
        Err(())
    }
}

/// 
pub fn write_virtaddr(token: usize, virtaddr: usize, data: u8) -> Result<(), ()> {
    let pgtb = PageTable::from_token(token);
    let vpn = VirtAddr::from(virtaddr).floor();
    // 获取pte
    if let Some(pte) = pgtb.translate(vpn) {
        if !pte.is_valid() || !pte.writable() {
            //页表项无效或不可写
            Err(())
        } else {
            let mut buf = translated_byte_buffer(token, virtaddr as *const u8, 1);
            if buf.is_empty() {
                Err(())
            } else {
                buf[0][0] = data;
                Ok(())
            }
        }
    } else {
        Err(())
    }
}
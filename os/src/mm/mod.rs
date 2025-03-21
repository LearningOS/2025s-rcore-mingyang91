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
use address::{StepByOne, VPNRange};
pub use frame_allocator::{frame_alloc, FrameTracker};
pub use memory_set::remap_test;
pub use memory_set::{kernel_stack_position, MapPermission, MemorySet, KERNEL_SPACE};
pub use page_table::{translated_byte_buffer, PageTableEntry};
use page_table::{PTEFlags, PageTable};

/// initiate heap allocator, frame allocator and kernel space
pub fn init() {
    heap_allocator::init_heap();
    frame_allocator::init_frame_allocator();
    KERNEL_SPACE.exclusive_access().activate();
}

pub fn trace_read<T>(token: usize, src: usize) -> Option<T> {
    let pt = PageTable::from_token(token);
    let Some(pte) = pt.translate(src.into()) else {
        return None;
    };

    if !pte.is_valid() {
        return None;
    }

    if !pte.readable() {
        return None;
    }

    let mut dst = core::mem::MaybeUninit::<T>::uninit();
    let dst_ptr = dst.as_mut_ptr();
    let dst_buf_ptr: *mut u8 = unsafe { core::mem::transmute(dst_ptr) };
    let len = core::mem::size_of::<T>();

    let src_frames = translated_byte_buffer(token, src as *const u8, len);

    let mut offset = 0;
    for src_frame in src_frames {
        unsafe { core::slice::from_raw_parts_mut(dst_buf_ptr.add(offset), src_frame.len()) }
            .copy_from_slice(src_frame);
        offset += src_frame.len();
    }

    Some(unsafe { dst.assume_init() })
}
// let src_frames = translated_byte_buffer(current_user_token(), src as *const u8, len);

pub fn trace_write<T>(token: usize, data: T, dst: usize) -> bool {
    let pt = PageTable::from_token(token);
    let Some(pte) = pt.translate(dst.into()) else {
        return false;
    };

    if !pte.is_valid() {
        return false;
    }

    if !pte.writable() {
        return false;
    }

    let src_buf_ptr: *const u8 = unsafe { core::mem::transmute(&data) };
    let len = core::mem::size_of::<T>();

    let dst_frames = translated_byte_buffer(current_user_token(), dst as *const u8, len);

    let mut offset = 0;
    for dst_frame in dst_frames {
        dst_frame.copy_from_slice(unsafe {
            core::slice::from_raw_parts(src_buf_ptr.add(offset), dst_frame.len())
        });
        offset += dst_frame.len();
    }

    true
}

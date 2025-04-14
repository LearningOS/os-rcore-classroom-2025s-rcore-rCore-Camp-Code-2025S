//! Process management syscalls
use crate::{
    mm::{translate_pte, translated_byte_buffer, VirtAddr},
    task::{
        change_program_brk, count_syscall, current_user_token, exit_current_and_run_next, mmap, munmap, suspend_current_and_run_next
    },
    timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    let timeval = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    let src = &timeval as *const TimeVal as *const u8;
    // ts是虚拟地址，需要拿到他的物理地址
    let dst = ts as *const u8;
    let buffers =
        translated_byte_buffer(current_user_token(), dst, core::mem::size_of_val(&timeval));
    for buffer in buffers {
        unsafe {
            buffer.copy_from_slice(core::slice::from_raw_parts(src, buffer.len()));
        }
    }
    0
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    match trace_request {
        // 读
        0 => {
            println!("id: {:?}", id);
            let pte = translate_pte(current_user_token(), id as *const u8);
            // 需要找到页表项
            if pte.is_none() {
                return -1;
            }
            let pte = pte.unwrap();
            // 需要可见，可读
            if !pte.is_valid() || !pte.readable() || !pte.user() {
                return -1;
            }
            let ppn = pte.ppn();
            ppn.get_bytes_array()[VirtAddr::from(id).page_offset()] as isize
        }
        // 写
        1 => {
            let pte = translate_pte(current_user_token(), id as *const u8);
            // 需要找到页表项
            if pte.is_none() {
                return -1;
            }
            let pte = pte.unwrap();
            // 需要可见，可读
            if !pte.is_valid() || !pte.writable() {
                return -1;
            }
            let ppn = pte.ppn();
            ppn.get_bytes_array()[VirtAddr::from(id).page_offset()] = data as u8;
            0
        }
        //
        2 => count_syscall(id),
        _ => -1,
    }
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    mmap(start, len, prot)
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
    munmap(start, len)
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

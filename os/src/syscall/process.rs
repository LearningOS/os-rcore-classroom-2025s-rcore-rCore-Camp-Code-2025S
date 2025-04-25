//! Process management syscalls

use crate::config::MEMORY_END;
use crate::task::{get_syscall_times, change_program_brk, exit_current_and_run_next, 
                    suspend_current_and_run_next, current_user_token, map_new_area, 
                    munmap_used_area};
use crate::timer::get_time_us;
use crate::mm::{copy_to_user, read_virtaddr, write_virtaddr, MapPermission};
use crate::mm::{VPNRange, VirtAddr, PageTable};

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
    let time_val  = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    let user_token = current_user_token(); // 获取当前任务的虚拟内存上下文
    let kernel_src = &time_val as *const TimeVal as *const u8;
    let len = core::mem::size_of::<TimeVal>();

    // 使用封装函数复制数据到用户态
    if copy_to_user(user_token, ts as *mut u8, kernel_src, len).is_ok() {
        0 // 返回成功
    } else {
        -1 // 返回失败
    }
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    if id > MEMORY_END {
        return  -1;
    }
    match trace_request {
        // _trace_request = 0, 获取当前任务id地址处一个字节的无符号整数值
        0 => {
            let addr = id as usize;
            let cur_token = current_user_token();
            match read_virtaddr(cur_token, addr) {
                Ok(val) => return val,
                Err(()) => return -1 as isize,
            };           
        },
        // _trace_request = 1, 将data写入到id对应地址处
        1 => {
            let addr = id as usize;
            let cur_token = current_user_token();
            match write_virtaddr(cur_token, addr, data as u8) {
                Ok(()) => 0 as isize,
                Err(()) => -1 as isize,
            }
        },
    // _trace_request = 2, 查询编号为id的系统调用的调用次数
        2 => {
            get_syscall_times(id) as isize
        },
        _ => -1, 
    }
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    let start_va = VirtAddr(start);
    if !start_va.aligned() {
        return -1;
    }
    if len <= 0 {
        return -1;
    }
    if (prot & !0x7 != 0) || (prot & 0x7 == 0) {
        return -1;
    }
    // len按页向上取整
    let start_va = VirtAddr(start).floor();
    let end_va = VirtAddr(start + len).ceil();
    let pgtb  = PageTable::from_token(current_user_token());

    // 检查虚拟地址是否已经被映射
    let vpns = VPNRange::new(start_va, end_va);
    for vpn in vpns {
        if let Some(pte) = pgtb.translate(vpn) {
            if pte.is_valid() {
                return -1;
            }
        }
    };

    // 构造permission: MapPermission
    let mut perm: MapPermission = MapPermission::U;
    if prot & 0x1 != 0 {
        perm |= MapPermission::R;
    }
    if prot & 0x2 != 0 {
        perm |= MapPermission::W;
    }
    if prot & 0x4 != 0 {
        perm |= MapPermission::X;
    }
    // 完成虚拟地址到物理地址的映射
    map_new_area(start_va.into(), end_va.into(), perm);
    0
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
    // 处理参数，获取需要取消映射的区间
    let start_va = VirtAddr(start);
    if !start_va.aligned() {
        return -1;
    }
    if len <= 0 {
        return -1;
    }
    let start_vpn = start_va.floor();
    let end_vpn = VirtAddr(start + len).ceil();
    // munmap_used_area的实现参考了荣誉准则标注的内容
    munmap_used_area(start_vpn.into(), end_vpn.into())
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

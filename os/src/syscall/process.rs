//! Process management syscalls
use alloc::sync::Arc;
use crate::{
    loader::get_app_data_by_name,
    //mm:: PageTable
    mm::{ translated_refmut, translated_str, MapPermission, VirtAddr },
    task::{
        add_task,
        current_task,
        current_user_token,
        exit_current_and_run_next,
        suspend_current_and_run_next,
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
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel:pid[{}] sys_yield", current_task().unwrap().pid.0);
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    trace!("kernel: sys_getpid pid:{}", current_task().unwrap().pid.0);
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
    trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork(); //复制任务 包括子空间
    let new_pid = new_task.pid.0; //子进程pid
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0; // 子进程的返回值设为 0 (a0 寄存器)
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path); //translated_str 找到要执行的应用名
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
        task.exec(data);
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    trace!("kernel::pid[{}] sys_waitpid [{}]", current_task().unwrap().pid.0, pid);
    let task = current_task().unwrap();
    // find a child process

    // ---- access current PCB exclusively
    let mut inner = task.inner_exclusive_access();
    //检查是否存在子进程
    if !inner.children.iter().any(|p| (pid == -1 || (pid as usize) == p.getpid())) {
        return -1;
        // ---- release current PCB
    }
    //获取 zombie 僵尸进程的pid
    let pair = inner.children
        .iter()
        .enumerate()
        .find(|(_, p)| {
            // ++++ temporarily access child PCB exclusively
            p.inner_exclusive_access().is_zombie() && (pid == -1 || (pid as usize) == p.getpid())
            // ++++ release child PCB
        });
    if let Some((idx, _)) = pair {
        //把僵尸进程从 child删掉
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        assert_eq!(Arc::strong_count(&child), 1); // 确保子进程资源会被回收。
        let found_pid = child.getpid();
        // ++++ temporarily access child PCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        // exit_code 写入用户空间的 exit_code_ptr
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    //ts 时间结构 time structure 这里是TimeVal
    //tz 时区 timezone 未使用
    trace!("kernel: sys_get_time");
    let time = get_time_us();
    *translated_refmut(current_user_token(), _ts) = TimeVal {
        sec: time / 1_000_000,
        usec: time % 1_000_000,
    };
    0
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    let start_va: VirtAddr = start.into();
    if !start_va.aligned() {
        return -1;
    }
    if (port & !0x7) != 0 || (port & 0x7) == 0 {
        return -1;
    }
    let end_va: VirtAddr = (start + len).into();
    let start_vpn = start_va.floor();
    let end_vpn = end_va.ceil();
    if let Some(task) = current_task() {
        if task.inner_exclusive_access().memory_set.is_overlap(start_vpn, end_vpn) {
            debug!("kernel: mmap: overlap");
            return -1;
        }
        task.inner_exclusive_access().memory_set.mmap(
            start_vpn,
            end_vpn,
            MapPermission::from_bits_truncate((port as u8) << 1) | MapPermission::U
        );
        0
    } else {
        -1
    }
}
// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    let start_va: VirtAddr = start.into();
    if !start_va.aligned() {
        return -1;
    }
    let start_vpn = start_va.floor();
    let end_va: VirtAddr = (start + len).into();
    let end_vpn = end_va.ceil();
    if let Some(task) = current_task() {
        task.inner_exclusive_access().memory_set.munmap(start_vpn, end_vpn)
    } else {
        -1
    }
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

/// YOUR JOB: Implement spawn.
/// HINT: fork + exec =/= spawn
pub fn sys_spawn(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_spawn NOT IMPLEMENTED", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let token = current_user_token();
    let path = translated_str(token, path);
    if path.is_empty() {
        //检查无效的文件名
        return -1;
    }
    if let Some(elf_data) = get_app_data_by_name(path.as_str()) {
        let new_task = current_task.spawn(elf_data);
        let new_pid = new_task.pid.0;
        add_task(new_task);
        new_pid as isize
    } else {
        -1
    }
}

// YOUR JOB: Set task priority.
pub fn sys_set_priority(_prio: isize) -> isize {
    trace!("kernel:pid[{}] sys_set_priority NOT IMPLEMENTED", current_task().unwrap().pid.0);
    -1
}

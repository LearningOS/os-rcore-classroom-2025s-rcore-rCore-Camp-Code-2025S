//! Task management implementation
//!
//! Everything about task management, like starting and switching tasks is
//! implemented here.
//!
//! A single global instance of [`TaskManager`] called `TASK_MANAGER` controls
//! all the tasks in the whole operating system.
//!
//! A single global instance of [`Processor`] called `PROCESSOR` monitors running
//! task(s) for each core.
//!
//! A single global instance of `PID_ALLOCATOR` allocates pid for user apps.
//!
//! Be careful when you see `__switch` ASM function in `switch.S`. Control flow around this function
//! might not be what you expect.
mod context;
mod id;
mod manager;
mod processor;
mod switch;
#[allow(clippy::module_inception)]
mod task;

//use crate::loader::{get_app_data, get_num_app };
//use crate::mm::{ MapPermission,/*  PageTable,*/ VirtAddr };
//use crate::sync::UPSafeCell;
//use alloc::vec::Vec;
//use crate::trap::TrapContext;
use alloc::sync::Arc;
use crate::loader::get_app_data_by_name;
use lazy_static::*;
pub use manager::{ fetch_task, TaskManager };
use switch::__switch;
pub use task::{ TaskControlBlock, TaskStatus };

pub use context::TaskContext;
pub use id::{ kstack_alloc, pid_alloc, KernelStack, PidHandle };
pub use manager::add_task;
pub use processor::{
    current_task,
    current_trap_cx,
    current_user_token,
    run_tasks,
    schedule,
    take_current_task,
    Processor,
};
/// Suspend the current 'Running' task and run the next task in task list.
pub fn suspend_current_and_run_next() {
    // There must be an application running.
    let task = take_current_task().unwrap(); //取出当前任务

    // ---- access current TCB exclusively
    let mut task_inner = task.inner_exclusive_access();
    let task_cx_ptr = &mut task_inner.task_cx as *mut TaskContext;
    // Change status to Ready 修改状态为ready
    task_inner.task_status = TaskStatus::Ready;
    drop(task_inner);
    // ---- release current PCB

    // push back to ready queue.
    add_task(task); //加入队列
    // jump to scheduling cycle
    schedule(task_cx_ptr);
}

/// pid of usertests app in make run TEST=1
pub const IDLE_PID: usize = 0;

/// Exit the current 'Running' task and run the next task in task list.
pub fn exit_current_and_run_next(exit_code: i32) {
    // take from Processor
    let task = take_current_task().unwrap();

    let pid = task.getpid();
    if pid == IDLE_PID {
        println!("[kernel] Idle process exit with exit_code {} ...", exit_code);
        panic!("All applications completed!");
    }

    // **** access current TCB exclusively
    let mut inner = task.inner_exclusive_access();
    // Change status to Zombie
    inner.task_status = TaskStatus::Zombie; //进程控制块中的状态修改为 僵尸进程 TaskStatus::Zombie
    // Record exit code 传入inner 的exit_code
    inner.exit_code = exit_code;
    // do not move to its parent but under initproc

    // ++++++ access initproc TCB exclusively
    {
        //吧所有的子进程挂在 initproc_inner下面  也就是子进程的父进程是init_proc init_proc的子进程是他们
        let mut initproc_inner = INITPROC.inner_exclusive_access();
        for child in inner.children.iter() {
            child.inner_exclusive_access().parent = Some(Arc::downgrade(&INITPROC));
            initproc_inner.children.push(child.clone());
        }
    }
    // ++++++ release parent PCB

    inner.children.clear(); //当前进程的孩子向量清空。
    // deallocate user space
    inner.memory_set.recycle_data_pages(); //前进程占用的资源进行早期回收 清空逻辑段area
    drop(inner);
    // **** release current PCB
    // drop task manually to maintain rc correctly
    drop(task);
    // we do not have to save task context
    // 因为不会回到该进程 调用schedule触发调度和任务切换
    let mut _unused = TaskContext::zero_init();
    schedule(&mut _unused as *mut _);
}

lazy_static! {
    /// Creation of initial process
    ///
    // the name "initproc" may be changed to any other app name like "usertests",
    /// but we have user_shell, so we don't need to change it.
    pub static ref INITPROC: Arc<TaskControlBlock> = Arc::new(
        TaskControlBlock::new(
            //get_app_data_by_name("initproc").unwrap()
            //初始化 name initproc
            get_app_data_by_name("ch5b_initproc").unwrap()
        )
    );
}
/// 增加 task
pub fn add_initproc() {
    //增加 task
    add_task(INITPROC.clone());
}
// 🔴🔴🔴🔴🔴🔴🔴🔴🔴🔴 在新的ch5 中ch4的内容被删除 mmap 和munmap的实现被直接移植到了syscall/process.rs

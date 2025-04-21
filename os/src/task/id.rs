//! Task pid implementation.
//!
//! Assign PID to the process here. At the same time, the position of the application KernelStack
//! is determined according to the PID.

use crate::config::{ KERNEL_STACK_SIZE, PAGE_SIZE, TRAMPOLINE };
use crate::mm::{ MapPermission, VirtAddr, KERNEL_SPACE };
use crate::sync::UPSafeCell;
use alloc::vec::Vec;
use lazy_static::*;

pub struct RecycleAllocator {
    current: usize,
    recycled: Vec<usize>,
}

impl RecycleAllocator {
    // 初始化
    pub fn new() -> Self {
        RecycleAllocator {
            current: 0,
            recycled: Vec::new(),
        }
    }
    // 分配pid
    pub fn alloc(&mut self) -> usize {
        // 如果有 使用回收的
        if let Some(id) = self.recycled.pop() {
            id
        } else {
            // 重新分配
            self.current += 1;
            self.current - 1
        }
    }
    // 回收pid
    pub fn dealloc(&mut self, id: usize) {
        // 首先断言 PID 必须是已分配的(小于 current)
        assert!(id < self.current);
        // 断言 PID 没有被重复回收
        assert!(!self.recycled.iter().any(|i| *i == id), "id {} has been deallocated!", id);
        self.recycled.push(id);
    }
}

lazy_static! {
    static ref PID_ALLOCATOR: UPSafeCell<RecycleAllocator> = unsafe {
        UPSafeCell::new(RecycleAllocator::new())
    };
    static ref KSTACK_ALLOCATOR: UPSafeCell<RecycleAllocator> = unsafe {
        UPSafeCell::new(RecycleAllocator::new())
    };
}

/// Abstract structure of PID
pub struct PidHandle(pub usize); //进程标识符

impl Drop for PidHandle {
    //自动资源回收
    fn drop(&mut self) {
        //println!("drop pid {}", self.0);
        PID_ALLOCATOR.exclusive_access().dealloc(self.0);
    }
}

/// Allocate a new PID
pub fn pid_alloc() -> PidHandle {
    // 全局函数调用alloc 分配pid
    PidHandle(PID_ALLOCATOR.exclusive_access().alloc())
}

/// Return (bottom, top) of a kernel stack in kernel space.
/// 内核栈位置计算
pub fn kernel_stack_position(app_id: usize) -> (usize, usize) {
    /*
    TRAMPOLINE 存放跳板代码 0xFFFFFFFFFFFFF000
    每个内核栈占用 KERNEL_STACK_SIZE + PAGE_SIZE 的空间
    Stack向下增长 top - KERNEL_STACK_SIZE 
    */
    let top = TRAMPOLINE - app_id * (KERNEL_STACK_SIZE + PAGE_SIZE);

    let bottom = top - KERNEL_STACK_SIZE;
    (bottom, top)
    // 返回(bottom, top),表示内核栈的地址范围
}

/// Kernel stack for a process(task)
//pub struct KernelStack(pub usize); //内核栈 KernelStack 中保存着它所属进程的 PID
pub struct KernelStack {
    pid: usize,
}
/// allocate a new kernel stack
pub fn kstack_alloc() -> KernelStack {
    let kstack_id = KSTACK_ALLOCATOR.exclusive_access().alloc();
    let (kstack_bottom, kstack_top) = kernel_stack_position(kstack_id);
    KERNEL_SPACE.exclusive_access().insert_framed_area(
        kstack_bottom.into(),
        kstack_top.into(),
        MapPermission::R | MapPermission::W
    );
    KernelStack { pid: kstack_id }
}

/* impl Drop for KernelStack {
    fn drop(&mut self) {
        let (kernel_stack_bottom, _) = kernel_stack_position(self.0);
        let kernel_stack_bottom_va: VirtAddr = kernel_stack_bottom.into();
        KERNEL_SPACE.exclusive_access().remove_area_with_start_vpn(kernel_stack_bottom_va.into());
        KSTACK_ALLOCATOR.exclusive_access().dealloc(self.0);
    }
} */
//新增的
impl Drop for KernelStack {
    fn drop(&mut self) {
        let (kernel_stack_bottom, _) = kernel_stack_position(self.pid);
        let kernel_stack_bottom_va: VirtAddr = kernel_stack_bottom.into();
        KERNEL_SPACE.exclusive_access().remove_area_with_start_vpn(kernel_stack_bottom_va.into());
    }
}

impl KernelStack {
    /// new一个KernelStack
    pub fn new(pid_handle: &PidHandle) -> Self {
        let pid = pid_handle.0;
        let (kernel_stack_bottom, kernel_stack_top) = kernel_stack_position(pid);
        KERNEL_SPACE.exclusive_access().insert_framed_area(
            // 将 [kernel_stack_bottom, kernel_stack_top) 映射到物理内存
            kernel_stack_bottom.into(),
            kernel_stack_top.into(),
            MapPermission::R | MapPermission::W
        );
        KernelStack { pid: pid_handle.0 }
    }
    /// Push a variable of type T into the top of the KernelStack and return its raw pointer
    #[allow(unused)]
    pub fn push_on_top<T>(&self, value: T) -> *mut T where T: Sized {
        let kernel_stack_top = self.get_top(); //获取当前栈顶地址
        let ptr_mut = (kernel_stack_top - core::mem::size_of::<T>()) as *mut T; //计算value 的存放位置
        unsafe {
            *ptr_mut = value; //写入数据
        }
        ptr_mut
    }
    /// Get the top of the KernelStack
    pub fn get_top(&self) -> usize {
        let (_, kernel_stack_top) = kernel_stack_position(self.pid);
        kernel_stack_top
    }
}

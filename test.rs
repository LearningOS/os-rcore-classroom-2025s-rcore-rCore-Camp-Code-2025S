pub fn sys_fork() -> isize {
    trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0; //子进程pid
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    trap_cx.x[10] = 0; // 子进程的返回值设为 0 (a0 寄存器)
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
        task.exec(data);
        0
    } else {
        -1
    }
}

pub fn sys_spawn(path: *const u8) -> isize {
    let token = current_user_token(); // 当前进程的页表token
    let path = translated_str(token, path); // 安全转换用户态路径字符串
    if path.is_empty() {
        return -1; // 错误：无效路径
    }

    // 从文件系统加载目标ELF文件
    if let Some(elf_data) = load_app_from_fs(&path) {
        let current_task = current_task().unwrap();
        let new_task = spawn_child(current_task, &elf_data); // 创建子进程
        new_task.pid.0 as isize // 返回子进程PID
    } else {
        -1 // 错误：文件不存在
    }
}

//⭕⭕⭕⭕⭕⭕⭕
//AI的写法
/// 基于父进程创建子进程并加载目标ELF
fn spawn_child(parent: &TaskControlBlock, elf_data: &[u8]) -> Arc<TaskControlBlock> {
    // 1. 创建新地址空间并加载ELF（类似exec逻辑）
    let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
    let trap_cx_ppn = memory_set.translate(VirtAddr::from(TRAP_CONTEXT_BASE).into()).unwrap().ppn();

    // 2. 分配PID和内核栈（与new()相同逻辑）
    let pid_handle = pid_alloc();
    let kernel_stack = kstack_alloc();
    let kernel_stack_top = kernel_stack.get_top();

    // 3. 构造子进程的TCB（继承父进程优先级等属性）
    let child_task = TaskControlBlock {
        pid: pid_handle,
        kernel_stack,
        inner: unsafe {
            UPSafeCell::new(TaskControlBlockInner {
                trap_cx_ppn,
                base_size: user_sp,
                task_cx: TaskContext::goto_trap_return(kernel_stack_top), // 初始化为跳转到trap_return
                task_status: TaskStatus::Ready,
                memory_set,
                parent: Some(Arc::downgrade(parent)), // 设置父进程指针
                children: Vec::new(),
                exit_code: 0,
                heap_bottom: user_sp, // 初始堆与用户栈相同
                program_brk: user_sp,
                priority: parent.inner_exclusive_access().priority, // 继承父进程优先级
                stride: 0,
            })
        },
    };

    // 4. 将子进程添加到父进程的children列表
    parent.inner_exclusive_access().children.push(Arc::clone(&child_task));

    // 5. 初始化子进程的Trap上下文（与exec相同逻辑）
    let trap_cx = child_task.inner_exclusive_access().get_trap_cx();
    *trap_cx = TrapContext::app_init_context(
        entry_point, // 用户程序入口点
        user_sp, // 用户栈顶
        KERNEL_SPACE.exclusive_access().token(),
        kernel_stack_top,
        trap_handler as usize
    );

    // 6. 设置子进程返回值（x10/a0寄存器为0，模拟fork行为）
    trap_cx.x[10] = 0;

    // 7. 加入调度队列
    add_task(Arc::clone(&child_task));
    child_task
}
//⭕⭕⭕⭕⭕⭕⭕

//➡️➡️➡️➡️➡️➡️
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

//➡️➡️➡️➡️➡️➡️➡️
pub fn exec(&self, elf_data: &[u8]) {
    // memory_set with elf program headers/trampoline/trap context/user stack
    // 生成一个全新的地址空间并直接替换进来
    // 原有地址空间生命周期结束，里面包含的全部物理页帧都会被回收
    let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
    let trap_cx_ppn = memory_set.translate(VirtAddr::from(TRAP_CONTEXT_BASE).into()).unwrap().ppn();

    // **** access current TCB exclusively
    // 更新数据状态
    let mut inner = self.inner_exclusive_access();
    // substitute memory_set
    inner.memory_set = memory_set;
    // update trap_cx ppn
    inner.trap_cx_ppn = trap_cx_ppn;
    // initialize base_size
    inner.base_size = user_sp;
    // initialize trap_cx
    // 修改新的地址空间中的 Trap 上下文
    let trap_cx = inner.get_trap_cx();
    *trap_cx = TrapContext::app_init_context(
        entry_point,
        user_sp,
        KERNEL_SPACE.exclusive_access().token(),
        self.kernel_stack.get_top(),
        trap_handler as usize
    );
    // **** release inner automatically
}

/// parent process fork the child process
pub fn fork(self: &Arc<Self>) -> Arc<Self> {
    // ---- access parent PCB exclusively
    let mut parent_inner = self.inner_exclusive_access();
    // copy user space(include trap context)
    // 地址空间通过复制父进程得到的
    let memory_set = MemorySet::from_existed_user(&parent_inner.memory_set);
    let trap_cx_ppn = memory_set.translate(VirtAddr::from(TRAP_CONTEXT_BASE).into()).unwrap().ppn();
    // alloc a pid and a kernel stack in kernel space
    let pid_handle = pid_alloc();
    let kernel_stack = kstack_alloc();
    let kernel_stack_top = kernel_stack.get_top();
    let task_control_block = Arc::new(TaskControlBlock {
        pid: pid_handle,
        kernel_stack,
        inner: unsafe {
            UPSafeCell::new(TaskControlBlockInner {
                trap_cx_ppn,
                base_size: parent_inner.base_size,
                task_cx: TaskContext::goto_trap_return(kernel_stack_top),
                task_status: TaskStatus::Ready,
                memory_set,
                parent: Some(Arc::downgrade(self)),
                children: Vec::new(),
                exit_code: 0,
                heap_bottom: parent_inner.heap_bottom,
                program_brk: parent_inner.program_brk,
                priority: 16,
                stride: 0,
            })
        },
    });
    // add child
    parent_inner.children.push(task_control_block.clone());
    // modify kernel_sp in trap_cx
    // **** access child PCB exclusively
    let trap_cx = task_control_block.inner_exclusive_access().get_trap_cx();
    trap_cx.kernel_sp = kernel_stack_top;
    // return
    task_control_block
    // **** release child PCB
    // ---- release parent PCB
}

pub fn new(elf_data: &[u8]) -> Self {
    let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);

    let trap_cx_ppn = memory_set.translate(VirtAddr::from(TRAP_CONTEXT_BASE).into()).unwrap().ppn();

    let pid_handle = pid_alloc(); // 分配 pid
    let kernel_stack = kstack_alloc(); //分配内核栈
    let kernel_stack_top = kernel_stack.get_top(); //获取栈顶地址
    let task_control_block = Self {
        pid: pid_handle,
        kernel_stack,
        inner: unsafe {
            UPSafeCell::new(TaskControlBlockInner {
                trap_cx_ppn, //trap_context物理页号码
                base_size: user_sp,
                task_cx: TaskContext::goto_trap_return(kernel_stack_top), //任务上下文
                task_status: TaskStatus::Ready,
                memory_set, //用户地址空间
                parent: None, // 父亲任务
                children: Vec::new(), // 子任务列表
                exit_code: 0, //退出代码
                heap_bottom: user_sp,
                program_brk: user_sp,
                priority: 16,
                stride: 0,
            })
        },
    };

    let trap_cx = task_control_block.inner_exclusive_access().get_trap_cx();
    *trap_cx = TrapContext::app_init_context(
        entry_point,
        user_sp,
        KERNEL_SPACE.exclusive_access().token(),
        kernel_stack_top,
        trap_handler as usize
    );
    task_control_block
}

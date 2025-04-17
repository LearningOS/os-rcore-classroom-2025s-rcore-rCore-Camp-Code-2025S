## 设计作业

为了实现 `sys_trace` 系统调用（ID 为 410），我们需要在系统中添加以下功能：

1. **定义系统调用**：在系统调用表中注册 `sys_trace`，将其编号设为 410，并实现函数 `sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize`。
2. **任务追踪结构**：为每个任务添加一个计数数组，记录其调用每个系统调用的次数。因为目前我们只有几个任务，所以我们需要建立一个映射，将系统调用ID映射到数组的索引上。
3. 功能实现
   - 若 `_trace_request = 0`，将 `_id` 视为指针，读取其地址处的一个字节并返回。
   - 若 `_trace_request = 1`，将 `_id` 视为指针，将 `_data` 的低 8 位写入该地址，返回 0。
   - 若 `_trace_request = 2`，查询当前任务调用编号为 `_id` 的系统调用的次数并返回（本次调用也计入）。
   - 其他情况返回 -1。
4. **修改调度器**：确保任务上下文保存追踪信息。
5.  初始化：在mod.rs中初始化每个任务的计数数组，然后给他设计两个方法`current_task_add(&self, syscall_id: usize)`表示系统调用ID计数+1,.`get_current_task_syscall_times(&self, syscall_id: usize)`表示返回系统调用次数。然后再pub两个函数用作`sys_trace`使用的接口。

#### 功能总结

实现了 `sys_trace` 系统调用，支持三种操作：读取任务内存字节（`trace_request=0`）、写入字节（`trace_request=1`）、查询系统调用次数（`trace_request=2`）。通过任务结构记录调用历史，无安全检查，直接操作内存，满足分时多任务环境下的测试需求。



## 简答作业

### 1. 进入 U 态后的特征及 bad 测例行为

sbi版本：[rustsbi] RustSBI version 0.3.0-alpha.2, adapting to RISC-V SBI v1.0.0

#### 1.1 ch2b_bad_address

这个用例中，`(0x0 as *mut u8).write_volatile(0)` 尝试向地址 0x00*x*0 写入值 00。在 U 态，若 0x00*x*0 未映射（通常如此），会触发存储访问异常（Store/AMO Access Fault）

#### 1.2 ch2b_bad_instructions

这个用例中，`core::arch::asm!("sret")` 使用内联汇编执行 `sret` 指令。而`sret` 是 RISC-V 的特权指令，用于从 S 态（监督者模式）返回到较低特权模式（通常是 U 态），并更新特权状态。是非法的。

#### 1.3 ch2b_bad_register

这个用例中，`csrr` 是 RISC-V 的特权指令，用于读取控制状态寄存器（CSR），这里尝试读取 `sstatus`（监督者状态寄存器）。而`sstatus` 是 S 态寄存器，U 态无权访问。是非法的。

### 2.深入理解 [trap.S](https://github.com/LearningOS/rCore-Tutorial-Code-2025S/blob/ch3/os/src/trap/trap.S) 中两个函数 `__alltraps` 和 `__restore` 的作用，并回答如下问题:

1. L40：刚进入 `__restore` 时，`sp` 代表了什么值。请指出 `__restore` 的两种使用情景。

   答： `sp` 代表内核栈上保存的 `TrapContext` 的基地址，当发生trap时候，有

   ```
   # now sp->kernel stack, sscratch->user stack
   # allocate a TrapContext on kernel stack
   ```

   代表在内核栈上分配了 34×8=27234×8=272 字节的空间，用于保存`TrapContext`，其 包含通用寄存器（x1, x3, x5-x31）、`sstatus`、`sepc` 和 `sscratch`（用户栈指针）的值。调用 `trap_handler` 处理完陷阱后，控制权转移到 `__restore`，此时 `sp` 仍指向内核栈上 `TrapContext` 的起始地址，即分配后的最低地址(栈是从高到低生长的)。

2. L43-L48：这几行汇编代码特殊处理了哪些寄存器？这些寄存器的的值对于进入用户态有何意义？请分别解释。

   ```
   ld t0, 32*8(sp)
   ld t1, 33*8(sp)
   ld t2, 2*8(sp)
   csrw sstatus, t0
   csrw sepc, t1
   csrw sscratch, t2
   ```

   将栈上的偏移：处理了`sstatus` ,`sepc`,`sscratch`这些寄存器： `32*8(sp)` ` 33*8(sp)` `2*8(sp)` 加载到上诉寄存器，恢复原来的值，作用：

   * `sstatus` 确保处理器以用户态的权限和配置运行，避免特权模式错误或中断行为异常。
   * `sret` 指令会将 `sepc` 的值加载到程序计数器（PC），决定用户态恢复执行的起点，确保用户程序从正确位置继续执行，避免跳跃到错误地址。
   * `sscratch` 是一个临时存储寄存器，在后续会将其值交换回`sp`，确保用户态程序使用正确的栈继续执行（如函数调用、局部变量访问），否则会导致栈混乱或内存访问错误。

3. L50-L56：为何跳过了 `x2` 和 `x4`？

   ```
   ld x1, 1*8(sp)
   ld x3, 3*8(sp)
   .set n, 5
   .rept 27
      LOAD_GP %n
      .set n, n+1
   .endr
   ```

   `x2` 是栈指针寄存器，在用户态和内核态都有重要作用。在 `__restore` 函数中，`sp` 的值需要通过后续的 `csrrw sp, sscratch, sp`从 `sscratch` 恢复为用户栈地址，而不是直接从内核栈的 `TrapContext` 中加载。

   `x4` 是线程指针寄存器，通常由应用程序或操作系统用于存储线程特定的数据（如线程控制块的地址）。但在代码中，提到`skip tp(x4), application does not use it`说明，没有用上，所以不需要保存和恢复。

4. L60：该指令之后，`sp` 和 `sscratch` 中的值分别有什么意义？

   ```
   csrrw sp, sscratch, sp
   ```

    sp->user stack, sscratch->kernel stack

5. `__restore`：中发生状态切换在哪一条指令？为何该指令执行之后会进入用户态？

   发生在  `sret` 上：执行 `sret` 时，硬件会：

   * 将特权模式设置为 `SPP` 的值（这里是 0，即 U 态）。
   * 将程序计数器 pc 设置为 `sepc`（Supervisor Exception Program Counter）中的值，即用户程序被中断时的下一条指令地址。
   * 更新 `sstatus` 中的中断使能位（SIE 和 SPIE 的切换），恢复用户态的中断状态。

6. L13：该指令之后，`sp` 和 `sscratch` 中的值分别有什么意义？

   ```
   csrrw sp, sscratch, sp
   ```

    sp->kernel stack, sscratch->user stack

7. 从 U 态进入 S 态是哪一条指令发生的？

​	发生在`ecall` 上

1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 **以下各位** 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

   > *无*

2. 此外，我也参考了 **以下资料** ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

   > *https://learningos.cn/rCore-Tutorial-Guide-2025S/chapter3/5exercise.html#id5*

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。
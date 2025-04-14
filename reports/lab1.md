# Lab1实验报告

## 简述功能
目标实现sys_trace的三个功能：  
1. 读取当前任务的指定地址数据
2. 写入当前任务的指定地址数据
3. 统计任务的系统调用次数，提供查询

如何实现：  
1. 直接返回给地址解索引后值
2. 直接赋值给地址
3. 修改TaskManager，增加records字段，类型为TaskRecord数组，长度同tasks，用来记录每个任务syscall次数  
TaskRecord就简单为Vec的(syscall_id, times)
提供修改，查询两个方法，  
修改调用链为  
syscall::syscall -> task::record_syscall -> TaskManager::record_syscall -> TaskRecord::record_syscall
每次修改不存在syscall，添加Vec元素，有就+1  
查询基本雷同

## 问答题
1. SBI版本：RustSBI-QEMU Version 0.2.0-alpha.2
    - ch2b_bad_address：触发Trap::Exception(Exception::StoreFault)
    - ch2b_bad_instructions：触发Trap::Exception(Exception::IllegalInstruction)
    - ch2b_bad_register: 同上，触发Trap::Exception(Exception::IllegalInstruction)
2. 对于trap.S中函数理解
    1. 刚进入__restore，sp代表kernel stack
        情景1：kernel初始化完，需要进入用户态执行  
        情景2：由于Trap后陷入内核态，处理完trap需要回到用户态执行用户代码
    2. 特殊处理了
        - sstatus，这个记录在此次Trap之前CPU处于哪个特权级，用来返回哪个态
        - sepc，这个记录此次Trap前，CPU下一条指令地址，用来继续执行之前中断的代码
        - sscratch，保存了内核态或用户态栈，__restore进入时，这个被初始化为用户栈，跑完最后保存内核栈，留给下次__alltraps时用
    3. x2已经保存在sscratch中了，不用再处理，x4寄存器线程指针，也不用处理
    4. sp 用户栈，sscratch 内核栈，并且释放了当前的Trap Context
    5. sret，会设置pc为sepc即用户态的下条指令地址，sstaus SPP为用户态，并且可中段，即完成了用户态的跳转
    6. 此指令之前, sp用户栈，sscratch内核栈，此指令之后，sp内核栈，sscratch用户栈
    7. 用户代码中，通过ecall指令发生Trap进入内核态

## 荣誉准则
1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 以下各位 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：  
无
2. 此外，我也参考了 以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：  
无
3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。
4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。

## 建议
无
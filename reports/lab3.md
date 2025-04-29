# 总结

这次实验需要对上个实验的两个内存系统调用函数进行维护，适应这次实验的修改。本次主要实现了系统调用spawn，在传统的fork+exec模式中，fork会完全复制父进程的内存空间（即使现代系统使用写时复制优化），但如果立即执行exec，这个复制操作变得多余，因为exec会立即替换掉整个内存空间。spawn可以避免这种不必要的内存复制。最后还是实现了带优先级的调度stride，设置优先级。

# 问答题

一、
在 p1.stride = 255, p2.stride = 250 的情况下，若 p2 执行一个时间片后，它的 stride 会变成 250 + 10 = 260。但由于使用 8bit 无符号整形存储，260 实际存储为 260 % 256 = 4。此时比较 p1.stride = 255 和 p2.stride = 4，普通比较会认为 4 < 255，所以会选 p2 执行，而不是 p1。这违背了 stride 调度的本意，因为实际上 p1 的步长更小。

二、当所有进程优先级 >= 2 时，每个进程的步进值 stride_step <= BigStride / 2。假设在某一时刻，进程 A 的 stride 值最小，它被调度执行后，其 stride 增加了 stride_step，但这个增加量不会超过 BigStride / 2。这意味着即使执行了 A，它的新 stride 值也不可能超过原先最大 stride 值太多。因此，任意两个进程的 stride 差值不会超过 BigStride / 2。

三、

```
fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.0.abs_diff(other.0) <= u64::MAX / 2 {
            // 正常情况下的比较
            Some(self.0.cmp(&other.0))
        } else {
            // 溢出情况下，反转比较结果
            Some(other.0.cmp(&self.0))
        }
    }
```
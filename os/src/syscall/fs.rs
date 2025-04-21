//! File and filesystem-related syscalls
use crate::mm::translated_byte_buffer;
use crate::sbi::console_getchar;
use crate::task::{current_task, current_user_token, suspend_current_and_run_next};

const FD_STDIN: usize = 0;
const FD_STDOUT: usize = 1;

/// write buf of length `len`  to a file with `fd`
pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_write", current_task().unwrap().pid.0);
    match fd {
        FD_STDOUT => {
            // 将用户空间缓冲区转换为内核可访问的形式
            let buffers = translated_byte_buffer(current_user_token(), buf, len);
            for buffer in buffers {
                //处理切片
                print!("{}", core::str::from_utf8(buffer).unwrap());
            }
            len as isize
        }
        _ => {
            panic!("Unsupported fd in sys_write!");
        }
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_read", current_task().unwrap().pid.0);
    match fd {
        FD_STDIN => {
            assert_eq!(len, 1, "Only support len = 1 in sys_read!");
            let mut c: usize;
            loop {
                c = console_getchar(); // 从SBI 获取字符 且每次只能读入一个字符
                if c == 0 {
                    suspend_current_and_run_next(); // 无输入 让出cpu
                    continue;
                } else {
                    break;
                }
            }
            let ch = c as u8;
            let mut buffers = translated_byte_buffer(current_user_token(), buf, len); //获取当前用户的token 将用户空间的 buf 指针转换为内核可安全访问的缓冲区（通过页表翻译）
            unsafe {
                buffers[0].as_mut_ptr().write_volatile(ch);
            }
            1
        }
        _ => {
            panic!("Unsupported fd in sys_read!");
        }
    }
}

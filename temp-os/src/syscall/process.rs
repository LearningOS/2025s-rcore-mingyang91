//! Process management syscalls
use core::borrow::Borrow;

use crate::{
    config::MAX_SYSCALL_NUM,
    mm::{trace_read, trace_write, translated_byte_buffer, MapPermission},
    task::{
        change_program_brk, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next, TaskControlBlock, TaskStatus, TASK_MANAGER,
    },
    timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// Task information
#[allow(dead_code)]
pub struct TaskInfo {
    /// Task status in it's life cycle
    status: TaskStatus,
    /// The numbers of syscall called by task
    syscall_times: [u32; MAX_SYSCALL_NUM],
    /// Total running time of task
    time: usize,
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

fn copy_to_virt<T>(src: &T, dst: *mut T) {
    let src_buf_ptr: *const u8 = unsafe { core::mem::transmute(src) };
    let dst_buf_ptr: *const u8 = unsafe { core::mem::transmute(dst) };
    let len = core::mem::size_of::<T>();

    let dst_frames = translated_byte_buffer(current_user_token(), dst_buf_ptr, len);

    let mut offset = 0;
    for dst_frame in dst_frames {
        dst_frame.copy_from_slice(unsafe {
            core::slice::from_raw_parts(src_buf_ptr.add(offset), dst_frame.len())
        });
        offset += dst_frame.len();
    }
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");

    let now = get_time_us();
    let time_val = TimeVal {
        sec: now / 1_000_000,
        usec: now % 1_000_000,
    };

    copy_to_virt(&time_val, ts);
    0
}

/// 获取任务信息
/// 在 ch3 中，我们的系统已经能够支持多个任务分时轮流运行，我们希望引入一个新的系统调用 ``sys_trace``（ID 为 410）用来追踪当前任务系统调用的历史信息，并做对应的修改。定义如下。
///
/// fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize
/// 调用规范：
/// 这个系统调用有三种功能，根据 trace_request 的值不同，执行不同的操作：
///
/// 如果 trace_request 为 0，则 id 应被视作 *const u8 ，表示读取当前任务 id 地址处一个字节的无符号整数值。此时应忽略 data 参数。返回值为 id 地址处的值。
///
/// 如果 trace_request 为 1，则 id 应被视作 *const u8 ，表示写入 data （作为 u8，即只考虑最低位的一个字节）到该用户程序 id 地址处。返回值应为0。
///
/// 如果 trace_request 为 2，表示查询当前任务调用编号为 id 的系统调用的次数，返回值为这个调用次数。本次调用也计入统计 。
///
/// 否则，忽略其他参数，返回值为 -1。
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");

    match trace_request {
        0 => {
            let data_ptr = id as *const u8;
            let data = trace_read::<u8>(current_user_token(), data_ptr as usize);
            if let Some(has) = data {
                has as isize
            } else {
                -1
            }
        }
        1 => {
            let data_ptr = id as *mut u8;
            if trace_write(current_user_token(), data as u8, data_ptr as usize) {
                0
            } else {
                -1
            }
        }
        2 => {
            let task = TASK_MANAGER.current_task();
            let info: &TaskControlBlock = task.borrow();

            info.syscall_times[id] as isize
        }
        _ => {
            trace!("Unsupported trace request: {}", trace_request);
            -1
        }
    }
}

bitflags! {
    pub struct MmapProt: usize {
        const PROT_NONE = 0;
        const PROT_READ = 1;
        const PROT_WRITE = 2;
        const PROT_EXEC = 4;
    }
}

impl From<MmapProt> for MapPermission {
    fn from(prot: MmapProt) -> Self {
        let mut permission = MapPermission::empty();
        if prot.contains(MmapProt::PROT_READ) {
            permission |= MapPermission::R;
        }
        if prot.contains(MmapProt::PROT_WRITE) {
            permission |= MapPermission::W;
        }
        if prot.contains(MmapProt::PROT_EXEC) {
            permission |= MapPermission::X;
        }
        permission
    }
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    debug!(
        "kernel: sys_mmap start: {:#x}, len: {:#x}, prot: {:#x}",
        start, len, prot
    );
    let Some(prot) = MmapProt::from_bits(prot) else {
        return -1;
    };

    if prot == MmapProt::PROT_NONE {
        return -1;
    }

    if start % 4096 != 0 {
        return -1;
    }

    if let Err(msg) = TASK_MANAGER.mmap(start.into(), (start + len).into(), prot.into()) {
        info!("kernel: sys_mmap failed: {}", msg);
        return -1;
    }
    0
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    debug!("kernel: sys_munmap start: {:#x}, len: {:#x}", start, len);

    if start % 4096 != 0 || len % 4096 != 0 {
        return -1;
    }

    if let Err(msg) = TASK_MANAGER.unmap(start.into(), (start + len).into()) {
        info!("kernel: sys_munmap failed: {}", msg);
        return -1;
    }
    0
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

//! Process management syscalls
use crate::{
    config::MAX_SYSCALL_NUM,
    task::{exit_current_and_run_next, suspend_current_and_run_next, TaskStatus, TASK_MANAGER},
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
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// get time with second and microsecond
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    unsafe {
        *ts = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        };
    }
    0
}

/// YOUR JOB: Finish sys_task_info to pass testcases
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
    trace!("kernel: sys_task_info");

    match trace_request {
        0 => {
            let data_ptr = id as *const u8;
            unsafe { *data_ptr as isize }
        }
        1 => {
            let data_ptr = id as *mut u8;
            unsafe { *data_ptr = data as u8 };
            0
        }
        2 => {
            let task = TASK_MANAGER.current_task();

            let info = task.info();

            info.syscall_times[id] as isize
        }
        _ => {
            trace!("Unsupported trace request: {}", trace_request);
            -1
        }
    }
}

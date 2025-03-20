//! Process management syscalls
use alloc::{sync::Arc, task};

use crate::{
    config::MAX_SYSCALL_NUM,
    loader::get_app_data_by_name,
    mm::{translated_byte_buffer, translated_refmut, translated_str, MapPermission},
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next, suspend_current_and_run_next, TaskControlBlock, TaskStatus
    },
};

use crate::timer::{get_time_ms, get_time_us};


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
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
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
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child PCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
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

/// YOUR JOB: Finish sys_task_info to pass testcases
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TaskInfo`] is splitted by two pages ?
pub fn sys_task_info(ti: *mut TaskInfo) -> isize {
    trace!("kernel: sys_task_info");
    let now = get_time_ms();
    let new = {
        let task = current_task()
            .expect("sys_task_info: current_task failed");
        let inner = task.inner_exclusive_access();
        TaskInfo {
            status: task.status(),
            time: now - inner.start_time,
            syscall_times: inner.syscall_times,
        }
    };
    copy_to_virt(&new, ti);
    0
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

/// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!("kernel: sys_mmap start: {:#x}, len: {:#x}, prot: {:#x}", start, len, prot);
    let Some(prot) = MmapProt::from_bits(prot) else {
        return -1;
    };

    if prot == MmapProt::PROT_NONE {
        return -1;
    }

    if start % 4096 != 0 {
        return -1;
    }

    let current = current_task().expect("sys_mmap: current_task failed");
    let memory_set = &mut current
        .inner_exclusive_access()
        .memory_set;
    if let Err(msg) = memory_set
        .mmap(
            start.into(),
            (start + len).into(),
            prot.into()
        ) {
            info!("kernel: sys_mmap failed: {}", msg);
            return -1;
        }
    0
}

/// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    debug!("kernel: sys_munmap start: {:#x}, len: {:#x}", start, len);

    if start % 4096 != 0 || len % 4096 != 0 {
        return -1;
    }

    let current = current_task().expect("sys_mmap: current_task failed");
    let memory_set = &mut current
        .inner_exclusive_access()
        .memory_set;
    if let Err(msg) = memory_set
        .unmap(
            start.into(),
            (start + len).into(),
        ) {
            info!("kernel: sys_munmap failed: {}", msg);
            return -1;
        }
    0
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
    let token = current_user_token();
    let path = translated_str(token, path);
    debug!("kernel: sys_spawn user: {}, path: {}", token, path);

    let Some(elf_data) = get_app_data_by_name(path.as_str()) else {
        debug!("kernel: sys_spawn failed: app [{}] not found", path);
        return -1;
    };

    let task_control_block = Arc::new(TaskControlBlock::new(&elf_data));
    add_task(task_control_block.clone());
    let Some(current) = current_task() else {
        return -1;
    };

    {
        current.inner_exclusive_access().children.push(task_control_block.clone());
    }
    let pid = task_control_block.getpid();
    debug!("kernel: sys_spawn pid: {}, current: {}", pid, current.pid.0);
    pid as isize  
}

// YOUR JOB: Set task priority.
pub fn sys_set_priority(prio: isize) -> isize {
    let current = current_task().expect("sys_set_priority: current_task failed");
    debug!("kernel:pid[{}] sys_set_priority prio: {}", current.pid.0, prio);

    if prio <= 1 {
        return -1;
    }

    let mut inner = current.inner_exclusive_access();
    inner.priority = prio as usize;
    prio
}

fn copy_to_virt<T>(src: &T, dst: *mut T) {
    let src_buf_ptr: *const u8 = unsafe { core::mem::transmute(src) };
    let dst_buf_ptr: *const u8 = unsafe { core::mem::transmute(dst) };
    let len = core::mem::size_of::<T>();

    let dst_frames = translated_byte_buffer(
        current_user_token(),
        dst_buf_ptr,
        len
    );

    let mut offset = 0;
    for dst_frame in dst_frames {
        dst_frame.copy_from_slice(unsafe {
            core::slice::from_raw_parts(src_buf_ptr.add(offset), dst_frame.len())
        });
        offset += dst_frame.len();
    }
}

//! Types related to task management

use crate::{config::MAX_SYSCALL_NUM, timer::get_time_ms};

use super::TaskContext;

/// The task control block (TCB) of a task.
#[derive(Copy, Clone)]
pub struct TaskControlBlock {
    /// The status of the task
    pub status: TaskStatus,
    /// The task information
    pub info: TaskInfo,
}

impl TaskControlBlock {
    /// Turn the task into `TaskControlBlock::Running`
    pub fn try_turn_to_running(&mut self) -> Result<(), &'static str> {
        if self.status != TaskStatus::Ready {
            return Err("TaskControlBlock::turn_to_running: not a Ready task");
        }

        if self.info.start_time == 0 {
            self.info.start_time = get_time_ms();
        }
        self.status = TaskStatus::Running;
        Ok(())
    }

    /// Turn the task into `TaskControlBlock::Ready`
    pub fn try_turn_to_ready(&mut self) -> Result<(), &'static str> {
        if self.status != TaskStatus::Running {
            return Err("TaskControlBlock::turn_to_ready: not a Running task");
        }

        self.status = TaskStatus::Ready;
        Ok(())
    }

    /// Turn the task into `TaskControlBlock::Exited`
    pub fn turn_to_exited(&mut self) {
        self.status = TaskStatus::Exited;
    }

    /// Get the task context
    pub fn cx(&self) -> &TaskContext {
        &self.info.task_cx
    }

    /// Check if the task is ready
    pub fn is_ready(&self) -> bool {
       self.status == TaskStatus::Ready
    }

    /// Get the status of the task
    pub fn status(&self) -> TaskStatus {
        self.status
    }

    /// Get the task information
    pub fn info(&self) -> &TaskInfo {
        &self.info
    }

    /// Increase the syscall times
    pub fn sys_call_inc(&mut self, syscall_id: usize) {
        self.info.sys_call_inc(syscall_id);
    }
}

#[derive(Copy, Clone, Debug)]
pub struct TaskInfo {
    /// The task context
    pub task_cx: TaskContext,
    /// The start time of the task
    pub start_time: usize,
    /// The number of syscalls called by the task
    pub syscall_times: [u32; MAX_SYSCALL_NUM],
}

impl TaskInfo {
    pub fn new(task_cx: TaskContext, start_time: usize) -> Self {
        Self {
            task_cx,
            start_time,
            syscall_times: [0; MAX_SYSCALL_NUM],
        }
    }

    /// increase the syscall times
    pub fn sys_call_inc(&mut self, syscall_id: usize) {
        self.syscall_times[syscall_id] += 1;
    }
}

/// The status of a task
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum TaskStatus {
    /// The task is uninitialized
    UnInit,
    /// The task is ready to run
    Ready,
    /// The task is running
    Running,
    /// The task has exited
    Exited,
}

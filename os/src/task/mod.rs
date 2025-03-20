//! Task management implementation
//!
//! Everything about task management, like starting and switching tasks is
//! implemented here.
//!
//! A single global instance of [`TaskManager`] called `TASK_MANAGER` controls
//! all the tasks in the operating system.
//!
//! Be careful when you see `__switch` ASM function in `switch.S`. Control flow around this function
//! might not be what you expect.

mod context;
mod switch;
#[allow(clippy::module_inception)]
mod task;


use core::array;

use crate::config::MAX_APP_NUM;
use crate::loader::{get_num_app, init_app_cx};
use crate::sync::UPSafeCell;
use lazy_static::*;
use switch::__switch;
use task::TaskInfo;
pub use task::{TaskControlBlock, TaskStatus};

pub use context::TaskContext;

/// The task manager, where all the tasks are managed.
///
/// Functions implemented on `TaskManager` deals with all task state transitions
/// and task context switching. For convenience, you can find wrappers around it
/// in the module level.
///
/// Most of `TaskManager` are hidden behind the field `inner`, to defer
/// borrowing checks to runtime. You can see examples on how to use `inner` in
/// existing functions on `TaskManager`.
pub struct TaskManager {
    /// total number of tasks
    num_app: usize,
    /// use inner value to get mutable access
    inner: UPSafeCell<TaskManagerInner>,
}

/// Inner of Task Manager
pub struct TaskManagerInner {
    /// task list
    tasks: [TaskControlBlock; MAX_APP_NUM],
    /// id of current `Running` task
    current_task: usize,
}

lazy_static! {
    /// Global variable: TASK_MANAGER
    pub static ref TASK_MANAGER: TaskManager = {
        let num_app = get_num_app();
        let tasks: [_; MAX_APP_NUM] = array::from_fn(|i| {
            TaskControlBlock {
                status: TaskStatus::Ready,
                info: TaskInfo::new(
                    TaskContext::goto_restore(init_app_cx(i)),
                    0, // start time, will be updated when task first runs
                )
            }
        });
        TaskManager {
            num_app,
            inner: unsafe {
                UPSafeCell::new(TaskManagerInner {
                    tasks,
                    current_task: 0,
                })
            },
        }
    };
}

impl TaskManager {
    /// Run the first task in task list.
    ///
    /// Generally, the first task in task list is an idle task (we call it zero process later).
    /// But in ch3, we load apps statically, so the first task is a real app.
    fn run_first_task(&self) -> ! {
        let next_task_cx_ptr = {
            let mut inner = self.inner.exclusive_access();
            let task0 = &mut inner.tasks[0];
            task0.try_turn_to_running().expect("First task must be ready");
            
            task0.cx() as *const TaskContext
        };
        let mut _unused = TaskContext::zero_init();
        // before this, we should drop local variables that must be dropped manually
        unsafe {
            __switch(&mut _unused as *mut TaskContext, next_task_cx_ptr);
        }
        panic!("unreachable in run_first_task!");
    }

    /// Change the status of current `Running` task into `Ready`.
    fn mark_current_suspended(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        if let Err(_) = inner.tasks[current].try_turn_to_ready() {
            trace!("Task {} is not running: {:?}", current, inner.tasks[current].status());
        }
    }

    /// Change the status of current `Running` task into `Exited`.
    fn mark_current_exited(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].turn_to_exited();
    }

    /// Find next task to run and return task id.
    ///
    /// In this case, we only return the first `Ready` task in task list.
    fn find_next_task(&self) -> Option<usize> {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        (current + 1..current + self.num_app + 1)
            .map(|id| id % self.num_app)
            .find(|id| inner.tasks[*id].is_ready())
    }

    /// Switch current `Running` task to the task we have found,
    /// or there is no `Ready` task and we can exit with all applications completed
    fn run_next_task(&self) {
        let Some(next) = self.find_next_task() else {
            panic!("All applications completed!");
        };

        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        if let Err(msg) = inner.tasks[current].try_turn_to_ready() {
            trace!("Task {} is not running: {}", current, msg);
        };
        if let Err(msg) = inner.tasks[next].try_turn_to_running() {
            trace!("Task {} is not ready: {}", next, msg);
        };

        let current_task_cx_ptr = inner.tasks[current].cx() as *const _ as *mut _;
        let next_task_cx_ptr = inner.tasks[next].cx() as *const _ as *mut _;
        inner.current_task = next;
        drop(inner);
        // before this, we should drop local variables that must be dropped manually
        unsafe {
            __switch(current_task_cx_ptr, next_task_cx_ptr);
        }
        // go back to user mode
    }

    pub(crate) fn current_task(&self) -> TaskControlBlock {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task]
    }

    pub(crate) fn sys_call_inc(&self, syscall_id: usize) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].sys_call_inc(syscall_id);
    }
}

/// Run the first task in task list.
pub fn run_first_task() {
    TASK_MANAGER.run_first_task();
}

/// Get the current task id.
pub fn current_task_id() -> usize {
    TASK_MANAGER.inner.exclusive_access().current_task
}

/// Switch current `Running` task to the task we have found,
/// or there is no `Ready` task and we can exit with all applications completed
fn run_next_task() {
    TASK_MANAGER.run_next_task();
}

/// Change the status of current `Running` task into `Ready`.
fn mark_current_suspended() {
    TASK_MANAGER.mark_current_suspended();
}

/// Change the status of current `Running` task into `Exited`.
fn mark_current_exited() {
    TASK_MANAGER.mark_current_exited();
}

/// Suspend the current 'Running' task and run the next task in task list.
pub fn suspend_current_and_run_next() {
    mark_current_suspended();
    run_next_task();
}

/// Exit the current 'Running' task and run the next task in task list.
pub fn exit_current_and_run_next() {
    mark_current_exited();
    run_next_task();
}

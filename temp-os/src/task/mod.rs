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

use core::borrow::{Borrow, BorrowMut};
use core::cell::RefMut;
use core::ptr::NonNull;

use crate::loader::{get_app_data, get_num_app};
use crate::mm::{MapPermission, VirtAddr};

use crate::sync::UPSafeCell;
use crate::trap::TrapContext;
use alloc::vec::Vec;
use lazy_static::*;
use switch::__switch;
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

/// The task manager inner in 'UPSafeCell'
struct TaskManagerInner {
    /// task list
    tasks: Vec<TaskControlBlock>,
    /// id of current `Running` task
    current_task: usize,
}

lazy_static! {
    /// a `TaskManager` global instance through lazy_static!
    pub static ref TASK_MANAGER: TaskManager = {
        println!("init TASK_MANAGER");
        let num_app = get_num_app();
        let tasks = (0..num_app)
            .map(|i| TaskControlBlock::new(get_app_data(i), i))
            .collect();
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
    /// But in ch4, we load apps statically, so the first task is a real app.
    fn run_first_task(&self) -> ! {
        let next_task_cx_ptr = {
            let mut inner = self.inner.exclusive_access();
            let task0 = &mut inner.tasks[0];
            task0
                .try_turn_to_running()
                .expect("First task must be ready");

            task0.cx() as *const TaskContext
        };
        let mut _unused = TaskContext::zero_init();
        // before this, we should drop local variables that must be dropped manually
        unsafe {
            __switch(&mut _unused as *mut _, next_task_cx_ptr);
        }
        panic!("unreachable in run_first_task!");
    }

    /// Change the status of current `Running` task into `Ready`.
    fn mark_current_suspended(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        if let Err(_) = inner.tasks[current].try_turn_to_ready() {
            trace!(
                "Task {} is not running: {:?}",
                current,
                inner.tasks[current].status()
            );
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

    /// Get the current 'Running' task's token.
    fn get_current_token(&self) -> usize {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_user_token()
    }

    /// Get the current 'Running' task's trap contexts.
    fn get_current_trap_cx(&self) -> &'static mut TrapContext {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_trap_cx()
    }

    /// Change the current 'Running' task's program break
    pub fn change_current_program_brk(&self, size: i32) -> Option<usize> {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].change_program_brk(size)
    }

    /// Switch current `Running` task to the task we have found,
    /// or there is no `Ready` task and we can exit with all applications completed
    fn run_next_task(&self) {
        let Some(next) = self.find_next_task() else {
            panic!("All applications completed!");
        };

        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        // if let Err(msg) = inner.tasks[current].try_turn_to_ready() {
        //     trace!("Task {} is not running: {}", current, msg);
        // };
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

    pub(crate) fn current_task(&self) -> Shared<'_, TaskControlBlock> {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let mut ret = Shared {
            owner: inner,
            value: NonNull::dangling(),
        };

        ret.value = NonNull::from(&mut ret.owner.tasks[current]);
        ret
    }

    /// Map a new memory area for current task.
    pub fn mmap(
        &self,
        start_va: VirtAddr,
        end_va: VirtAddr,
        permission: MapPermission,
    ) -> Result<(), &str> {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        let current_memory_set = &mut inner.tasks[cur].memory_set;

        let mut next_vpn = start_va.floor();
        let end_vpn = end_va.ceil();

        while next_vpn < end_vpn {
            if let Some(pte) = current_memory_set.translate(next_vpn) {
                if pte.is_valid() {
                    debug!("mmap: area already mapped: {:x?}", pte.ppn());
                    return Err("mmap: area already mapped");
                }
            }
            next_vpn.0 += 1;
        }
        current_memory_set.insert_framed_area(start_va, end_va, permission | MapPermission::U);
        Ok(())
    }

    /// Unmap a memory area for current task.
    pub fn unmap(&self, start_va: VirtAddr, end_va: VirtAddr) -> Result<(), &str> {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        let current_memory_set = &mut inner.tasks[cur].memory_set;

        let mut next_vpn = start_va.floor();
        let end_vpn = end_va.ceil();

        while next_vpn < end_vpn {
            if let Some(pte) = current_memory_set.translate(next_vpn) {
                if !pte.is_valid() {
                    debug!("unmap: area not mapped: {:x?}", next_vpn);
                    return Err("unmap: area not mapped");
                }
            }
            next_vpn.0 += 1;
        }

        current_memory_set.remove_framed_area(start_va, end_va);
        Ok(())
    }
}

/// Run the first task in task list.
pub fn run_first_task() {
    TASK_MANAGER.run_first_task();
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

/// Get the current 'Running' task's token.
pub fn current_user_token() -> usize {
    TASK_MANAGER.get_current_token()
}

/// Get the current 'Running' task's trap contexts.
pub fn current_trap_cx() -> &'static mut TrapContext {
    TASK_MANAGER.get_current_trap_cx()
}

/// Change the current 'Running' task's program break
pub fn change_program_brk(size: i32) -> Option<usize> {
    TASK_MANAGER.change_current_program_brk(size)
}

/// Share a ref
pub struct Shared<'a, T> {
    owner: RefMut<'a, TaskManagerInner>,
    value: NonNull<T>,
}

impl Borrow<TaskControlBlock> for Shared<'_, TaskControlBlock> {
    fn borrow(&self) -> &TaskControlBlock {
        unsafe { self.value.as_ref() }
    }
}

impl BorrowMut<TaskControlBlock> for Shared<'_, TaskControlBlock> {
    fn borrow_mut(&mut self) -> &mut TaskControlBlock {
        unsafe { self.value.as_mut() }
    }
}

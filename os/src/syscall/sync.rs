use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;
fn get_pid() -> usize { current_task().unwrap().process.upgrade().unwrap().getpid() }
fn get_tid() -> usize { 
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid 
}
/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    debug!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        get_pid(), get_tid()
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}
/// mutex create syscall
pub fn sys_mutex_create(blocking: bool) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_create",
        get_pid(), get_tid()
    );
    let process = current_process();
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };
    let mut process_inner = process.inner_exclusive_access();
    if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        id as isize
    } else {
        process_inner.mutex_list.push(mutex);
        let id = process_inner.mutex_list.len() - 1;
        process_inner.new_mutex(id);
        id as isize
    }
}
/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    let pid = get_pid();
    let tid = get_tid();
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_lock",
        pid, tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    let try_lock = process_inner.try_lock_mutex(tid, mutex_id);
    drop(process_inner);
    drop(process);
    if try_lock {
        mutex.lock();
        {
            current_process().inner_exclusive_access().lock_mutex(tid, mutex_id);
        }
        return 0;
    } else {
        info!("mutex {} deadlock detected", mutex_id);
        return -0xdead;
    }
}
/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    let pid = get_pid();
    let tid = get_tid();
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_unlock",
        pid, tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    process_inner.unlock_mutex(tid, mutex_id);
    drop(process_inner);
    drop(process);
    mutex.unlock();
    0
}
/// semaphore create syscall
pub fn sys_semaphore_create(res_count: usize) -> isize {
    let pid = get_pid();
    let tid = get_tid();
    debug!(
        "kernel:pid[{}] tid[{}] sys_semaphore_create({})",
        pid, tid, res_count
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .semaphore_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
        id
    } else {
        process_inner
            .semaphore_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
        let id = process_inner.semaphore_list.len() - 1;
        process_inner.new_semaphore(id, res_count);
        id
    };
    id as isize
}
/// semaphore up syscall
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    let pid = get_pid();
    let tid = get_tid();
    debug!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up",
        pid, tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    info!("semaphore {} up", sem_id);
    process_inner.release_semaphore(sem_id, 1);
    sem.up();
    0
}
/// semaphore down syscall
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    let pid = get_pid();
    let tid = get_tid();
    debug!(
        "kernel:pid[{}] tid[{}] sys_semaphore_down",
        pid, tid
    );
    let process = current_process();
    let try_down = {
        let mut process_inner = process.inner_exclusive_access();
        if process_inner.try_request_semaphore(tid, sem_id, 1) {
            process_inner.semaphore_list[sem_id].as_ref().cloned()
        } else {
            None
        }
    };
    if let Some(sem) = try_down {
        sem.down();
        info!("semaphore {} down", sem_id);
        {
            current_process().inner_exclusive_access().request_semaphore(tid, sem_id, 1);
        }
        return 0;
    } else {
        info!("semaphore {} deadlock", sem_id);
        debug!("???{} -> {:?}", tid, current_process().inner_exclusive_access().banker);
        return -0xdead;
    }
}
/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        let id = process_inner.condvar_list.len() - 1;
        process_inner.new_condvar(id);
        id
    };
    id as isize
}
/// condvar signal syscall
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_signal",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}
/// condvar wait syscall
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_wait",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}
/// enable deadlock detection syscall
///
/// YOUR JOB: Implement deadlock detection, but might not all in this syscall
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    let process = current_process();
    if process.turn_deadlock_detect(enabled != 0) {
        0
    } else {
        -1
    }
}

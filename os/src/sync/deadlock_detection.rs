use core::fmt::Display;

use alloc::{collections::vec_deque::VecDeque, vec::Vec};

/// Deadlock detection
#[derive(Debug)]
pub struct Banker<R> {
    // resource id is the index in the resources vector
    resources: Vec<Option<R>>, // None means the resource is deallocated
    recycle: VecDeque<usize>, // recycle resource id
    num_tasks: usize,
    // [task][resource]
    max: Vec<Vec<usize>>,
    allocated: Vec<Vec<usize>>,
    available: Vec<usize>,
    need: Vec<Vec<usize>>,
}

impl <R: Eq + Copy + Display> Banker<R> {
    /// Create a new DeadlockDetection
    pub fn new() -> Self {
        Self {
            resources: Vec::new(),
            recycle: VecDeque::new(),
            num_tasks: 1,
            max: alloc::vec![Vec::new()],
            allocated: alloc::vec![Vec::new()],
            available: Vec::new(),
            need: alloc::vec![Vec::new()],
        }
    }

    /// Add a task to the allocated list
    pub fn update_task(&mut self, task_id: usize) -> usize {
        if task_id < self.num_tasks {
            return self.num_tasks;
        }
        for _ in self.num_tasks..=task_id {
            self.max.push(alloc::vec![0; self.resources.len()]);
            self.allocated.push(alloc::vec![0; self.resources.len()]);
            self.need.push(alloc::vec![0; self.resources.len()]);
        }
        self.num_tasks = task_id + 1;
        self.num_tasks
    }

    /// Remove a task from the allocated list
    pub fn remove_task(&mut self, task_id: usize) -> bool {
        if task_id >= self.num_tasks {
            return false;
        }
        self.max[task_id].fill(0);
        self.allocated[task_id].fill(0);
        self.need[task_id].fill(0);
        true
    }

    /// Add a task to the allocated list
    pub fn add_resource(&mut self, resource: R, total: usize) {
        if let Some(id) = self.recycle.pop_front() {
            self.resources[id] = Some(resource);
            self.available[id] = total;
            return;
        }

        self.resources.push(Some(resource));
        self.available.push(total);
        self.max.iter_mut().for_each(|task| task.push(0));
        self.allocated.iter_mut().for_each(|task| task.push(0));
        self.need.iter_mut().for_each(|task| task.push(0));
    }

    /// Remove a task from the allocated list
    pub fn remove_resource(&mut self, resource: R) -> bool {
        if let Some(id) = self.resource_id(resource) {
            self.resources[id] = None;
            self.recycle.push_back(id);
            self.available[id] = 0;
            self.max.iter_mut().for_each(|task| task[id] = 0);
            self.allocated.iter_mut().for_each(|task| task[id] = 0);
            self.need.iter_mut().for_each(|task| task[id] = 0);
            return true;
        }
        false
    }

    /// Get the resource id
    pub fn resource_id(&self, resource: R) -> Option<usize> {
        self.resources.iter().position(|res| res == &Some(resource))
    }

    /// Release new resource from a task
    pub fn release_new(&mut self, resource: R, amount: usize) {
        let Some(resource_id) = self.resource_id(resource) else {
            panic!("resource {} not found", resource);
        };

        self.available[resource_id] += amount;
    }

    /// Release a holding resource from a task
    pub fn release_holding(&mut self, task_id: usize, resource: R, amount: usize) {
        assert!(task_id < self.num_tasks, "task_id out of range");

        let Some(resource_id) = self.resource_id(resource) else {
            panic!("resource {} not found", resource);
        };

        self.available[resource_id] += amount;
        self.allocated[task_id][resource_id] -= amount;
        self.max[task_id][resource_id] -= amount;
    }

    /// Allocate a resource to a task
    pub fn is_safe(&self) -> bool {
        let mut work = self.available.clone();
        let mut finish = alloc::vec![false; self.num_tasks];
        let mut count = 0;
        while count < self.num_tasks {
            let mut found = false;
            for task_id in 0..self.num_tasks {
                if finish[task_id] {
                    continue;
                }

                let mut is_safe = true;
                for resource_id in 0..self.resources.len() {
                    if self.need[task_id][resource_id] > work[resource_id] {
                        is_safe = false;
                        break;
                    }
                }

                if is_safe {
                    for resource_id in 0..self.resources.len() {
                        work[resource_id] += self.allocated[task_id][resource_id];
                    }
                    finish[task_id] = true;
                    count += 1;
                    found = true;
                }
            }

            if !found {
                debug!("???, {:?}", work);
                debug!("???, {:?}", finish);
                return false;
            }
        }

        true
    }

    /// Allocate a resource to a task
    pub fn try_request(&mut self, task_id: usize, resource: R, amount: usize) -> bool {
        assert!(task_id < self.num_tasks, "task_id out of range");

        let Some(resource_id) = self.resource_id(resource) else {
            panic!("resource not found");
        };

        self.max[task_id][resource_id] += amount;
        self.need[task_id][resource_id] += amount;
        if self.is_safe() {
            true
        } else {
            self.max[task_id][resource_id] -= amount;
            self.need[task_id][resource_id] -= amount;
            false
        }
    }

    /// Allocate a resource to a task
    pub fn request(&mut self, task_id: usize, resource: R, amount: usize) {
        assert!(task_id < self.num_tasks, "task_id out of range");

        let Some(resource_id) = self.resource_id(resource) else {
            panic!("resource not found");
        };

        assert!(amount <= self.available[resource_id], "amount exceeds available");

        self.allocated[task_id][resource_id] += amount;
        self.available[resource_id] -= amount;
        self.need[task_id][resource_id] -= amount;
    }
}
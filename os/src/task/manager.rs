//!Implementation of [`TaskManager`]
use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::collections::BinaryHeap;
use alloc::sync::Arc;
use core::cmp::Ordering;
use lazy_static::*;
///A array of `TaskControlBlock` that is thread-safe

/// Pointer to TCB
struct PTCB(Arc<TaskControlBlock>);
impl Eq for PTCB {}

impl PartialEq<Self> for PTCB {
    fn eq(&self, other: &Self) -> bool {
        self.0.inner_shared_access().stride.eq(&other.0.inner_shared_access().stride)
    }
}

impl PartialOrd<Self> for PTCB {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        other.0.inner_shared_access().stride.partial_cmp(&self.0.inner_shared_access().stride)
    }
}

impl Ord for PTCB
{
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.inner_shared_access().stride.cmp(&self.0.inner_shared_access().stride)
    }
}

/// Task Manager
pub struct TaskManager {
    ready_queue: BinaryHeap<PTCB>,
}

/// A simple FIFO scheduler.
impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: BinaryHeap::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push(PTCB(task));
    }
    /// Take a process out of the ready queue
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        self.ready_queue.pop().map(|x|{x.0})
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    TASK_MANAGER.exclusive_access().fetch()
}

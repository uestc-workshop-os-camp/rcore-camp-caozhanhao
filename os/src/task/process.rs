//! Implementation of  [`ProcessControlBlock`]

use super::id::RecycleAllocator;
use super::manager::insert_into_pid2process;
use super::TaskControlBlock;
use super::{add_task, SignalFlags};
use super::{pid_alloc, PidHandle};
use crate::fs::{File, Stdin, Stdout};
use crate::mm::{translated_refmut, MemorySet, KERNEL_SPACE};
use crate::sync::{Condvar, Mutex, Semaphore, UPSafeCell};
use crate::trap::{trap_handler, TrapContext};
use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec;
use alloc::vec::Vec;
use core::cell::{Ref, RefMut};

pub struct DLDCBInner
{
    /// `avail[j] == k` means that the available quantity of the j-th type resource is k.
    /// IOW, the mutex/semaphore whose id is j has k left.
    pub avail: Vec<Option<usize>>,
    /// `alloc[i][j] == g` means that thread i has currently been allocated the number of
    /// j-th type resources, which is g.
    pub alloc: Vec<Option<Vec<Option<usize>>>>,
    /// `need[i][j] == f` means that thread i still needs the number of j-th type resources of f.
    pub need: Vec<Option<Vec<Option<usize>>>>,
}

impl DLDCBInner
{
    /// new
    pub fn new() -> Self
    {
        Self
        {
            avail: Vec::new(),
            alloc: Vec::new(),
            need: Vec::new(),
        }
    }
    /// add a thread
    pub fn add_thread(&mut self, id: usize)
    {
        let add_to_matrix = |matrix: &mut Vec<Option<Vec<Option<usize>>>>| {
            if id < matrix.len()
            {
                assert!(matrix[id].is_none());
                matrix[id] = Some(Vec::new());
            } else {
                while matrix.len() != id
                {
                    matrix.push(None);
                }
                matrix.push(Some(Vec::new()));
            }
            let new = matrix[id].as_mut().unwrap();
            for a in self.avail.iter()
            {
                new.push(a.map(|_| { 0 }));
            }
        };
        add_to_matrix(&mut self.alloc);
        add_to_matrix(&mut self.need);
    }
    /// add resource
    pub fn add_res(&mut self, res_type: usize, res_avail: usize)
    {
        if res_type < self.avail.len()
        {
            assert!(self.avail[res_type].is_none());
            self.avail[res_type] = Some(res_avail);
            let add_to_matrix = |matrix: &mut Vec<Option<Vec<Option<usize>>>>| {
                for a in matrix.iter_mut()
                {
                    if let Some(ref mut b) = a
                    {
                        assert!(b[res_type].is_none());
                        b[res_type] = Some(0);
                    }
                }
            };
            add_to_matrix(&mut self.alloc);
            add_to_matrix(&mut self.need);
        } else {
            while self.avail.len() != res_type
            {
                self.avail.push(None);
            }
            self.avail.push(Some(res_avail));
            let add_to_matrix = |matrix: &mut Vec<Option<Vec<Option<usize>>>>| {
                for a in matrix.iter_mut()
                {
                    if let Some(ref mut b) = a
                    {
                        while b.len() != res_type
                        {
                            b.push(None);
                        }
                        b.push(Some(0));
                    }
                }
            };
            add_to_matrix(&mut self.alloc);
            add_to_matrix(&mut self.need);
        }
    }

    /// get resource
    pub fn get_resource(&mut self, request_resource: usize, thread_id: usize, res_type: usize) -> isize
    {
        assert!(self.avail[res_type].is_some());
        assert!(self.alloc[thread_id].is_some());
        assert!(self.need[thread_id].is_some());

        let avail_bak = self.avail.clone();
        let alloc_bak = self.alloc.clone();
        let need_bak = self.need.clone();

        let avail = self.avail[res_type].as_mut().unwrap();
        let alloc = self.alloc[thread_id].as_mut().unwrap()[res_type].as_mut().unwrap();
        let need = self.need[thread_id].as_mut().unwrap()[res_type].as_mut().unwrap();

        if request_resource > *avail
        {
            *need = request_resource;
        } else {
            *need = 0;
            *avail -= request_resource;
            *alloc += request_resource;
        }

        let mut work = self.avail.clone();
        let mut finish = vec![false; self.alloc.len()];

        loop {
            let task = finish.iter().enumerate().find(|(i, finished)| {
                if **finished { return false; }
                if let Some(ref n) = self.need[*i]
                {
                    for (j, opt) in n.iter().enumerate()
                    {
                        if let Some(need_num) = opt
                        {
                            let work_num = *work[j].as_ref().unwrap();
                            if *need_num > work_num
                            {
                                return false;
                            }
                        }
                    }
                }
                true
            });
            if let Some((i, _)) = task {
                for (j, opt) in work.iter_mut().enumerate()
                {
                    if let Some(ref mut work_num) = opt
                    {
                        *work_num += *self.alloc[i].as_ref().unwrap()[j].as_ref().unwrap();
                    }
                }
                finish[i] = true;
            } else {
                break;
            }
        }

        if finish.iter().any(|x| !x)
        {
            self.avail = avail_bak;
            self.alloc = alloc_bak;
            self.need = need_bak;
            return -1;
        }
        0
    }

    /// release resource
    pub fn release_resource(&mut self, released_resource: usize, thread_id: usize, res_type: usize)
    {
        assert!(res_type < self.avail.len() && self.avail[res_type].is_some());
        assert!(thread_id < self.alloc.len() && self.alloc[thread_id].is_some());
        assert!(thread_id < self.need.len() && self.need[thread_id].is_some());

        let avail = self.avail[res_type].as_mut().unwrap();
        let alloc = self.alloc[thread_id].as_mut().unwrap()[res_type].as_mut().unwrap();

        *avail += released_resource;
        *alloc -= released_resource;
    }
}

pub struct DeadLockDetectControlBlock
{
    /// Mutex
    pub mtx: DLDCBInner,
    /// Semaphore
    pub sem: DLDCBInner,
}

impl DeadLockDetectControlBlock
{
    /// new a control block
    pub fn new() -> Self
    {
        Self {
            mtx: DLDCBInner::new(),
            sem: DLDCBInner::new(),
        }
    }
    /// add a thread
    pub fn add_thread(&mut self, id: usize)
    {
        self.mtx.add_thread(id);
        self.sem.add_thread(id);
    }
    /// create mutex
    pub fn add_mutex(&mut self, id: usize)
    {
        self.mtx.add_res(id, 1);
    }
    /// create semaphore
    pub fn add_semaphore(&mut self, id: usize, avail: usize)
    {
        self.sem.add_res(id, avail);
    }
    /// get mutex resource
    pub fn get_mutex_resource(&mut self, thread_id: usize, mutex_id: usize) -> isize
    {
        self.mtx.get_resource(1, thread_id, mutex_id)
    }
    /// get semaphore resource
    pub fn get_semaphore_resource(&mut self, thread_id: usize, semaphore_id: usize) -> isize
    {
        self.sem.get_resource(1, thread_id, semaphore_id)
    }
    /// release mutex resource
    pub fn release_mutex_resource(&mut self, thread_id: usize, mutex_id: usize)
    {
        self.mtx.release_resource(1, thread_id, mutex_id);
    }
    /// get semaphore resource
    pub fn release_semaphore_resource(&mut self, thread_id: usize, semaphore_id: usize)
    {
        self.sem.release_resource(1, thread_id, semaphore_id);
    }
}

/// Process Control Block
pub struct ProcessControlBlock {
    /// immutable
    pub pid: PidHandle,
    /// mutable
    inner: UPSafeCell<ProcessControlBlockInner>,
}

/// Inner of Process Control Block
pub struct ProcessControlBlockInner {
    /// is zombie?
    pub is_zombie: bool,
    /// memory set(address space)
    pub memory_set: MemorySet,
    /// parent process
    pub parent: Option<Weak<ProcessControlBlock>>,
    /// children process
    pub children: Vec<Arc<ProcessControlBlock>>,
    /// exit code
    pub exit_code: i32,
    /// file descriptor table
    pub fd_table: Vec<Option<Arc<dyn File + Send + Sync>>>,
    /// signal flags
    pub signals: SignalFlags,
    /// tasks(also known as threads)
    pub tasks: Vec<Option<Arc<TaskControlBlock>>>,
    /// task resource allocator
    pub task_res_allocator: RecycleAllocator,
    /// mutex list
    pub mutex_list: Vec<Option<Arc<dyn Mutex>>>,
    /// semaphore list
    pub semaphore_list: Vec<Option<Arc<Semaphore>>>,
    /// condvar list
    pub condvar_list: Vec<Option<Arc<Condvar>>>,
    /// deadlock detect
    pub deadlock_detect: bool,
    /// deadlock detect control block
    pub deadlock_ctl: DeadLockDetectControlBlock,
}

impl ProcessControlBlockInner {
    #[allow(unused)]
    /// get the address of app's page table
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    /// allocate a new file descriptor
    pub fn alloc_fd(&mut self) -> usize {
        if let Some(fd) = (0..self.fd_table.len()).find(|fd| self.fd_table[*fd].is_none()) {
            fd
        } else {
            self.fd_table.push(None);
            self.fd_table.len() - 1
        }
    }
    /// allocate a new task id
    pub fn alloc_tid(&mut self) -> usize {
        self.task_res_allocator.alloc()
    }
    /// deallocate a task id
    pub fn dealloc_tid(&mut self, tid: usize) {
        self.task_res_allocator.dealloc(tid)
    }
    /// the count of tasks(threads) in this process
    pub fn thread_count(&self) -> usize {
        self.tasks.len()
    }
    /// get a task with tid in this process
    pub fn get_task(&self, tid: usize) -> Arc<TaskControlBlock> {
        self.tasks[tid].as_ref().unwrap().clone()
    }
    /// enable deadlock detect
    pub fn enable_deadlock_detect(&mut self) -> isize {
        assert!(self.mutex_list.is_empty() && self.semaphore_list.is_empty());
        if self.deadlock_detect { return -1; }
        self.deadlock_detect = true;
        for (id, opt) in self.tasks.iter().enumerate()
        {
            if opt.is_some()
            {
                self.deadlock_ctl.add_thread(id);
            }
        }
        0
    }
    /// disable deadlock detect
    pub fn disable_deadlock_detect(&mut self) -> isize
    {
        if !self.deadlock_detect { return -1; }
        self.deadlock_detect = false;
        self.deadlock_ctl = DeadLockDetectControlBlock::new();
        0
    }
}

impl ProcessControlBlock {
    /// inner_exclusive_access
    pub fn inner_exclusive_access(&self) -> RefMut<'_, ProcessControlBlockInner> {
        self.inner.exclusive_access()
    }
    /// inner_shared_access
    pub fn inner_shared_access(&self) -> Ref<'_, ProcessControlBlockInner> {
        self.inner.shared_access()
    }
    /// new process from elf file
    pub fn new(elf_data: &[u8]) -> Arc<Self> {
        trace!("kernel: ProcessControlBlock::new");
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, ustack_base, entry_point) = MemorySet::from_elf(elf_data);
        // allocate a pid
        let pid_handle = pid_alloc();
        let process = Arc::new(Self {
            pid: pid_handle,
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    is_zombie: false,
                    memory_set,
                    parent: None,
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: vec![
                        // 0 -> stdin
                        Some(Arc::new(Stdin)),
                        // 1 -> stdout
                        Some(Arc::new(Stdout)),
                        // 2 -> stderr
                        Some(Arc::new(Stdout)),
                    ],
                    signals: SignalFlags::empty(),
                    tasks: Vec::new(),
                    task_res_allocator: RecycleAllocator::new(),
                    mutex_list: Vec::new(),
                    semaphore_list: Vec::new(),
                    condvar_list: Vec::new(),
                    deadlock_detect: false,
                    deadlock_ctl: DeadLockDetectControlBlock::new(),
                })
            },
        });
        // create a main thread, we should allocate ustack and trap_cx here
        let task = Arc::new(TaskControlBlock::new(
            Arc::clone(&process),
            ustack_base,
            true,
        ));
        // prepare trap_cx of main thread
        let task_inner = task.inner_exclusive_access();
        let trap_cx = task_inner.get_trap_cx();
        let ustack_top = task_inner.res.as_ref().unwrap().ustack_top();
        let kstack_top = task.kstack.get_top();
        drop(task_inner);
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            ustack_top,
            KERNEL_SPACE.exclusive_access().token(),
            kstack_top,
            trap_handler as usize,
        );
        // add main thread to the process
        let mut process_inner = process.inner_exclusive_access();
        process_inner.tasks.push(Some(Arc::clone(&task)));
        drop(process_inner);
        insert_into_pid2process(process.getpid(), Arc::clone(&process));
        // add main thread to scheduler
        add_task(task);
        process
    }

    /// Only support processes with a single thread.
    pub fn exec(self: &Arc<Self>, elf_data: &[u8], args: Vec<String>) {
        trace!("kernel: exec");
        assert_eq!(self.inner_exclusive_access().thread_count(), 1);
        // memory_set with elf program headers/trampoline/trap context/user stack
        trace!("kernel: exec .. MemorySet::from_elf");
        let (memory_set, ustack_base, entry_point) = MemorySet::from_elf(elf_data);
        let new_token = memory_set.token();
        // substitute memory_set
        trace!("kernel: exec .. substitute memory_set");
        self.inner_exclusive_access().memory_set = memory_set;
        // then we alloc user resource for main thread again
        // since memory_set has been changed
        trace!("kernel: exec .. alloc user resource for main thread again");
        let task = self.inner_exclusive_access().get_task(0);
        let mut task_inner = task.inner_exclusive_access();
        task_inner.res.as_mut().unwrap().ustack_base = ustack_base;
        task_inner.res.as_mut().unwrap().alloc_user_res();
        task_inner.trap_cx_ppn = task_inner.res.as_mut().unwrap().trap_cx_ppn();
        // push arguments on user stack
        trace!("kernel: exec .. push arguments on user stack");
        let mut user_sp = task_inner.res.as_mut().unwrap().ustack_top();
        user_sp -= (args.len() + 1) * core::mem::size_of::<usize>();
        let argv_base = user_sp;
        let mut argv: Vec<_> = (0..=args.len())
            .map(|arg| {
                translated_refmut(
                    new_token,
                    (argv_base + arg * core::mem::size_of::<usize>()) as *mut usize,
                )
            })
            .collect();
        *argv[args.len()] = 0;
        for i in 0..args.len() {
            user_sp -= args[i].len() + 1;
            *argv[i] = user_sp;
            let mut p = user_sp;
            for c in args[i].as_bytes() {
                *translated_refmut(new_token, p as *mut u8) = *c;
                p += 1;
            }
            *translated_refmut(new_token, p as *mut u8) = 0;
        }
        // make the user_sp aligned to 8B for k210 platform
        user_sp -= user_sp % core::mem::size_of::<usize>();
        // initialize trap_cx
        trace!("kernel: exec .. initialize trap_cx");
        let mut trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            task.kstack.get_top(),
            trap_handler as usize,
        );
        trap_cx.x[10] = args.len();
        trap_cx.x[11] = argv_base;
        *task_inner.get_trap_cx() = trap_cx;
    }

    /// Only support processes with a single thread.
    pub fn fork(self: &Arc<Self>) -> Arc<Self> {
        trace!("kernel: fork");
        let mut parent = self.inner_exclusive_access();
        assert_eq!(parent.thread_count(), 1);
        // clone parent's memory_set completely including trampoline/ustacks/trap_cxs
        let memory_set = MemorySet::from_existed_user(&parent.memory_set);
        // alloc a pid
        let pid = pid_alloc();
        // copy fd table
        let mut new_fd_table: Vec<Option<Arc<dyn File + Send + Sync>>> = Vec::new();
        for fd in parent.fd_table.iter() {
            if let Some(file) = fd {
                new_fd_table.push(Some(file.clone()));
            } else {
                new_fd_table.push(None);
            }
        }
        // create child process pcb
        let child = Arc::new(Self {
            pid,
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    is_zombie: false,
                    memory_set,
                    parent: Some(Arc::downgrade(self)),
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: new_fd_table,
                    signals: SignalFlags::empty(),
                    tasks: Vec::new(),
                    task_res_allocator: RecycleAllocator::new(),
                    mutex_list: Vec::new(),
                    semaphore_list: Vec::new(),
                    condvar_list: Vec::new(),
                    deadlock_detect: false,
                    deadlock_ctl: DeadLockDetectControlBlock::new(),
                })
            },
        });
        // add child
        parent.children.push(Arc::clone(&child));
        // create main thread of child process
        let task = Arc::new(TaskControlBlock::new(
            Arc::clone(&child),
            parent
                .get_task(0)
                .inner_exclusive_access()
                .res
                .as_ref()
                .unwrap()
                .ustack_base(),
            // here we do not allocate trap_cx or ustack again
            // but mention that we allocate a new kstack here
            false,
        ));
        // attach task to child process
        let mut child_inner = child.inner_exclusive_access();
        child_inner.tasks.push(Some(Arc::clone(&task)));
        drop(child_inner);
        // modify kstack_top in trap_cx of this thread
        let task_inner = task.inner_exclusive_access();
        let trap_cx = task_inner.get_trap_cx();
        trap_cx.kernel_sp = task.kstack.get_top();
        drop(task_inner);
        insert_into_pid2process(child.getpid(), Arc::clone(&child));
        // add this thread to scheduler
        add_task(task);
        child
    }
    /// get pid
    pub fn getpid(&self) -> usize {
        self.pid.0
    }
}

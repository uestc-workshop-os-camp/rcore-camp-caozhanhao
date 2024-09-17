//! Process management syscalls
use crate::{
    loader::get_app_data_by_name,
    mm::{translated_byte_buffer, translated_refmut, translated_str, MapPermission},
    task::*,
    timer::get_time_us,
};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::mem::size_of;
use core::slice::from_raw_parts;


#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
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
        task.exec(data);
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

fn copy_to_buffers(src: &[u8], dest: Vec<&mut [u8]>) {
    let mut beg = 0_usize;
    for buf in dest {
        if buf.len() < src.len() - beg
        {
            buf.copy_from_slice(&src[beg..beg + buf.len()]);
            beg += buf.len();
        } else {
            buf[..src.len() - beg].copy_from_slice(&src[beg..]);
            break;
        }
    }
}

unsafe fn copy_to_app<T>(item: &T, dest: *mut T)
{
    let raw = from_raw_parts(item as *const _ as *const u8, size_of::<T>());
    let buffers = translated_byte_buffer(current_user_token(),
                                         dest as *const u8, size_of::<T>());
    copy_to_buffers(raw, buffers);
}

// unsafe fn copy_to_app<T>(item: &T, dest: *mut T)
// where
//     [(); size_of::<T>()]:,
// {
//     let raw: [u8; size_of::<T>()] = transmute_copy(item);
//
//     // Comment the next line will lead to infinite loop, WHY?
//     trace!("{:?}", raw);
//
//     let buffers = translated_byte_buffer(current_user_token(),
//                                          dest as *const u8, size_of::<T>());
//     copy_to_buffers(&raw, buffers);
// }

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    let time = TimeVal { sec: us / 1_000_000, usec: us % 1_000_000 };
    unsafe { copy_to_app(&time, _ts); }
    0
}

/// YOUR JOB: Finish sys_task_info to pass testcases
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TaskInfo`] is splitted by two pages ?
pub fn sys_task_info(_ti: *mut TaskInfo) -> isize {
    trace!("kernel: sys_task_info");
    let info = current_task().unwrap().inner_shared_access().task_info;
    unsafe { copy_to_app(&info, _ti); }
    0
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    trace!("kernel: sys_mmap");
    if _start % 4096 != 0 {
        return -1;
    } else if _len == 0 {
        return 0;
    } else if _port | 0b111 != 0b111 || _port & 0b111 == 0 {
        return -1;
    }
    let mut perm = MapPermission::U;
    if _port & 0b001 != 0 {
        perm |= MapPermission::R;
    }
    if _port & 0b010 != 0 {
        perm |= MapPermission::W;
    }
    if _port & 0b100 != 0 {
        perm |= MapPermission::X;
    }

    current_task().unwrap().inner_exclusive_access().memory_set
        .try_insert_framed_area(_start.into(), (_start + _len).into(), perm)
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!("kernel: sys_munmap");
    if _start % 4096 != 0 {
        return -1;
    } else if _len == 0 {
        return 0;
    }
    current_task().unwrap().inner_exclusive_access().memory_set
        .try_remove_area(_start.into(), (_start + _len).into())
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
pub fn sys_spawn(_path: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_spawn",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    let path = translated_str(token, _path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let curr = current_task().unwrap();
        let new_task = curr.spawn(data);
        let new_pid = new_task.pid.0;
        add_task(new_task);
        new_pid as isize
    } else {
        -1
    }
}

// YOUR JOB: Set task priority.
pub fn sys_set_priority(_prio: isize) -> isize {
    trace!(
        "kernel:pid[{}] sys_set_priority NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    if _prio < 2 { return -1; }
    current_task().unwrap().inner_exclusive_access().priority = _prio as usize;
    _prio
}

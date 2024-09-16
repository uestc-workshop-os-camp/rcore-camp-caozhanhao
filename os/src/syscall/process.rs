//! Process management syscalls

use crate::mm::{translated_byte_buffer, MapPermission};
use crate::task::*;
use crate::timer::get_time_us;
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
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
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
    let info = get_current_tcblk().task_info;
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

    get_current_tcblk().memory_set
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
    get_current_tcblk().memory_set.try_remove_area(_start.into(), (_start + _len).into())
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

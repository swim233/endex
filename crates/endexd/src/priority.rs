//! Thread priorities and returning freed memory to the system.

use std::io;

const IOPRIO_WHO_PROCESS: libc::c_int = 1;
const IOPRIO_CLASS_IDLE: libc::c_int = 3;
const IOPRIO_CLASS_SHIFT: libc::c_int = 13;

/// Moves the calling thread to nice 19 and the idle IO class. Threads it creates later (ONNX
/// Runtime and LiteRT thread pools) inherit both. Unprivileged threads cannot undo this.
pub fn lower_current_thread() -> io::Result<()> {
    // SAFETY: plain syscalls on the calling thread's id.
    unsafe {
        let tid = libc::gettid();
        if libc::setpriority(libc::PRIO_PROCESS, tid as libc::id_t, 19) != 0 {
            return Err(io::Error::last_os_error());
        }
        let class = IOPRIO_CLASS_IDLE << IOPRIO_CLASS_SHIFT;
        if libc::syscall(libc::SYS_ioprio_set, IOPRIO_WHO_PROCESS, tid, class) != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Hands memory freed by released models back to the system instead of keeping it in the
/// allocator's free lists.
pub fn trim_heap() {
    // SAFETY: no preconditions.
    unsafe {
        libc::malloc_trim(0);
    }
}

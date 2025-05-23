cfg_if::cfg_if! {
    if #[cfg(target_os = "zkvm")] {
        use core::arch::asm;
        use crate::zkvm;
        use sha2::digest::Update;
    }
}

/// Write `nbytes` of data to the prover to a given file descriptor `fd` from `write_buf`.
#[allow(unused_variables)]
#[no_mangle]
pub extern "C" fn syscall_write(fd: u32, write_buf: *const u8, nbytes: usize) {
    cfg_if::cfg_if! {
        if #[cfg(target_os = "zkvm")] {
            const FD_PUBLIC_VALUES: u32 = 3;
            unsafe {
                asm!(
                    "syscall",
                    in("$2") crate::syscalls::WRITE,
                    in("$4") fd,
                    in("$5") write_buf,
                    in("$6") nbytes,
                );
            }

            // For writes to the public values fd, we update a global program hasher with the bytes
            // being written. At the end of the program, we call the COMMIT syscall with the finalized
            // version of this hash.
            if fd == FD_PUBLIC_VALUES {
                let pi_slice: &[u8] = unsafe { core::slice::from_raw_parts(write_buf, nbytes) };
                unsafe { zkvm::PUBLIC_VALUES_HASHER.as_mut().unwrap().update(pi_slice) };
            }
        } else {
            unreachable!()
        }
    }
}

/// Returns the length of the next element in the hint stream.
#[allow(unused_variables)]
#[no_mangle]
pub extern "C" fn syscall_hint_len() -> usize {
    #[cfg(target_os = "zkvm")]
    unsafe {
        let len;
        asm!(
            "syscall",
            in("$2") crate::syscalls::HINT_LEN,
            lateout("$2") len,
        );
        len
    }

    #[cfg(not(target_os = "zkvm"))]
    unreachable!()
}

/// Reads the next element in the hint stream into the given buffer.
#[allow(unused_variables)]
#[no_mangle]
pub extern "C" fn syscall_hint_read(ptr: *mut u8, len: usize) {
    #[cfg(target_os = "zkvm")]
    unsafe {
        asm!(
            "syscall",
            in("$2") crate::syscalls::HINT_READ,
            in("$4") ptr,
            in("$5") len,
        );
    }

    #[cfg(not(target_os = "zkvm"))]
    unreachable!()
}

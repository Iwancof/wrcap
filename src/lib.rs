#![feature(anonymous_pipe)]

use std::{
    io::{self, Read},
    os::fd::{FromRawFd, IntoRawFd, OwnedFd},
    pipe::{PipeReader, pipe},
    sync::{Mutex, MutexGuard, PoisonError},
};

unsafe extern "C" {
    // in fops.c
    fn swap_fd(file: *mut nix::libc::FILE, fd: nix::libc::c_int) -> nix::libc::c_int;

    // in libc
    fn flockfile(file: *mut nix::libc::FILE);
    fn funlockfile(file: *mut nix::libc::FILE);

    static mut stdout: *mut nix::libc::FILE;
    static mut stderr: *mut nix::libc::FILE;
}

pub struct LentFile {
    file: *mut nix::libc::FILE,

    #[allow(dead_code)]
    guard: MutexGuard<'static, ()>,
}

pub fn lent_stdout() -> Result<LentFile, PoisonError<MutexGuard<'static, ()>>> {
    static MUTEX: Mutex<()> = Mutex::new(());
    let guard = MUTEX.lock()?;

    unsafe { flockfile(stdout) };

    Ok(LentFile {
        file: unsafe { stdout }, // SAFETY: lock is held
        guard,
    })
}

pub fn lent_stderr() -> Result<LentFile, PoisonError<MutexGuard<'static, ()>>> {
    static MUTEX: Mutex<()> = Mutex::new(());
    let guard = MUTEX.lock()?;

    unsafe { flockfile(stderr) };

    Ok(LentFile {
        file: unsafe { stderr }, // SAFETY: lock is held
        guard,
    })
}

impl Drop for LentFile {
    fn drop(&mut self) {
        unsafe { funlockfile(self.file) };
    }
}

impl LentFile {
    unsafe fn swap_fd<FD: IntoRawFd>(&self, fd: FD) -> OwnedFd {
        let swapped = unsafe { swap_fd(self.file, fd.into_raw_fd()) };
        unsafe { OwnedFd::from_raw_fd(swapped) }
    }

    fn flush(&self) -> Result<(), io::Error> {
        if unsafe { nix::libc::fflush(self.file) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn capture_into<FD: IntoRawFd, F: FnOnce()>(&self, fd: FD, f: F) -> std::io::Result<()> {
        // self.file is locked. and any other threads can't create a new LentFile.

        // before install fd, we must flush the file
        self.flush()?;

        let old_fd = unsafe { self.swap_fd(fd) };

        f();

        // after capture, we must flush the file
        self.flush()?;

        let _swapped = unsafe { self.swap_fd(old_fd) };

        // drop _swapped(pipe writer)

        Ok(())
    }

    pub fn capture<F: FnOnce()>(&self, f: F) -> std::io::Result<PipeReader> {
        let (reader, writer) = pipe()?;

        self.capture_into(writer, f)?;

        Ok(reader)
    }

    pub fn capture_string<F: FnOnce()>(&self, f: F) -> std::io::Result<String> {
        let mut reader = self.capture(f)?;
        let mut string = String::new();
        reader.read_to_string(&mut string)?;

        Ok(string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" {
        fn puts(s: *const u8) -> i32;
        fn printf(s: *const u8) -> i32;
    }

    #[test]
    fn stress_test() {
        let mut threads = Vec::new();
        for tid in 0..5 {
            let thread = std::thread::spawn(move || {
                for i in 0..100 {
                    let r = lent_stdout()
                        .unwrap()
                        .capture_string(|| unsafe {
                            puts(format!("Hello, world! {}\0", i).as_ptr());

                            // sleep random time
                            std::thread::sleep(std::time::Duration::from_millis(
                                rand::random::<u64>() % 10,
                            ));

                            printf(b"goodbye\0".as_ptr());
                        })
                        .unwrap();

                    println!("cap_stdout thread {}: {}", tid, i);

                    assert_eq!(r, format!("Hello, world! {}\ngoodbye", i));
                }
            });

            threads.push(thread);
        }

        for tid in 0..5 {
            let thread = std::thread::spawn(move || {
                for i in 0..100 {
                    unsafe {
                        printf(format!("Hello from outside of cap_stdout! {}\0", i).as_ptr());

                        // sleep random time
                        std::thread::sleep(std::time::Duration::from_millis(
                            rand::random::<u64>() % 10,
                        ));

                        printf(b"goodbye~~~\0".as_ptr());
                    }

                    println!("outside cap_stdout thread {}: {}", tid, i);
                }
            });

            threads.push(thread);
        }

        for thread in threads {
            thread.join().unwrap();
        }
    }
}

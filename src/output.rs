use std::io::{self, IsTerminal, Write};

use crate::config::{cfg, Pager};

pub fn write_output(buf: &[u8], _line_count: usize) -> io::Result<()> {
    let stdout = io::stdout();
    if should_page(stdout.is_terminal()) {
        if let Some(()) = try_pager(buf)? {
            return Ok(());
        }
    }

    let mut out = stdout.lock();
    out.write_all(buf)?;
    out.flush()?;
    Ok(())
}

fn should_page(is_tty: bool) -> bool {
    let pager_set = std::env::var("PAGER")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .is_some();
    match cfg().pager {
        Pager::Never => false,
        Pager::Always => pager_set,
        Pager::Auto => is_tty && pager_set,
    }
}

#[cfg(unix)]
fn try_pager(buf: &[u8]) -> io::Result<Option<()>> {
    use std::ffi::{c_char, CString};

    extern "C" {
        fn pipe(fds: *mut i32) -> i32;
        fn fork() -> i32;
        fn dup2(oldfd: i32, newfd: i32) -> i32;
        fn close(fd: i32) -> i32;
        fn _exit(status: i32) -> !;
        fn write(fd: i32, buf: *const u8, count: usize) -> isize;
        fn execvp(file: *const c_char, argv: *const *const c_char) -> i32;
    }

    let Some(s) = std::env::var("PAGER").ok().filter(|s| !s.trim().is_empty()) else {
        return Ok(None);
    };
    let parts: Vec<String> = s.split_whitespace().map(|s| s.to_string()).collect();
    let Some(cmd) = parts.first() else {
        return Ok(None);
    };
    let Ok(c_cmd) = CString::new(cmd.as_str()) else {
        return Ok(None);
    };
    let c_argv: Vec<CString> = parts
        .iter()
        .map(|s| CString::new(s.as_str()).unwrap())
        .collect();
    let mut argv: Vec<*const c_char> = c_argv.iter().map(|c| c.as_ptr()).collect();
    argv.push(std::ptr::null());

    let mut fds: [i32; 2] = [0; 2];
    if unsafe { pipe(fds.as_mut_ptr()) } != 0 {
        return Ok(None);
    }
    let (rfd, wfd) = (fds[0], fds[1]);

    let _ = io::stdout().flush();
    let _ = io::stderr().flush();

    let pid = unsafe { fork() };
    if pid < 0 {
        unsafe {
            close(rfd);
            close(wfd);
        }
        return Ok(None);
    }
    if pid == 0 {
        // Child: stream rendered buf into the pipe and exit. Parent will exec
        // the pager, replacing bijjou's process so the pager keeps bijjou's
        // pid/pgid and stays in the shell's foreground process group.
        unsafe { close(rfd) };
        let mut written = 0usize;
        while written < buf.len() {
            let n =
                unsafe { write(wfd, buf.as_ptr().add(written), buf.len() - written) };
            if n < 0 {
                if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                break;
            }
            if n == 0 {
                break;
            }
            written += n as usize;
        }
        unsafe {
            close(wfd);
            _exit(0);
        }
    }

    unsafe { close(wfd) };
    if unsafe { dup2(rfd, 0) } < 0 {
        unsafe { close(rfd) };
        return Ok(None);
    }
    unsafe { close(rfd) };
    unsafe { execvp(c_cmd.as_ptr(), argv.as_ptr()) };
    eprintln!(
        "bijjou: failed to exec pager '{}': {}",
        cmd,
        io::Error::last_os_error()
    );
    unsafe { _exit(127) };
}

#[cfg(not(unix))]
fn try_pager(buf: &[u8]) -> io::Result<Option<()>> {
    use std::process::{Command, Stdio};

    let Some(s) = std::env::var("PAGER").ok().filter(|s| !s.trim().is_empty()) else {
        return Ok(None);
    };
    let mut parts = s.split_whitespace().map(|s| s.to_string());
    let Some(cmd) = parts.next() else {
        return Ok(None);
    };
    let args: Vec<String> = parts.collect();

    let mut child = match Command::new(&cmd).args(&args).stdin(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };

    if let Some(mut stdin) = child.stdin.take() {
        match stdin.write_all(buf) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => {}
            Err(e) => return Err(e),
        }
    }
    let _ = child.wait();
    Ok(Some(()))
}

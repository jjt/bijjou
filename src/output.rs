use std::io::{self, IsTerminal, Write};

pub fn write_output(buf: &[u8], line_count: usize) -> io::Result<()> {
    let stdout = io::stdout();
    let is_tty = stdout.is_terminal();
    let height = terminal_height();
    let should_page = is_tty && height.map_or(false, |h| line_count > h as usize);

    if should_page {
        if let Some(()) = try_pager(buf)? {
            return Ok(());
        }
    }

    let mut out = stdout.lock();
    out.write_all(buf)?;
    out.flush()?;
    Ok(())
}

fn terminal_height() -> Option<u16> {
    use core::ffi::c_ulong;
    use std::os::fd::AsRawFd;

    #[repr(C)]
    struct Winsize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }

    extern "C" {
        fn ioctl(fd: i32, request: c_ulong, ...) -> i32;
    }

    #[cfg(target_os = "macos")]
    const TIOCGWINSZ: c_ulong = 0x40087468;
    #[cfg(target_os = "linux")]
    const TIOCGWINSZ: c_ulong = 0x5413;
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return None;

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let mut ws: Winsize = unsafe { std::mem::zeroed() };
        let fd = io::stdout().as_raw_fd();
        let r = unsafe { ioctl(fd, TIOCGWINSZ, &mut ws as *mut Winsize) };
        if r == 0 && ws.ws_row > 0 {
            Some(ws.ws_row)
        } else {
            None
        }
    }
}

fn try_pager(buf: &[u8]) -> io::Result<Option<()>> {
    use std::process::{Command, Stdio};

    let pager_env = std::env::var("PAGER").ok().filter(|s| !s.trim().is_empty());
    let Some(s) = pager_env else {
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

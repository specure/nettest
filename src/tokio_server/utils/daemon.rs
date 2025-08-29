#[cfg(unix)]
use std::os::unix::io::AsRawFd;
#[cfg(unix)]
use nix::unistd::{fork, ForkResult, setsid, dup2};

use std::fs::File;
use std::process;
use std::env;
use log::info;

#[cfg(unix)]
pub fn daemonize() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Сохраняем текущую директорию
    let current_dir = env::current_dir()?;
    info!("Current directory before daemonize: {}", current_dir.display());

    match unsafe { fork() } {
        Ok(ForkResult::Parent { .. }) => {
            process::exit(0);
        }
        Ok(ForkResult::Child) => {
            setsid()?;
            
            env::set_current_dir(&current_dir)?;
            info!("Current directory after daemonize: {}", current_dir.display());

            unsafe { libc::umask(0); }

            let devnull = File::open("/dev/null")?;
            let null_fd = devnull.as_raw_fd();

            let stdin_fd = unsafe { libc::dup(0) };
            let stdout_fd = unsafe { libc::dup(1) };
            let stderr_fd = unsafe { libc::dup(2) };

            dup2(null_fd, 0)?;
            dup2(null_fd, 1)?;
            dup2(null_fd, 2)?;

            unsafe {
                libc::close(null_fd);
                libc::close(stdin_fd);
                libc::close(stdout_fd);
                libc::close(stderr_fd);
            }

            Ok(())
        }
        Err(e) => Err(format!("Failed to fork: {}", e).into()),
    }
}

#[cfg(windows)]
pub fn daemonize() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("Daemonize not supported on Windows, continuing as foreground process");
    Ok(())
} 
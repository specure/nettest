use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::process;
use nix::unistd::{fork, ForkResult, setsid, dup2};
use std::env;

pub fn daemonize() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match unsafe { fork() } {
        Ok(ForkResult::Parent { .. }) => {
            process::exit(0);
        }
        Ok(ForkResult::Child) => {
            // Создать новую сессию
            setsid()?;
            // Сменить рабочую директорию на /
            env::set_current_dir("/")?;
            // Установить umask(0)
            unsafe { libc::umask(0); }
            // Перенаправить stdin, stdout, stderr в /dev/null
            let devnull = File::open("/dev/null")?;
            for fd in 0..3 {
                dup2(devnull.as_raw_fd(), fd)?;
            }
            Ok(())
        }
        Err(e) => Err(format!("Failed to fork: {}", e).into()),
    }
} 
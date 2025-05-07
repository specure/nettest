use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::process;
use nix::unistd::{fork, ForkResult, setsid, dup2};
use std::env;
use log::info;

pub fn daemonize() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Сохраняем текущую директорию
    let current_dir = env::current_dir()?;
    info!("Current directory before daemonize: {}", current_dir.display());

    match unsafe { fork() } {
        Ok(ForkResult::Parent { .. }) => {
            process::exit(0);
        }
        Ok(ForkResult::Child) => {
            // Создать новую сессию
            setsid()?;
            
            // Сохраняем текущую директорию вместо смены на /
            env::set_current_dir(&current_dir)?;
            info!("Current directory after daemonize: {}", current_dir.display());

            // Установить umask(0)
            unsafe { libc::umask(0); }

            // Открываем /dev/null
            let devnull = File::open("/dev/null")?;
            let null_fd = devnull.as_raw_fd();

            // Сохраняем текущие дескрипторы
            let stdin_fd = unsafe { libc::dup(0) };
            let stdout_fd = unsafe { libc::dup(1) };
            let stderr_fd = unsafe { libc::dup(2) };

            // Перенаправляем stdin, stdout, stderr в /dev/null
            dup2(null_fd, 0)?;
            dup2(null_fd, 1)?;
            dup2(null_fd, 2)?;

            // Закрываем лишние дескрипторы
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
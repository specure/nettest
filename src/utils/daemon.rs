use std::process;
use nix::unistd::{fork, ForkResult, setsid};

pub fn daemonize() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match unsafe { fork() } {
        Ok(ForkResult::Parent { .. }) => {
            process::exit(0);
        }
        Ok(ForkResult::Child) => {
            if let Err(e) = setsid() {
                return Err(format!("Failed to create new session: {}", e).into());
            }
            Ok(())
        }
        Err(e) => Err(format!("Failed to fork: {}", e).into()),
    }
} 
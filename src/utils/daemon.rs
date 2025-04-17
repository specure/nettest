use std::process;
use libc::daemon;

pub fn daemonize() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Forking daemon...");
    
    // daemon(0, 0) - no chdir, no close
    if unsafe { daemon(0, 0) } == -1 {
        return Err(format!("Failed to daemonize: {}", std::io::Error::last_os_error()).into());
    }

    Ok(())
} 
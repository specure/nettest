use chrono::Local;
use libc::{c_int, signal, SIGHUP};
use log::{LevelFilter, Log, Metadata, Record};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct FileLogger {
    level: LevelFilter,
    log_file: Arc<Mutex<File>>,
}

impl FileLogger {
    pub fn new(level: LevelFilter, log_path: &Path) -> Result<Self, std::io::Error> {
        // Create directory if it doesn't exist
        if let Some(parent) = log_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)?;

        Ok(Self {
            level,
            log_file: Arc::new(Mutex::new(log_file)),
        })
    }

    pub fn reopen_log_file(&self, log_path: &Path) -> Result<(), std::io::Error> {
        let mut log_file = self.log_file.lock().unwrap();
        *log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)?;
        Ok(())
    }

    fn format_log(&self, record: &Record) -> String {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        format!("{} [{}] - {}\n", timestamp, record.level(), record.args())
    }
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let message = self.format_log(record);

        // Write to file
        if let Ok(mut file) = self.log_file.lock() {
            let _ = file.write_all(message.as_bytes());
            let _ = file.flush();
        }

        // Write to stdout
        let _ = io::stdout().write_all(message.as_bytes());
        let _ = io::stdout().flush();
    }

    fn flush(&self) {
        if let Ok(mut file) = self.log_file.lock() {
            let _ = file.flush();
        }
        let _ = io::stdout().flush();
    }
}

extern "C" fn handle_sighup(_: c_int) {
    // This will be called when logrotate sends SIGHUP
    if let Some(logger) = LOGGER.lock().ok().and_then(|l| l.as_ref().cloned()) {
        let log_path = if cfg!(target_os = "macos") {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/root".to_string());
            Path::new(&home).join("Library/Logs/rmbt/rmbt_server.log")
        } else {
            Path::new("/var/log/rmbt/rmbt_server.log").to_path_buf()
        };
        let _ = logger.reopen_log_file(&log_path);
    }
}

lazy_static::lazy_static! {
    static ref LOGGER: Mutex<Option<Arc<FileLogger>>> = Mutex::new(None);
}

pub fn init_logger(level: LevelFilter) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Create log directory if it doesn't exist
    let log_dir = if cfg!(target_os = "macos") {
        // On macOS, use ~/Library/Logs/rmbt
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/root".to_string());
        Path::new(&home).join("Library/Logs/rmbt")
    } else {
        // Create PID file
        let pid = std::process::id();
        let pid_dir = Path::new("/run");
        if !pid_dir.exists() {
            std::fs::create_dir_all(pid_dir)?;
        }
        let pid_path = pid_dir.join("rmbt.pid");
        std::fs::write(&pid_path, pid.to_string())?;
        log::info!("PID file location: {}", pid_path.display());
        // On Linux/Unix, use /var/log/rmbt
        Path::new("/var/log/rmbt").to_path_buf()
    };

    // Create directory and all parent directories if they don't exist
    if !log_dir.exists() {
        fs::create_dir_all(&log_dir)?;
    }

    // Ensure we have write permissions
    let log_path = log_dir.join("rmbt_server.log");

    // Try to create the log file with proper permissions
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    // Create logger
    let logger = Arc::new(FileLogger {
        level,
        log_file: Arc::new(Mutex::new(log_file)),
    });

    // Store logger in global state
    *LOGGER.lock().unwrap() = Some(logger.clone());

    // Set up SIGHUP handler for log rotation
    unsafe {
        signal(SIGHUP, handle_sighup as usize);
    }

    log::set_boxed_logger(Box::new(logger))?;
    log::set_max_level(level);

    // Log initial message to verify logger is working
    log::info!("Logger initialized with level: {:?}", level);
    log::info!("Log file location: {}", log_path.display());

    Ok(())
}

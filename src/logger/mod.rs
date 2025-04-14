use log::LevelFilter;

pub fn init_logger(level: LevelFilter) {
    env_logger::Builder::new()
        .filter_level(level)
        .init();
} 
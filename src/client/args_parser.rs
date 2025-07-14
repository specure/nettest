use env_logger::Env;
use log::LevelFilter;

use crate::{client::client::ClientConfig, config::Config};

pub fn parse_args(args: Vec<String>, default_config: Config) -> Result<ClientConfig, anyhow::Error> {
    let thread_count = args
        .iter()
        .enumerate()
        .find_map(|(i, arg)| {
            if arg == "-t" {
                args.get(i + 1).and_then(|next| next.parse::<usize>().ok())
            } else if arg.starts_with("-t") {
                arg[2..].parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(default_config.client_thread_count);

    let server = args.iter()
        .enumerate()
        .find_map(|(i, arg)| {
            if arg == "-c" && i + 1 < args.len() {
                let next_arg = &args[i + 1];
                if next_arg.starts_with('-') {
                    None // Следующий аргумент - это флаг, значит адрес не указан
                } else {
                    Some(next_arg)
                }
            } else {
                None
            }
        })
        .ok_or("Server address is required".to_string());

    let use_tls = args.iter().any(|arg| arg == "-tls") || default_config.client_use_tls;
    let use_websocket = args.iter().any(|arg| arg == "-ws") || default_config.client_use_websocket;
    let log_level = args.iter().enumerate().find_map(|(i, arg)| {
        if arg == "-log" {
            args.get(i + 1)
                .and_then(|next| next.parse::<LevelFilter>().ok())
        }  else {
            Some(LevelFilter::Info)
        }
    });

    let graphs = args.iter().find_map(|arg| {
        if arg == "-g" {
            Some(true)
        } else {
            Some(false)
        }
    });

    if log_level.is_some() {
        env_logger::init_from_env(Env::default().filter_or(log_level.unwrap().to_string(), "info".to_string()));
    }

    let port = args.iter().enumerate().find_map(|(i, arg)| {
        if arg == "-p" {
            args.get(i + 1).and_then(|next| next.parse::<u16>().ok())
        } else {
            None
        }
    });


    let config = ClientConfig {
        use_tls: use_tls,
        use_websocket: use_websocket,
        graphs: graphs.unwrap_or(false),
        log: log_level,
        thread_count: thread_count,
        server: server.unwrap().to_string(),
        port: port.unwrap_or(5005),
        tls_port: port.unwrap_or(8080),
    };
    Ok(config)
}

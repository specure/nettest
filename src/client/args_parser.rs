use env_logger::Env;
use log::LevelFilter;

use crate::{client::client::ClientConfig, config::FileConfig, logger};

pub fn parse_args(args: Vec<String>, default_config: FileConfig) -> Result<ClientConfig, anyhow::Error> {
    let mut config = ClientConfig {
        use_tls: default_config.client_use_tls,
        use_websocket: default_config.client_use_websocket,
        graphs: false,
        log: Some(LevelFilter::Info),
        thread_count: default_config.client_thread_count,
        server: String::new(),
        port: default_config.server_tcp_port.parse().unwrap_or(5005),
        tls_port: default_config.server_tls_port.unwrap_or("443".to_string()).parse().unwrap(),
    };


    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-t" => {
                i += 1;
                if i < args.len() {
                    config.thread_count = args[i].parse()?;
                }
            }
            "-c" => {
                i += 1;
                if i < args.len() {
                    config.server = args[i].clone();
                } else {
                    return Err(anyhow::anyhow!("Server address is required"));
                }
            }
            "-tls" => {
                config.use_tls = true;
            }
            "-ws" => {
                config.use_websocket = true;
            }
            "-g" => {
                config.graphs = true;
            }
            "-log" => {
                i += 1;
                if i < args.len() {
                    match args[i].as_str() {
                        "info" => config.log = Some(LevelFilter::Info),
                        "debug" => config.log = Some(LevelFilter::Debug),
                        "trace" => config.log = Some(LevelFilter::Trace),
                        _ => return Err(anyhow::anyhow!("Invalid log level: {}", args[i])),
                    }
                }
            }
            "-p" => {
                i += 1;
                if i < args.len() {
                    config.port = args[i].parse()?;
                    config.tls_port = args[i].parse()?;
                }
            }
            "--help" | "-h" => {
                print_help();
                return Err(anyhow::anyhow!("Help printed"));
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown option: {}", args[i]));
            }
        }
        i += 1;
    }

    if config.server.is_empty() {
        return Err(anyhow::anyhow!("Server address is required"));
    }

    if config.log.is_some() {
        logger::init_logger(config.log.unwrap()).unwrap();
    }

    Ok(config)
}




pub fn print_help() {
    println!("==== Nettest Client ====");
    println!("Usage: nettest -c <server_address> [-t<num_threads>] [-ws] [-tls] ");
    println!("By default, nettest will connect to server on port :5005 for TCP or :443 fot TLS");
    println!("Usage: nettest -c 127.0.0.1 -ws -tls -t5");
    println!("-ws - use websocket");
    println!("-tls - use tls");
    println!("-log - `RUST_LOG=debug ./nettest 127.0.0.1  -t5 -tls -log`");
    println!("-t<num_threads> - number of threads");
    println!("-help - print help");
    println!("-h - print help");
    println!("-g - print graphs");
    println!("-p - port");
    println!("-e - encryption key");
    println!("-h - print help");
    println!("-h - print help");
}

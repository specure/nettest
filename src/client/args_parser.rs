use log::{debug, LevelFilter};

use crate::{client::{client::ClientConfig, control_server::get_best_measurement_server}, config::FileConfig, logger};

pub async fn parse_args(args: Vec<String>, default_config: FileConfig) -> Result<ClientConfig, anyhow::Error> {
    let mut config = ClientConfig {
        use_tls: default_config.client_use_tls,
        use_websocket: default_config.client_use_websocket,
        graphs: false,
        raw_output: false,
        log: None,
        thread_count: default_config.client_thread_count,
        server: None,
        port: default_config.server_tcp_port.parse().unwrap_or(5005),
        tls_port: default_config.server_tls_port.unwrap_or("443".to_string()).parse().unwrap(),
        x_nettest_client: default_config.x_nettest_client,
        control_server: default_config.control_server,
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
                if i + 1 < args.len() {
                    let next_arg = &args[i + 1];
                    if next_arg.starts_with('-') {
                        config.server = None;
                    } else {
                        i += 1;
                        config.server = Some(next_arg.clone());
                    }
                } else {
                    config.server = None;
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
            "-raw" => {
                config.raw_output = true;
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

    if config.log.is_some() {
        logger::init_logger(config.log.unwrap()).unwrap();
    }
    if config.server.is_none()  {
        debug!("No server address provided, using default");
        //TODO: verify tls
        let server = get_best_measurement_server(&config.x_nettest_client, &config.control_server).await?.ok_or_else(|| {
            println!("No server found, using default");
            anyhow::anyhow!("No server found")
        })?;
        let address = if server.web_address.is_empty() {
            server.ip_address.unwrap()
        } else {
            server.web_address.clone()
        };
        config.server = Some(address);
        let details = server.server_type_details;
        let rmbt_details = details.iter().find(|s| s.server_type == "RMBT");
        if rmbt_details.is_some() {
            config.port = rmbt_details.unwrap().port as u16;
            config.tls_port = rmbt_details.unwrap().port_ssl as u16;
        } 
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
    println!("-raw - output results in parseable format (ping/download/upload)");
    println!("-help - print help");
    println!("-h - print help");
    println!("-g - print graphs");
    println!("-p - port");
    println!("-e - encryption key");
    println!("-h - print help");
    println!("-h - print help");
}

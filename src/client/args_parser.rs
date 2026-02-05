use log::{debug, LevelFilter};

use crate::{client::{client::ClientConfig, control_server::get_best_measurement_server}, config::FileConfig, logger};

pub async fn parse_args(args: Vec<String>, default_config: FileConfig) -> Result<ClientConfig, anyhow::Error> {
    debug!("Default config: {:?}", default_config);

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
        save_results: false,
        signed_result: default_config.signed_result,
        client_uuid: default_config.client_uuid,
        git_hash: None,
        legacy: false,
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
                debug!("Using TLS");
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
            "-save" => {
                config.save_results = true;
            }
            "-signed" => {
                config.signed_result = true;
            }
            "-legacy" => {
                config.legacy = true;
            }
            "-git-hash" => {
                i += 1;
                if i < args.len() {
                    config.git_hash = Some(args[i].clone());
                }
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
                std::process::exit(0);
            }
            _ => {
                eprintln!("Error: Unknown option '{}'\n", args[i]);
                print_help();
                std::process::exit(1);
            }
        }
        i += 1;
    }

    if config.log.is_some() || default_config.logger != LevelFilter::Off {
        logger::init_logger(config.log.unwrap_or(default_config.logger)).unwrap();
    }
    if config.server.is_none()  {
        debug!("No server address provided, using default");
        //TODO: verify tls
        let server = get_best_measurement_server(&config.x_nettest_client, &config.control_server).await?.ok_or_else(|| {
            anyhow::anyhow!("No server found. Probably no running servers of version 2.0.0 or higher")
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
    println!("nettest - Network speed measurement client\n");
    println!("USAGE:");
    println!("    nettest                          Auto-discover server and run test");
    println!("    nettest -c [SERVER] [OPTIONS]    Connect to specific server\n");
    println!("EXAMPLES:");
    println!("    nettest                          Find nearest server automatically");
    println!("    nettest -c 192.168.1.100         Connect to server at 192.168.1.100");
    println!("    nettest -c example.com -tls      Connect using TLS encryption");
    println!("    nettest -c example.com -tls -ws  Connect using TLS over WebSocket\n");
    println!("OPTIONS:");
    println!("    -c [SERVER]     Run as client, optionally specify server address");
    println!("    -p PORT         Server port (default: 5005 for TCP, 443 for TLS)");
    println!("    -t THREADS      Number of parallel threads (default: from config)");
    println!("    -tls            Use TLS encryption");
    println!("    -ws             Use WebSocket protocol");
    println!("    -g              Display download/upload graphs");
    println!("    -raw            Output results in parseable format: ping/download/upload");
    println!("    -save           Save results to control server");
    println!("    -signed         Request signed result from server");
    println!("    -legacy         Use legacy PUT command instead of PUTTIMERESULT");
    println!("    -log LEVEL      Set log level: info, debug, trace");
    println!("    -h, --help      Show this help message");
}

use crate::{
    config::FileConfig, logger, mioserver::server::ServerConfig, tokio_server::{server_config::parse_listen_address, utils::user}
};

pub fn parse_args(
    args: Vec<String>,
    default_config: FileConfig,
) -> Result<ServerConfig, anyhow::Error> {
    let mut config = ServerConfig {
        tcp_address: parse_listen_address(&default_config.server_tcp_port).unwrap(),
        tls_address: parse_listen_address(&default_config.server_tls_port.unwrap_or("443".to_string())).unwrap(),
        cert_path: default_config.cert_path,
        key_path: default_config.key_path,
        num_workers: default_config.server_workers, 
        user: default_config.user,
        daemon: default_config.daemonize,
        version: Some("1.5.0".to_string()),
        secret_key: default_config.secret_key,
        log_level: None,
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-l" | "-L" => {
                i += 1;
                if i < args.len() {
                    let addr = parse_listen_address(&args[i]).unwrap();
                    if args[i - 1] == "-L" {
                        config.tls_address = addr;
                    } else {
                        config.tcp_address = addr;
                    }
                }
            }
            "-c" => {
                i += 1;
                if i < args.len() {
                    if config.cert_path.is_some() {
                        return Err(anyhow::anyhow!("Error: only one -c is allowed"));
                    }
                    config.cert_path = Some(args[i].clone());
                }
            }
            "-k" => {
                i += 1;
                if i < args.len() {
                    if config.key_path.is_some() {
                        return Err(anyhow::anyhow!("Error: only one -k is allowed"));
                    }
                    config.key_path = Some(args[i].clone());
                }
            }
            "-t" => {
                i += 1;
                if i < args.len() {
                    config.num_workers = Some(args[i].parse().unwrap());
                }
            }
            "-u" => {
                if i < args.len() {
                    user::UserPrivileges::check_root()?;
                    i += 1;
                    config.user = Some(args[i].clone());
                }
                i += 1;
            }
            "-d" => {
                config.daemon = true;
            }
            "-log" => {
                i += 1;
                if i < args.len() {
                    config.log_level = Some(args[i].parse().unwrap());
                }
            }
            "-e" => {
                i += 1;
                if i < args.len() {
                    config.secret_key = Some(args[i].clone());
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
    if config.log_level.is_some() {
        logger::init_logger(config.log_level.unwrap()).unwrap();
    }
    Ok(config)
}



fn print_help() {
    println!("==== Nettest Server ====");
    println!("By default, rmbtd will listen TCP on port 5005");
    println!("Usage: 'nettest -s' will listen on TCP on port 5005");
    println!("Usage: 'nettest -s -k privkey1.pem -c fullchain1.pem'  will listen on TCP and TLS on ports 5005 and 443");
    println!("Usage: nettest -s [-l <listen_address>] [-c <cert_path>] [-k <key_path>] [-t <num_threads>] [-u <user>] [-d] [-D] [-w] [-v <version>]");
    println!("command line arguments:\n");
    println!(" -l/-L  listen on (IP and) port; -L for SSL; default port is 5005, 443 for TLS");
    println!("        examples: \"443\",\"1.2.3.4:1234\",\"[2001:1234::567A]:1234\"");
    println!(" -c     path to SSL certificate in PEM format;");
    println!("        intermediate certificates following server cert in same file if needed");
    println!("        required\n");
    println!(" -k     path to SSL key file in PEM format; required\n");
    println!(" -t     number of worker threads to run for handling connections (default: 200)\n");
    println!(" -u     drop root privileges and setuid to specified user; must be root\n");
    println!(" -d     fork into background as daemon (no argument)\n");
    println!(" -log    log level: info, debug, trace\n");
    println!(" -e     encryption key for resut signature\n");
}

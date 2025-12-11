use crate::{
    config::FileConfig,
    logger,
    mioserver::{handlers::signed_result::generate_secret_key, server::ServerConfig},
    tokio_server::{server_config::parse_listen_address, utils::user},
};
use log::LevelFilter;

pub fn parse_args(
    args: Vec<String>,
    default_config: FileConfig,
) -> Result<ServerConfig, anyhow::Error> {
    let mut config = ServerConfig {
        tcp_address: parse_listen_address(&default_config.server_tcp_port).unwrap(),
        tls_address: parse_listen_address(
            &default_config.server_tls_port.unwrap_or("443".to_string()),
        )
        .unwrap(),
        cert_path: default_config.cert_path,
        key_path: default_config.key_path,
        num_workers: default_config.server_workers,
        user: default_config.user,
        daemon: default_config.daemonize,
        version: Some("2.0.0".to_string()),
        secret_key: generate_secret_key(),
        log_level: Some(default_config.logger),
        server_registration: default_config.server_registration,
        control_server: default_config.control_server,
        hostname: default_config.hostname,
        x_nettest_client: default_config.x_nettest_client,
        registration_token: default_config.registration_token,
        server_name: default_config.server_name,
        enable_mdns: false,
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
                    config.cert_path = Some(args[i].clone());
                }
            }
            "-k" => {
                i += 1;
                if i < args.len() {
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
            "-register" => {
                config.server_registration = true;
            }
            "-mdns" => {
                config.enable_mdns = true;
            }
            "--help" | "-h" => {
                print_help();
                return Err(anyhow::anyhow!("Help printed"));
            }
            _ => {
                print_help();
                return Err(anyhow::anyhow!(
                    "Unknown option: {}, use -h for help",
                    args[i]
                ));
            }
        }
        i += 1;
    }
    if config.log_level.is_some() && config.log_level.unwrap() != LevelFilter::Off {
        println!(
            "Initializing logger with level: {:?}",
            config.log_level.unwrap()
        );
        logger::init_logger(config.log_level.unwrap()).unwrap();
    }
    Ok(config)
}

fn print_help() {
    println!("==== Nettest Server ====");
    println!("By default, rmbtd will listen TCP on port 5005");
    println!("Usage: 'nettest -s' will listen on TCP on port 5005");
    println!("Usage: 'nettest -s -k privkey1.pem -c fullchain1.pem'  will listen on TCP and TLS on ports 5005 and 443");
    println!("Usage: nettest -s [-l <listen_address>] [-c <cert_path>] [-k <key_path>] [-t <num_threads>] [-u <user>] [-d] [-w] [-v <version>]");
    println!("command line arguments:\n");
    println!(" -l/-L  listen on (IP and) port; -L for SSL; default port is 5005, 443 for TLS");
    println!("        examples: \"443\",\"1.2.3.4:1234\",\"[2001:1234::567A]:1234\"");
    println!(" -c     path to SSL certificate in PEM format;");
    println!("        intermediate certificates following server cert in same file if needed");
    println!("        required for TLS\n");
    println!(" -k     path to SSL key file in PEM format; required for TLS\n");
    println!(" -u     drop root privileges and setuid to specified user; must be root\n");
    println!(" -d     fork into background as daemon (no argument)\n");
    println!(" -log    log level: info, debug, trace\n");
    println!(" -register  enable server registration\n");
    println!(" -mdns  enable mDNS service discovery for local network\n");
    println!("--help - print help");
    println!("-h - print help");
}

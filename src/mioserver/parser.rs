use crate::{
    config::FileConfig,
    logger,
    mioserver::{handlers::signed_result::generate_secret_key, server::ServerConfig},
    tokio_server::{ utils::user},
    config::parser::parse_listen_address,
};
use log::LevelFilter;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

pub fn parse_args(
    args: Vec<String>,
    default_config: FileConfig,
) -> Result<ServerConfig, anyhow::Error> {
    let mut config = ServerConfig {
        tcp_addresses: vec![],
        tls_addresses: vec![],
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
                        config.tls_addresses.push(addr);
                    } else {
                        config.tcp_addresses.push(addr);
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
    if config.log_level.is_some() && config.log_level.unwrap() != LevelFilter::Off {
        println!(
            "Initializing logger with level: {:?}",
            config.log_level.unwrap()
        );
        logger::init_logger(config.log_level.unwrap()).unwrap();
    }

    //add default addresses if args were not provided
    if config.tcp_addresses.is_empty() {
        config.tcp_addresses.push(SocketAddr::new(
            IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            default_config.server_tcp_port.parse().unwrap(),
        ));
        config.tcp_addresses.push(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            default_config.server_tcp_port.parse().unwrap(),
        ));
    }
    if config.tls_addresses.is_empty() {
        //keep this order to avoid conflicts with IPv4 addresses on unix
        config.tls_addresses.push(SocketAddr::new(
            IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            default_config
                .server_tls_port
                .clone()
                .unwrap_or("443".to_string())
                .parse()
                .unwrap(),
        ));
        config.tls_addresses.push(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            default_config
                .server_tls_port
                .unwrap_or("443".to_string())
                .parse()
                .unwrap(),
        ));
    }
    Ok(config)
}

fn print_help() {
    println!("nettest - Network speed measurement server\n");
    println!("USAGE:");
    println!("    nettest -s [OPTIONS]\n");
    println!("EXAMPLES:");
    println!("    nettest -s                                   Start TCP server on port 5005");
    println!("    nettest -s -k key.pem -c cert.pem            Start TCP + TLS server");
    println!("    nettest -s -l 8080 -L 8443                   Custom ports for TCP and TLS");
    println!("    nettest -s -d -u nobody                      Run as daemon with user 'nobody'\n");
    println!("OPTIONS:");
    println!("    -l ADDRESS      TCP listen address (default: 0.0.0.0:5005)");
    println!("                    Examples: \"5005\", \"192.168.1.1:5005\", \"[::]:5005\"");
    println!("    -L ADDRESS      TLS listen address (default: 0.0.0.0:443)");
    println!("                    Examples: \"443\", \"192.168.1.1:443\", \"[::]:443\"");
    println!("    -c PATH         Path to SSL certificate in PEM format (required for TLS)");
    println!("                    Include intermediate certs in same file if needed");
    println!("    -k PATH         Path to SSL private key in PEM format (required for TLS)");
    println!("    -t THREADS      Number of worker threads");
    println!("    -u USER         Drop privileges and run as specified user (requires root)");
    println!("    -d              Run as daemon in background");
    println!("    -log LEVEL      Set log level: info, debug, trace");
    println!("    -register       Enable server registration with control server");
    println!("    -mdns           Enable mDNS service discovery for local network");
    println!("    -h, --help      Show this help message");
}

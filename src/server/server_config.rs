use libc::{getpwnam, getuid, setgid, setuid};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::ffi::CString;
use std::fs;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::process;
use std::str::FromStr;

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerConfig {
    pub listen_addresses: Vec<SocketAddr>,
    pub ssl_listen_addresses: Vec<SocketAddr>,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
    pub num_threads: usize,
    pub user: Option<String>,
    pub daemon: bool,
    pub debug: bool,
    pub websocket: bool,
    pub version: Option<String>,
}

impl ServerConfig {
    pub fn from_args() -> Result<Self, Box<dyn Error + Send + Sync>> {
        let mut config = ServerConfig {
            listen_addresses: Vec::new(),
            ssl_listen_addresses: Vec::new(),
            cert_path: None,
            key_path: None,
            num_threads: 200, // Default value from C version
            user: None,
            daemon: false,
            debug: false,
            websocket: false,
            version: None,
        };

        let args: Vec<String> = std::env::args().collect();
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "-l" | "-L" => {
                    i += 1;
                    if i < args.len() {
                        let addr = parse_listen_address(&args[i])?;
                        if args[i-1] == "-L" {
                            config.ssl_listen_addresses.push(addr);
                        } else {
                            config.listen_addresses.push(addr);
                        }
                    }
                }
                "-c" => {
                    i += 1;
                    if i < args.len() {
                        if config.cert_path.is_some() {
                            eprintln!("Error: only one -c is allowed");
                            process::exit(1);
                        }
                        config.cert_path = Some(args[i].clone());
                    }
                }
                "-k" => {
                    i += 1;
                    if i < args.len() {
                        if config.key_path.is_some() {
                            eprintln!("Error: only one -k is allowed");
                            process::exit(1);
                        }
                        config.key_path = Some(args[i].clone());
                    }
                }
                "-t" => {
                    i += 1;
                    if i < args.len() {
                        config.num_threads = args[i].parse()?;
                    }
                }
                "-u" => {
                    i += 1;
                    if i < args.len() {
                        if unsafe { getuid() } != 0 {
                            eprintln!("Error: must be root to use option -u");
                            process::exit(1);
                        }
                        if config.user.is_some() {
                            eprintln!("Error: only one -u is allowed");
                            process::exit(1);
                        }
                        let username = CString::new(args[i].as_bytes())?;
                        let pw = unsafe { getpwnam(username.as_ptr()) };
                        if pw.is_null() {
                            eprintln!("Error: could not find user \"{}\"", args[i]);
                            process::exit(1);
                        }
                        unsafe {
                            setgid((*pw).pw_gid);
                            setuid((*pw).pw_uid);
                        }
                        config.user = Some(args[i].clone());
                    }
                }
                "-d" => {
                    config.daemon = true;
                }
                "-D" => {
                    config.debug = true;
                }
                "-w" => {
                    config.websocket = true;
                    println!("starting as websocket server");
                }
                "-v" => {
                    i += 1;
                    if i < args.len() {
                        if config.version.is_some() {
                            eprintln!("Error: only one -v is allowed");
                            process::exit(1);
                        }
                        if args[i] != "0.3" {
                            eprintln!("Error: unsupported version for backwards compatibility: >{}<", args[i]);
                            process::exit(1);
                        }
                        config.version = Some(args[i].clone());
                    }
                }
                "--help" | "-h" => {
                    print_help();
                    process::exit(0);
                }
                _ => {
                    eprintln!("Unknown option: {}", args[i]);
                    process::exit(1);
                }
            }
            i += 1;
        }

        // Validate required options
        if config.cert_path.is_none() || config.key_path.is_none() {
            eprintln!("Error: -c and -k options are required");
            process::exit(1);
        }

        if config.listen_addresses.is_empty() && config.ssl_listen_addresses.is_empty() {
            eprintln!("Error: at least one -l or -L option is required");
            process::exit(1);
        }

        Ok(config)
    }

    pub fn load_identity(&self) -> Result<tokio_native_tls::native_tls::Identity, Box<dyn Error + Send + Sync>> {
        let cert = fs::read(self.cert_path.as_ref().unwrap())?;
        let key = fs::read(self.key_path.as_ref().unwrap())?;
        Ok(tokio_native_tls::native_tls::Identity::from_pkcs8(&cert, &key)?)
    }
}

fn parse_listen_address(addr: &str) -> Result<SocketAddr, Box<dyn Error + Send + Sync>> {
    // Try IPv6 format: [::1]:8080
    if let Some(addr) = addr.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        if let Some((ip, port)) = addr.split_once(':') {
            let ip: Ipv6Addr = ip.parse()?;
            let port: u16 = port.parse()?;
            return Ok(SocketAddr::new(IpAddr::V6(ip), port));
        }
    }

    // Try IPv4 format: 127.0.0.1:8080
    if let Some((ip, port)) = addr.split_once(':') {
        let ip: std::net::Ipv4Addr = ip.parse()?;
        let port: u16 = port.parse()?;
        return Ok(SocketAddr::new(IpAddr::V4(ip), port));
    }

    // Try port only: 8080
    if let Ok(port) = addr.parse::<u16>() {
        return Ok(SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port));
    }

    Err("Invalid listen address format".into())
}

fn print_help() {
    println!("==== rmbtd ====");
    println!("command line arguments:\n");
    println!(" -l/-L  listen on (IP and) port; -L for SSL;");
    println!("        examples: \"443\",\"1.2.3.4:1234\",\"[2001:1234::567A]:1234\"");
    println!("        maybe specified multiple times; at least once\n");
    println!(" -c     path to SSL certificate in PEM format;");
    println!("        intermediate certificates following server cert in same file if needed");
    println!("        required\n");
    println!(" -k     path to SSL key file in PEM format; required\n");
    println!(" -t     number of worker threads to run for handling connections (default: 200)\n");
    println!(" -u     drop root privileges and setuid to specified user; must be root\n");
    println!(" -d     fork into background as daemon (no argument)\n");
    println!(" -D     enable debug logging (no argument)\n");
    println!(" -w     use as websocket server (no argument)\n");
    println!(" -v     behave as version (v) for serving very old clients");
    println!("        example: \"0.3\"\n");
    println!("Required are -c,-k and at least one -l/-L option");
}
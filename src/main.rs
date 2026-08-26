use log::{debug, info};
use tokio::signal;

use nettest::config::parser::read_config_file;
use nettest::mioserver::MioServer;
use std::error::Error as StdError;

#[tokio::main]
async fn main() -> Result<(), Box<dyn StdError + Send + Sync>> {
    let mut args: Vec<String> = std::env::args().collect();

    let config_result = read_config_file();
    if config_result.is_err() {
        return Err(config_result.err().unwrap().into());
    }
    let config = config_result.unwrap();
    if args.len() == 1 || args[1] == "-c" {
        args = args.iter().skip(1).map(|s| s.clone()).collect();
        nettest::client::client::client_run(args, config).await?;
        return Ok(());
    } else if args[1] == "-s" {
        debug!("args: {:?}", args);
        args = args.iter().skip(1).map(|s| s.clone()).collect();

        let wt_settings = (
            config.enable_webtransport,
            config.server_wt_port,
            config.cert_path.clone(),
            config.key_path.clone(),
        );
        let mut mio_server = MioServer::new(args, config)?;

        // QUIC/WebTransport QoS endpoint on its own UDP port, next to the RMBT
        // TCP listener: it is what lets a browser measure jitter / packet loss,
        // which need unreliable datagrams. Failing to start it is not fatal —
        // the control channel then answers "QoS unavailable" and everything
        // else still runs.
        let (wt_enabled, wt_port, wt_cert, wt_key) = wt_settings;
        if wt_enabled {
            match nettest::wtqos::endpoint::identity_from_config(
                wt_cert.as_deref(),
                wt_key.as_deref(),
            )
            .await
            {
                Ok((identity, self_signed)) => {
                    if let Err(e) = nettest::wtqos::start(wt_port, identity, self_signed) {
                        info!("WebTransport QoS endpoint not started: {e}");
                    }
                }
                Err(e) => info!("WebTransport QoS identity unavailable: {e}"),
            }
        }


        // Create separate thread for signal handling
        let shutdown_signal = mio_server.get_shutdown_signal();
        tokio::spawn(async move {
            signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
            info!("Ctrl+C received, shutting down server...");
            shutdown_signal.store(true, std::sync::atomic::Ordering::Relaxed);
        });

        mio_server.run()?;
        info!("Server stopping...");
        mio_server.shutdown().await?;
        info!("Server stopped");
    } else if args[1] == "-v" || args[1] == "--version" {
        println!("nettest {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    } else {
        let is_help = args[1] == "-h" || args[1] == "--help";
        if !is_help {
            eprintln!("Error: Invalid argument '{}'\n", args[1]);
        }
        println!("nettest - Network speed measurement tool\n");
        println!("USAGE:");
        println!("    nettest              Run client with auto-discovered server");
        println!("    nettest -c [OPTIONS] Run as client");
        println!("    nettest -s [OPTIONS] Run as server\n");
        println!("For detailed help:");
        println!("    nettest -c -h        Show client options");
        println!("    nettest -s -h        Show server options");
        println!("    nettest -v           Print version and exit");
        if !is_help {
            std::process::exit(1);
        }
    }
    Ok(())
}

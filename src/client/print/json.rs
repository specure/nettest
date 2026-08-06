use chrono::{SecondsFormat, Utc};
use serde::Serialize;

use crate::client::api::MeasurementResult;
use crate::client::client::ClientConfig;

/// A single nettest measurement in machine-readable form.
///
/// The document reports nettest's native units: milliseconds for latency and
/// jitter, bits per second for speed, percent for packet loss and bytes for the
/// transferred volume. A value that was not measured is omitted rather than
/// reported as zero, so a consumer can tell "not measured" apart from "measured
/// as zero". In `-legacy` mode, for example, jitter and packet loss are absent
/// because no VoIP and no UDP test ran.
#[derive(Debug, Serialize)]
pub struct JsonResult {
    #[serde(rename = "type")]
    pub result_type: &'static str,
    pub timestamp: String,
    pub client: JsonClient,
    pub server: JsonServer,
    pub protocol: &'static str,
    pub num_threads: usize,
    pub failed_threads: usize,
    pub ping: JsonPing,
    pub download: JsonTransfer,
    pub upload: JsonTransfer,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packet_loss_percent: Option<f64>,
}

/// The client that produced the measurement.
#[derive(Debug, Serialize)]
pub struct JsonClient {
    pub name: &'static str,
    pub version: &'static str,
}

/// The measurement server that the client connected to.
#[derive(Debug, Serialize)]
pub struct JsonServer {
    pub host: String,
    pub port: u16,
}

/// Latency figures of the ping phase.
#[derive(Debug, Serialize)]
pub struct JsonPing {
    /// Median round trip time in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<f64>,
    /// Mean jitter in milliseconds, measured by the VoIP test on an idle line.
    ///
    /// This is not the jitter under download or upload load; nettest does not
    /// measure that, so no such value is reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jitter_ms: Option<f64>,
}

/// Figures of one transfer direction.
#[derive(Debug, Serialize)]
pub struct JsonTransfer {
    /// Throughput in bits per second, as computed by the RMBT calculation.
    pub speed_bps: u64,
    /// Total bytes transferred by all threads in this direction.
    pub bytes_transferred: u64,
}

/// Rounds a value to `digits` decimal places.
fn round(value: f64, digits: u32) -> f64 {
    let factor = 10_f64.powi(digits as i32);
    (value * factor).round() / factor
}

/// Converts nanoseconds to milliseconds, keeping microsecond precision.
fn ns_to_ms(value: u64) -> f64 {
    round(value as f64 / 1_000_000.0, 3)
}

impl JsonResult {
    pub fn from_measurement(result: &MeasurementResult, config: &ClientConfig) -> Self {
        Self {
            result_type: "measurement",
            timestamp: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            client: JsonClient {
                name: "nettest",
                version: env!("CARGO_PKG_VERSION"),
            },
            server: JsonServer {
                host: config.server.clone().unwrap_or_default(),
                port: if config.use_tls {
                    config.tls_port
                } else {
                    config.port
                },
            },
            protocol: protocol_name(config),
            num_threads: result.num_threads,
            failed_threads: result.failed_threads,
            ping: JsonPing {
                latency_ms: result.ping_median_ns.map(ns_to_ms),
                jitter_ms: result.jitter_ns.map(ns_to_ms),
            },
            download: JsonTransfer {
                speed_bps: result.download_speed_bps.round() as u64,
                bytes_transferred: result.download_bytes,
            },
            upload: JsonTransfer {
                speed_bps: result.upload_speed_bps.round() as u64,
                bytes_transferred: result.upload_bytes,
            },
            packet_loss_percent: result.packet_loss_percent.map(|loss| round(loss as f64, 2)),
        }
    }
}

/// Names the transport that carried the measurement.
fn protocol_name(config: &ClientConfig) -> &'static str {
    match (config.use_websocket, config.use_tls) {
        (true, true) => "wss",
        (true, false) => "ws",
        (false, true) => "tls",
        (false, false) => "tcp",
    }
}

/// Prints the measurement as JSON on stdout.
///
/// Nothing else may be written to stdout in JSON mode, so that the output stays
/// parseable without filtering.
pub fn print_json_result(result: &MeasurementResult, config: &ClientConfig) {
    let document = JsonResult::from_measurement(result, config);
    match serde_json::to_string_pretty(&document) {
        Ok(json) => println!("{}", json),
        Err(e) => eprintln!("Failed to serialize the measurement: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ClientConfig {
        ClientConfig {
            server: Some("192.168.1.100".to_string()),
            port: 5005,
            ..Default::default()
        }
    }

    fn measurement() -> MeasurementResult {
        MeasurementResult {
            ping_median_ns: Some(12_340_000),
            download_speed_bps: 942_123_456.4,
            download_speed_gbps: 0.942_123_456,
            upload_speed_bps: 512_345_678.6,
            upload_speed_gbps: 0.512_345_678,
            download_bytes: 1_177_654_320,
            upload_bytes: 640_432_098,
            jitter_ns: Some(420_000),
            packet_loss_percent: Some(0.5),
            num_threads: 3,
            failed_threads: 0,
        }
    }

    fn to_value(result: &MeasurementResult, config: &ClientConfig) -> serde_json::Value {
        let document = JsonResult::from_measurement(result, config);
        serde_json::to_value(&document).unwrap()
    }

    #[test]
    fn reports_every_measured_value() {
        let json = to_value(&measurement(), &config());

        assert_eq!(json["type"], "measurement");
        assert_eq!(json["client"]["name"], "nettest");
        assert_eq!(json["client"]["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(json["server"]["host"], "192.168.1.100");
        assert_eq!(json["server"]["port"], 5005);
        assert_eq!(json["protocol"], "tcp");
        assert_eq!(json["num_threads"], 3);
        assert_eq!(json["failed_threads"], 0);
        assert!(json["timestamp"].as_str().unwrap().ends_with('Z'));
    }

    #[test]
    fn converts_nanoseconds_to_milliseconds() {
        let json = to_value(&measurement(), &config());

        assert_eq!(json["ping"]["latency_ms"], 12.34);
        assert_eq!(json["ping"]["jitter_ms"], 0.42);
    }

    #[test]
    fn reports_speed_in_bits_per_second_and_volume_in_bytes() {
        let json = to_value(&measurement(), &config());

        assert_eq!(json["download"]["speed_bps"], 942_123_456u64);
        assert_eq!(json["download"]["bytes_transferred"], 1_177_654_320u64);
        assert_eq!(json["upload"]["speed_bps"], 512_345_679u64);
        assert_eq!(json["upload"]["bytes_transferred"], 640_432_098u64);
    }

    #[test]
    fn reports_packet_loss_as_percent() {
        let json = to_value(&measurement(), &config());

        assert_eq!(json["packet_loss_percent"], 0.5);
    }

    #[test]
    fn omits_values_that_were_not_measured() {
        let mut result = measurement();
        result.jitter_ns = None;
        result.packet_loss_percent = None;
        result.ping_median_ns = None;

        let json = to_value(&result, &config());

        assert!(json.get("packet_loss_percent").is_none());
        assert!(json["ping"].get("jitter_ms").is_none());
        assert!(json["ping"].get("latency_ms").is_none());
    }

    #[test]
    fn names_the_transport_and_reports_the_port_in_use() {
        let mut tls = config();
        tls.use_tls = true;
        tls.tls_port = 443;
        let json = to_value(&measurement(), &tls);
        assert_eq!(json["protocol"], "tls");
        assert_eq!(json["server"]["port"], 443);

        let mut websocket = config();
        websocket.use_websocket = true;
        assert_eq!(to_value(&measurement(), &websocket)["protocol"], "ws");

        let mut secure_websocket = config();
        secure_websocket.use_websocket = true;
        secure_websocket.use_tls = true;
        assert_eq!(
            to_value(&measurement(), &secure_websocket)["protocol"],
            "wss"
        );
    }

    #[test]
    fn serializes_to_valid_pretty_json() {
        let document = JsonResult::from_measurement(&measurement(), &config());
        let json = serde_json::to_string_pretty(&document).unwrap();

        assert!(json.contains('\n'), "pretty output spans several lines");
        serde_json::from_str::<serde_json::Value>(&json).unwrap();
    }
}

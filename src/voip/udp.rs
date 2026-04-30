use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::voip::calculator::{calculate_qos, RtpQoSResult};
use crate::voip::rtp::{now_ns, PacketMap, RtpControlData, RtpPacket};
use crate::voip::VoipParams;

pub fn run_server_udp(
    params: VoipParams,
    ssrc: u32,
    client_ip: IpAddr,
    result_store: Arc<Mutex<Option<RtpQoSResult>>>,
) {
    let recv_addr = format!("0.0.0.0:{}", params.out_port);
    let recv_socket = match UdpSocket::bind(&recv_addr) {
        Ok(s) => s,
        Err(e) => {
            log::error!("VoIP server: failed to bind UDP {}: {}", recv_addr, e);
            *result_store.lock().unwrap() = Some(RtpQoSResult::default());
            return;
        }
    };
    recv_socket.set_read_timeout(Some(Duration::from_millis(50))).ok();

    let send_socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(e) => {
            log::error!("VoIP server: failed to bind send socket: {}", e);
            *result_store.lock().unwrap() = Some(RtpQoSResult::default());
            return;
        }
    };

    let client_udp_addr = SocketAddr::new(client_ip, params.in_port);
    let num_packets = params.num_packets();
    let payload_size = params.payload_size();
    let ts_increment = params.timestamp_increment();
    let delay = Duration::from_millis(params.delay_ms);
    let params_send = params.clone();

    // Send RTP stream to client in a separate thread
    let send_thread = thread::spawn(move || {
        let mut seq = params_send.initial_seq;
        let mut ts: u32 = 0;
        for i in 0..num_packets {
            let pkt = RtpPacket::new(seq, ts, ssrc, params_send.payload_type, i == 0, payload_size);
            send_socket.send_to(&pkt.to_bytes(), client_udp_addr).ok();
            seq = seq.wrapping_add(1);
            ts = ts.wrapping_add(ts_increment);
            thread::sleep(delay);
        }
    });

    // Receive client→server RTP packets
    let mut received: PacketMap = HashMap::new();
    let deadline = Instant::now() + Duration::from_millis(params.duration_ms + 3000);
    let mut buf = vec![0u8; 1500];

    loop {
        if Instant::now() >= deadline {
            break;
        }
        match recv_socket.recv_from(&mut buf) {
            Ok((n, _)) => {
                let ts_ns = now_ns();
                if let Some(pkt) = RtpPacket::from_bytes(&buf[..n]) {
                    received.entry(pkt.sequence_number).or_insert(RtpControlData {
                        sequence_number: pkt.sequence_number,
                        rtp_timestamp: pkt.timestamp,
                        received_ns: ts_ns,
                    });
                }
            }
            Err(_) => {}
        }
    }

    send_thread.join().ok();

    let result = calculate_qos(&received, params.initial_seq, params.sample_rate, params.buffer_ns);
    log::info!(
        "VoIP server result: received={} max_jitter={}ns mean_jitter={}ns",
        result.received_packets,
        result.max_jitter,
        result.mean_jitter,
    );
    *result_store.lock().unwrap() = Some(result);
}

pub fn run_client_udp(params: VoipParams, ssrc: u32, server_ip: IpAddr) -> RtpQoSResult {
    let recv_addr = format!("0.0.0.0:{}", params.in_port);
    let recv_socket = match UdpSocket::bind(&recv_addr) {
        Ok(s) => s,
        Err(e) => {
            log::error!("VoIP client: failed to bind UDP {}: {}", recv_addr, e);
            return RtpQoSResult::default();
        }
    };
    recv_socket.set_read_timeout(Some(Duration::from_millis(50))).ok();

    let send_socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(e) => {
            log::error!("VoIP client: failed to bind send socket: {}", e);
            return RtpQoSResult::default();
        }
    };

    let server_udp_addr = SocketAddr::new(server_ip, params.out_port);
    let num_packets = params.num_packets();
    let payload_size = params.payload_size();
    let ts_increment = params.timestamp_increment();
    let delay = Duration::from_millis(params.delay_ms);
    let params_send = params.clone();

    let send_thread = thread::spawn(move || {
        let mut seq = params_send.initial_seq;
        let mut ts: u32 = 0;
        for i in 0..num_packets {
            let pkt = RtpPacket::new(seq, ts, ssrc, params_send.payload_type, i == 0, payload_size);
            send_socket.send_to(&pkt.to_bytes(), server_udp_addr).ok();
            seq = seq.wrapping_add(1);
            ts = ts.wrapping_add(ts_increment);
            thread::sleep(delay);
        }
    });

    let mut received: PacketMap = HashMap::new();
    let deadline = Instant::now() + Duration::from_millis(params.duration_ms + 3000);
    let mut buf = vec![0u8; 1500];

    loop {
        if Instant::now() >= deadline {
            break;
        }
        match recv_socket.recv(&mut buf) {
            Ok(n) => {
                let ts_ns = now_ns();
                if let Some(pkt) = RtpPacket::from_bytes(&buf[..n]) {
                    received.entry(pkt.sequence_number).or_insert(RtpControlData {
                        sequence_number: pkt.sequence_number,
                        rtp_timestamp: pkt.timestamp,
                        received_ns: ts_ns,
                    });
                }
            }
            Err(_) => {}
        }
    }

    send_thread.join().ok();

    let result = calculate_qos(&received, params.initial_seq, params.sample_rate, params.buffer_ns);
    log::info!(
        "VoIP client result: received={} max_jitter={}ns mean_jitter={}ns",
        result.received_packets,
        result.max_jitter,
        result.mean_jitter,
    );
    result
}

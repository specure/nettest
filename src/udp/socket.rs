use std::collections::{BTreeMap, HashMap, HashSet};
use std::io;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::udp::payload::{
    rtts_to_json, UdpPayload, FLAG_AWAIT_RESPONSE, FLAG_ONE_DIRECTION, FLAG_RESPONSE,
    UDP_PAYLOAD_SIZE,
};
use crate::udp::result::{
    calculate_qos, packet_loss_rate_simple, PacketRecord, UdpQoSResult, UdpServerInResult,
    UdpServerOutResult,
};
use crate::udp::{DEFAULT_UDP_DELAY_NS, DEFAULT_UDP_TMAX_NS};

// Shared epoch for cross-thread nanosecond timestamps
fn now_ns(epoch: &Instant) -> u64 {
    epoch.elapsed().as_nanos() as u64
}

// ---------------------------------------------------------------------------
// Client — OUT test (client sends AWAIT_RESPONSE, server echoes RESPONSE)
// Full RFC 6673: per-packet Tmax, Undefined state, burst detection
// ---------------------------------------------------------------------------
pub fn run_client_udp_out(
    server_ip:   IpAddr,
    server_port: u16,
    num_packets: u32,
    delay_ns:    u64,
    tmax_ns:     u64,
    uuid:        [u8; 16],
) -> UdpQoSResult {
    let socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(e) => {
            log::error!("UDP OUT: failed to bind socket: {}", e);
            return UdpQoSResult::default();
        }
    };
    socket.set_read_timeout(Some(Duration::from_millis(50))).ok();

    let send_socket = match socket.try_clone() {
        Ok(s) => s,
        Err(e) => {
            log::error!("UDP OUT: failed to clone socket: {}", e);
            return UdpQoSResult::default();
        }
    };

    let server_addr = SocketAddr::new(server_ip, server_port);
    let epoch = Arc::new(Instant::now());
    let records: Arc<Mutex<HashMap<u32, PacketRecord>>> = Arc::new(Mutex::new(HashMap::new()));
    let duplicates: Arc<Mutex<HashSet<u32>>> = Arc::new(Mutex::new(HashSet::new()));

    let epoch_send   = epoch.clone();
    let records_send = records.clone();

    let send_thread = thread::spawn(move || {
        for i in 0..num_packets {
            let ts = now_ns(&epoch_send);
            {
                let mut map = records_send.lock().unwrap();
                map.insert(i, PacketRecord {
                    packet_number: i,
                    send_time_ns:  ts,
                    deadline_ns:   ts + tmax_ns,
                    return_time:   None,
                });
            }
            let payload = UdpPayload {
                communication_flag: FLAG_AWAIT_RESPONSE,
                packet_number:      i,
                uuid,
                timestamp_ns:       ts as i64,
            };
            send_socket.send_to(&payload.to_bytes(), server_addr).ok();
            thread::sleep(Duration::from_nanos(delay_ns));
        }
    });

    // Receive loop — runs until all per-packet deadlines have passed
    let loop_deadline = Instant::now() + Duration::from_nanos(num_packets as u64 * delay_ns + tmax_ns);
    let mut buf = [0u8; UDP_PAYLOAD_SIZE + 16];

    loop {
        if Instant::now() >= loop_deadline {
            break;
        }
        match socket.recv_from(&mut buf) {
            Ok((n, _)) => {
                let return_time = now_ns(&epoch);
                if let Some(payload) = UdpPayload::from_bytes(&buf[..n]) {
                    if payload.communication_flag != FLAG_RESPONSE {
                        continue;
                    }
                    let mut map = records.lock().unwrap();
                    if let Some(rec) = map.get_mut(&payload.packet_number) {
                        if rec.return_time.is_none() {
                            rec.return_time = Some(return_time);
                        } else {
                            duplicates.lock().unwrap().insert(payload.packet_number);
                        }
                    }
                }
            }
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                continue;
            }
            Err(_) => break,
        }
    }

    send_thread.join().ok();

    let map      = records.lock().unwrap();
    let dup_count = duplicates.lock().unwrap().len();
    let result = calculate_qos(&map, num_packets as usize, dup_count);
    log::info!(
        "UDP OUT result: received={} lost={} undefined={} loss_rate={}% max_burst={}",
        result.received_packets, result.lost_packets, result.undefined_packets,
        result.packet_loss_rate, result.max_burst_loss,
    );
    result
}

// ---------------------------------------------------------------------------
// Client — IN test (server sends, client receives and echoes)
// Count-based loss; RTTs come from server via GET UDPRESULT IN
// ---------------------------------------------------------------------------
pub fn run_client_udp_in(socket: &UdpSocket, in_num_packets: u32, tmax_ns: u64) -> UdpQoSResult {
    let mut received:   HashSet<u32> = HashSet::new();
    let mut duplicates: HashSet<u32> = HashSet::new();
    let mut buf = [0u8; UDP_PAYLOAD_SIZE + 16];

    let deadline = Instant::now()
        + Duration::from_nanos(in_num_packets as u64 * DEFAULT_UDP_DELAY_NS + tmax_ns);

    loop {
        if Instant::now() >= deadline {
            break;
        }
        if received.len() >= in_num_packets as usize {
            break;
        }
        match socket.recv_from(&mut buf) {
            Ok((n, src)) => {
                if let Some(mut payload) = UdpPayload::from_bytes(&buf[..n]) {
                    if payload.communication_flag != FLAG_ONE_DIRECTION
                        && payload.communication_flag != FLAG_AWAIT_RESPONSE
                    {
                        continue;
                    }
                    let pn = payload.packet_number;
                    if received.contains(&pn) {
                        duplicates.insert(pn);
                    } else {
                        received.insert(pn);
                        // Echo back for server-side RTT measurement
                        payload.communication_flag = FLAG_RESPONSE;
                        socket.send_to(&payload.to_bytes(), src).ok();
                    }
                }
            }
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                continue;
            }
            Err(_) => break,
        }
    }

    let received_count = received.len();
    let loss_rate = packet_loss_rate_simple(in_num_packets as usize, received_count);

    let result = UdpQoSResult {
        sent_packets:      in_num_packets as usize,
        received_packets:  received_count,
        lost_packets:      in_num_packets as usize - received_count,
        undefined_packets: 0,
        duplicate_packets: duplicates.len(),
        packet_loss_rate:  loss_rate,
        max_burst_loss:    0,
        loss_episodes:     0,
        rtt_avg_ns:        None,
        rtt_min_ns:        None,
        rtt_max_ns:        None,
        rtts_ns:           BTreeMap::new(),
    };
    log::info!(
        "UDP IN result: received={}/{} loss_rate={}%",
        received_count, in_num_packets, loss_rate,
    );
    result
}

// ---------------------------------------------------------------------------
// Server — OUT handler: receive packets, echo each back, count unique
// ---------------------------------------------------------------------------
pub fn start_server_udp_out(
    socket:      UdpSocket,
    num_packets: u32,
) -> Arc<Mutex<Option<UdpServerOutResult>>> {
    let result_store = Arc::new(Mutex::new(None));
    let store_clone  = result_store.clone();

    thread::spawn(move || {
        let port = socket.local_addr().map(|a| a.port()).unwrap_or(0);
        socket.set_read_timeout(Some(Duration::from_millis(100))).ok();

        let deadline = Instant::now()
            + Duration::from_nanos(num_packets as u64 * DEFAULT_UDP_DELAY_NS + DEFAULT_UDP_TMAX_NS);
        let mut received: HashSet<u32> = HashSet::new();
        let mut buf = [0u8; UDP_PAYLOAD_SIZE + 16];

        loop {
            if Instant::now() >= deadline {
                break;
            }
            if received.len() >= num_packets as usize {
                // Wait briefly for any final packets still in flight
                thread::sleep(Duration::from_millis(100));
                break;
            }
            match socket.recv_from(&mut buf) {
                Ok((n, src)) => {
                    if let Some(mut payload) = UdpPayload::from_bytes(&buf[..n]) {
                        if payload.communication_flag == FLAG_AWAIT_RESPONSE {
                            received.insert(payload.packet_number);
                            payload.communication_flag = FLAG_RESPONSE;
                            socket.send_to(&payload.to_bytes(), src).ok();
                        }
                    }
                }
                Err(_) => continue,
            }
        }

        log::info!("UDP server OUT: received={}/{}", received.len(), num_packets);
        *store_clone.lock().unwrap() = Some(UdpServerOutResult {
            received: received.len() as u32,
            port,
        });
    });

    result_store
}

// ---------------------------------------------------------------------------
// Server — IN handler: send packets to client, receive echoes for RTT
// ---------------------------------------------------------------------------
pub fn start_server_udp_in(
    client_ip:    IpAddr,
    client_port:  u16,
    num_packets:  u32,
) -> Arc<Mutex<Option<UdpServerInResult>>> {
    let result_store = Arc::new(Mutex::new(None));
    let store_clone  = result_store.clone();

    thread::spawn(move || {
        let socket = match UdpSocket::bind("0.0.0.0:0") {
            Ok(s) => s,
            Err(e) => {
                log::error!("UDP server IN: failed to bind: {}", e);
                *store_clone.lock().unwrap() = Some(UdpServerInResult {
                    received: 0,
                    port:     0,
                    rtts:     BTreeMap::new(),
                });
                return;
            }
        };

        let server_port = socket.local_addr().map(|a| a.port()).unwrap_or(0);
        socket.set_read_timeout(Some(Duration::from_millis(100))).ok();

        let client_addr = SocketAddr::new(client_ip, client_port);
        let epoch = Instant::now();
        let mut send_times: HashMap<u32, u64> = HashMap::new();

        // Send packets with delay
        for i in 0..num_packets {
            let ts = epoch.elapsed().as_nanos() as u64;
            send_times.insert(i, ts);
            let payload = UdpPayload {
                communication_flag: FLAG_ONE_DIRECTION,
                packet_number:      i,
                uuid:               [0u8; 16],
                timestamp_ns:       ts as i64,
            };
            socket.send_to(&payload.to_bytes(), client_addr).ok();
            thread::sleep(Duration::from_nanos(DEFAULT_UDP_DELAY_NS));
        }

        // Receive echoes for RTT measurement
        let echo_deadline = Instant::now() + Duration::from_nanos(DEFAULT_UDP_TMAX_NS);
        let mut rtts: BTreeMap<u32, u64> = BTreeMap::new();
        let mut buf = [0u8; UDP_PAYLOAD_SIZE + 16];

        loop {
            if Instant::now() >= echo_deadline {
                break;
            }
            if rtts.len() >= num_packets as usize {
                break;
            }
            match socket.recv_from(&mut buf) {
                Ok((n, _)) => {
                    if let Some(payload) = UdpPayload::from_bytes(&buf[..n]) {
                        if payload.communication_flag == FLAG_RESPONSE {
                            let pn = payload.packet_number;
                            if !rtts.contains_key(&pn) {
                                if let Some(&send_ts) = send_times.get(&pn) {
                                    let rtt = (epoch.elapsed().as_nanos() as u64)
                                        .saturating_sub(send_ts);
                                    rtts.insert(pn, rtt);
                                }
                            }
                        }
                    }
                }
                Err(_) => continue,
            }
        }

        log::info!(
            "UDP server IN: echoes={}/{} json={}",
            rtts.len(), num_packets,
            rtts_to_json(&rtts),
        );
        *store_clone.lock().unwrap() = Some(UdpServerInResult {
            received: rtts.len() as u32,
            port:     server_port,
            rtts,
        });
    });

    result_store
}

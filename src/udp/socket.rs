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
use crate::udp::server::SharedUdpServer;
use crate::udp::{DEFAULT_UDP_DELAY_NS, DEFAULT_UDP_SERVER_PORT, DEFAULT_UDP_TMAX_NS};

fn now_ns(epoch: &Instant) -> u64 {
    epoch.elapsed().as_nanos() as u64
}

// ---------------------------------------------------------------------------
// Client — OUT test (sends AWAIT_RESPONSE, server echoes RESPONSE)
// Full RFC 6673: per-packet Tmax, Undefined, burst detection
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
        Err(e) => { log::error!("UDP OUT: bind failed: {}", e); return UdpQoSResult::default(); }
    };
    socket.set_read_timeout(Some(Duration::from_millis(50))).ok();

    let send_socket = match socket.try_clone() {
        Ok(s) => s,
        Err(e) => { log::error!("UDP OUT: clone failed: {}", e); return UdpQoSResult::default(); }
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
            records_send.lock().unwrap().insert(i, PacketRecord {
                packet_number: i,
                send_time_ns:  ts,
                deadline_ns:   ts + tmax_ns,
                return_time:   None,
            });
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

    let loop_deadline = Instant::now() + Duration::from_nanos(num_packets as u64 * delay_ns + tmax_ns);
    let mut buf = [0u8; UDP_PAYLOAD_SIZE + 16];

    loop {
        if Instant::now() >= loop_deadline { break; }
        match socket.recv_from(&mut buf) {
            Ok((n, _)) => {
                let return_time = now_ns(&epoch);
                if let Some(p) = UdpPayload::from_bytes(&buf[..n]) {
                    if p.communication_flag != FLAG_RESPONSE { continue; }
                    let mut map = records.lock().unwrap();
                    if let Some(rec) = map.get_mut(&p.packet_number) {
                        if rec.return_time.is_none() {
                            rec.return_time = Some(return_time);
                        } else {
                            duplicates.lock().unwrap().insert(p.packet_number);
                        }
                    }
                }
            }
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => continue,
            Err(_) => break,
        }
    }

    send_thread.join().ok();

    let map = records.lock().unwrap();
    let dup_count = duplicates.lock().unwrap().len();
    let result = calculate_qos(&map, num_packets as usize, dup_count);
    log::info!(
        "UDP OUT: received={} lost={} undefined={} loss={}% burst={}",
        result.received_packets, result.lost_packets, result.undefined_packets,
        result.packet_loss_rate, result.max_burst_loss,
    );
    result
}

// ---------------------------------------------------------------------------
// Client — IN test (server sends, client receives and echoes)
// ---------------------------------------------------------------------------
pub fn run_client_udp_in(socket: &UdpSocket, in_num_packets: u32, tmax_ns: u64) -> UdpQoSResult {
    let mut received:   HashSet<u32> = HashSet::new();
    let mut duplicates: HashSet<u32> = HashSet::new();
    let mut buf = [0u8; UDP_PAYLOAD_SIZE + 16];
    let deadline = Instant::now()
        + Duration::from_nanos(in_num_packets as u64 * DEFAULT_UDP_DELAY_NS + tmax_ns);

    loop {
        if Instant::now() >= deadline { break; }
        if received.len() >= in_num_packets as usize { break; }
        match socket.recv_from(&mut buf) {
            Ok((n, src)) => {
                if let Some(mut p) = UdpPayload::from_bytes(&buf[..n]) {
                    if p.communication_flag != FLAG_ONE_DIRECTION
                        && p.communication_flag != FLAG_AWAIT_RESPONSE
                    { continue; }
                    let pn = p.packet_number;
                    if received.contains(&pn) {
                        duplicates.insert(pn);
                    } else {
                        received.insert(pn);
                        p.communication_flag = FLAG_RESPONSE;
                        socket.send_to(&p.to_bytes(), src).ok();
                    }
                }
            }
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => continue,
            Err(_) => break,
        }
    }

    let received_count = received.len();
    log::info!("UDP IN: received={}/{} loss={}%", received_count, in_num_packets,
        packet_loss_rate_simple(in_num_packets as usize, received_count));

    UdpQoSResult {
        sent_packets:      in_num_packets as usize,
        received_packets:  received_count,
        lost_packets:      in_num_packets as usize - received_count,
        undefined_packets: 0,
        duplicate_packets: duplicates.len(),
        packet_loss_rate:  packet_loss_rate_simple(in_num_packets as usize, received_count),
        max_burst_loss:    0,
        loss_episodes:     0,
        rtt_avg_ns:        None,
        rtt_min_ns:        None,
        rtt_max_ns:        None,
        rtts_ns:           BTreeMap::new(),
    }
}

// ---------------------------------------------------------------------------
// Server — OUT: receives packets via channel, echoes via shared socket
// ---------------------------------------------------------------------------
pub fn start_server_udp_out(
    uuid:       [u8; 16],
    num_packets: u32,
    udp_server: Arc<SharedUdpServer>,
) -> Arc<Mutex<Option<UdpServerOutResult>>> {
    let result_store = Arc::new(Mutex::new(None));
    let store_clone  = result_store.clone();
    let rx           = udp_server.register_udp_out(uuid);
    let socket       = udp_server.socket.clone();

    thread::spawn(move || {
        let deadline = Instant::now()
            + Duration::from_nanos(num_packets as u64 * DEFAULT_UDP_DELAY_NS + DEFAULT_UDP_TMAX_NS);
        let mut received: HashSet<u32> = HashSet::new();

        loop {
            if Instant::now() >= deadline { break; }
            if received.len() >= num_packets as usize {
                thread::sleep(Duration::from_millis(100));
                break;
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok((p, src, _recv_ns)) if p.communication_flag == FLAG_AWAIT_RESPONSE => {
                    received.insert(p.packet_number);
                    let mut echo = p;
                    echo.communication_flag = FLAG_RESPONSE;
                    socket.send_to(&echo.to_bytes(), src).ok();
                }
                _ => {}
            }
        }

        udp_server.unregister_udp_out(&uuid);
        log::info!("UDP server OUT: received={}/{}", received.len(), num_packets);
        *store_clone.lock().unwrap() = Some(UdpServerOutResult {
            received: received.len() as u32,
            port:     0, // filled in by handler from state.udp_port
        });
    });

    result_store
}

// ---------------------------------------------------------------------------
// Server — IN: sends packets via shared socket, receives echoes via channel
// ---------------------------------------------------------------------------
pub fn start_server_udp_in(
    client_ip:   IpAddr,
    client_port: u16,
    num_packets: u32,
    udp_server:  Arc<SharedUdpServer>,
) -> Arc<Mutex<Option<UdpServerInResult>>> {
    let result_store = Arc::new(Mutex::new(None));
    let store_clone  = result_store.clone();
    let client_addr  = SocketAddr::new(client_ip, client_port);
    let rx           = udp_server.register_udp_in(client_addr);
    let socket       = udp_server.socket.clone();

    thread::spawn(move || {
        let mut send_times: HashMap<u32, u64> = HashMap::new();

        for i in 0..num_packets {
            // Use SystemTime to match recv_ns clock in dispatch thread (RFC 2330)
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
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

        let echo_deadline = Instant::now() + Duration::from_nanos(DEFAULT_UDP_TMAX_NS);
        let mut rtts: BTreeMap<u32, u64> = BTreeMap::new();

        loop {
            if Instant::now() >= echo_deadline { break; }
            if rtts.len() >= num_packets as usize { break; }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok((p, _, recv_ns)) if p.communication_flag == FLAG_RESPONSE => {
                    // recv_ns from dispatch — RFC 2330 compliant timestamp for RTT
                    rtts.entry(p.packet_number).or_insert_with(|| {
                        recv_ns.saturating_sub(
                            send_times.get(&p.packet_number).copied().unwrap_or(recv_ns)
                        )
                    });
                }
                _ => {}
            }
        }

        udp_server.unregister_udp_in(&client_addr);
        log::info!("UDP server IN: echoes={}/{} rtts={}", rtts.len(), num_packets, rtts_to_json(&rtts));
        *store_clone.lock().unwrap() = Some(UdpServerInResult {
            received: rtts.len() as u32,
            port:     0, // filled in by handler from state.udp_port
            rtts,
        });
    });

    result_store
}

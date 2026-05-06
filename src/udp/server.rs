use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::udp::payload::{UdpPayload, FLAG_AWAIT_RESPONSE, FLAG_RESPONSE, UDP_PAYLOAD_SIZE};

const RTP_MIN_SIZE: usize = 12;

/// Single UDP socket shared across all connections.
/// Dispatches packets to the correct handler by SSRC (VoIP) or UUID/addr (packet loss).
pub struct SharedUdpServer {
    pub socket: Arc<UdpSocket>,
    // VoIP OUT (client→server): SSRC → (raw bytes, src, recv_ns)
    rtp: Arc<Mutex<HashMap<u32, Sender<(Vec<u8>, SocketAddr, u64)>>>>,
    // UDP loss OUT (client→server, AWAIT_RESPONSE): UUID → (payload, src, recv_ns)
    udp_out: Arc<Mutex<HashMap<[u8; 16], Sender<(UdpPayload, SocketAddr, u64)>>>>,
    // UDP loss IN echoes (client→server, RESPONSE): client addr → (payload, src, recv_ns)
    udp_in: Arc<Mutex<HashMap<SocketAddr, Sender<(UdpPayload, SocketAddr, u64)>>>>,
}

impl SharedUdpServer {
    pub fn new(port: u16) -> io::Result<Arc<Self>> {
        let socket = UdpSocket::bind(format!("0.0.0.0:{}", port))?;
        socket.set_read_timeout(Some(Duration::from_millis(50)))?;
        log::info!("Shared UDP server bound to 0.0.0.0:{}", port);

        let srv = Arc::new(Self {
            socket: Arc::new(socket),
            rtp:     Arc::new(Mutex::new(HashMap::new())),
            udp_out: Arc::new(Mutex::new(HashMap::new())),
            udp_in:  Arc::new(Mutex::new(HashMap::new())),
        });

        let srv_clone = srv.clone();
        thread::spawn(move || srv_clone.dispatch_loop());

        Ok(srv)
    }

    // ---- VoIP ----------------------------------------------------------------

    pub fn register_rtp(&self, ssrc: u32) -> Receiver<(Vec<u8>, SocketAddr, u64)> {
        let (tx, rx) = channel();
        self.rtp.lock().unwrap().insert(ssrc, tx);
        rx
    }
    pub fn unregister_rtp(&self, ssrc: u32) {
        self.rtp.lock().unwrap().remove(&ssrc);
    }

    // ---- UDP loss OUT --------------------------------------------------------

    pub fn register_udp_out(&self, uuid: [u8; 16]) -> Receiver<(UdpPayload, SocketAddr, u64)> {
        let (tx, rx) = channel();
        self.udp_out.lock().unwrap().insert(uuid, tx);
        rx
    }
    pub fn unregister_udp_out(&self, uuid: &[u8; 16]) {
        self.udp_out.lock().unwrap().remove(uuid);
    }

    // ---- UDP loss IN ---------------------------------------------------------

    pub fn register_udp_in(&self, client_addr: SocketAddr) -> Receiver<(UdpPayload, SocketAddr, u64)> {
        let (tx, rx) = channel();
        self.udp_in.lock().unwrap().insert(client_addr, tx);
        rx
    }
    pub fn unregister_udp_in(&self, client_addr: &SocketAddr) {
        self.udp_in.lock().unwrap().remove(client_addr);
    }

    // ---- Dispatch loop -------------------------------------------------------

    fn dispatch_loop(&self) {
        let mut buf = vec![0u8; 1500];
        loop {
            match self.socket.recv_from(&mut buf) {
                Ok((n, src)) if n > 0 => {
                    // RFC 2330/3393: timestamp as close to wire as possible
                    let recv_ns = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos() as u64;
                    self.dispatch(&buf[..n], src, recv_ns);
                }
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
                _ => {}
            }
        }
    }

    fn dispatch(&self, buf: &[u8], src: SocketAddr, recv_ns: u64) {
        match buf[0] {
            // RTP packet (V=2 means top 2 bits = 10, i.e. byte >= 0x80)
            b if b >= 0x80 && buf.len() >= RTP_MIN_SIZE => {
                let ssrc = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);
                let tx = self.rtp.lock().unwrap().get(&ssrc).cloned();
                if let Some(tx) = tx { tx.send((buf.to_vec(), src, recv_ns)).ok(); }
            }
            // UDP loss OUT: new packet from client
            FLAG_AWAIT_RESPONSE if buf.len() >= UDP_PAYLOAD_SIZE => {
                if let Some(p) = UdpPayload::from_bytes(buf) {
                    let tx = self.udp_out.lock().unwrap().get(&p.uuid).cloned();
                    if let Some(tx) = tx { tx.send((p, src, recv_ns)).ok(); }
                }
            }
            // UDP loss IN: echo from client
            FLAG_RESPONSE if buf.len() >= UDP_PAYLOAD_SIZE => {
                if let Some(p) = UdpPayload::from_bytes(buf) {
                    let tx = self.udp_in.lock().unwrap().get(&src).cloned();
                    if let Some(tx) = tx { tx.send((p, src, recv_ns)).ok(); }
                }
            }
            _ => {}
        }
    }
}

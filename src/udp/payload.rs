pub const UDP_PAYLOAD_SIZE: usize = 29;

pub const FLAG_HOLE_PUNCH:     u8 = 0; // client→server before IN test to open NAT mapping
pub const FLAG_ONE_DIRECTION:  u8 = 1;
pub const FLAG_RESPONSE:       u8 = 2;
pub const FLAG_AWAIT_RESPONSE: u8 = 3;

#[derive(Debug, Clone)]
pub struct UdpPayload {
    pub communication_flag: u8,
    pub packet_number:      u32,
    pub uuid:               [u8; 16],
    pub timestamp_ns:       i64,
}

impl UdpPayload {
    pub fn to_bytes(&self) -> [u8; UDP_PAYLOAD_SIZE] {
        let mut buf = [0u8; UDP_PAYLOAD_SIZE];
        buf[0] = self.communication_flag;
        buf[1..5].copy_from_slice(&self.packet_number.to_be_bytes());
        buf[5..21].copy_from_slice(&self.uuid);
        buf[21..29].copy_from_slice(&self.timestamp_ns.to_be_bytes());
        buf
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < UDP_PAYLOAD_SIZE {
            return None;
        }
        Some(Self {
            communication_flag: data[0],
            packet_number:      u32::from_be_bytes([data[1], data[2], data[3], data[4]]),
            uuid:               data[5..21].try_into().ok()?,
            timestamp_ns:       i64::from_be_bytes([
                data[21], data[22], data[23], data[24],
                data[25], data[26], data[27], data[28],
            ]),
        })
    }
}

pub fn random_uuid() -> [u8; 16] {
    let mut uuid = [0u8; 16];
    for b in uuid.iter_mut() {
        *b = fastrand::u8(..);
    }
    uuid
}

pub fn rtts_to_json(rtts: &std::collections::BTreeMap<u32, u64>) -> String {
    if rtts.is_empty() {
        return "{}".to_string();
    }
    let entries: Vec<String> = rtts.iter().map(|(k, v)| format!("\"{}\":{}", k, v)).collect();
    format!("{{{}}}", entries.join(","))
}

pub fn rtts_from_json(json: &str) -> std::collections::BTreeMap<u32, u64> {
    let mut map = std::collections::BTreeMap::new();
    let s = json.trim().trim_start_matches('{').trim_end_matches('}');
    if s.is_empty() {
        return map;
    }
    for entry in s.split(',') {
        let parts: Vec<&str> = entry.splitn(2, ':').collect();
        if parts.len() == 2 {
            let k = parts[0].trim().trim_matches('"').parse::<u32>().ok();
            let v = parts[1].trim().parse::<u64>().ok();
            if let (Some(k), Some(v)) = (k, v) {
                map.insert(k, v);
            }
        }
    }
    map
}

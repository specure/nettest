use std::fs;
use std::io::{self, BufRead};
use log::{info, error};

const MAX_SECRET_KEY_LINE_LENGTH: usize = 256;

pub struct SecretKey {
    pub key: String,
    pub label: String,
}

pub fn read_secret_keys(path: &str) -> io::Result<Vec<SecretKey>> {
    info!("Opening secret keys file: {}", path);
    
    let file = fs::File::open(path)?;
    let reader = io::BufReader::new(file);
    let mut keys = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        
        // Skip empty lines or comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Split on first space to separate key and label
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        let (key, label) = match parts.len() {
            1 => (parts[0].to_string(), parts[0].to_string()),
            2 => (parts[0].to_string(), parts[1].to_string()),
            _ => continue,
        };

        // Skip if key is too short
        if key.len() < 5 {
            continue;
        }

        keys.push(SecretKey { key, label });
    }

    info!("Read {} secret keys from file", keys.len());
    Ok(keys)
}

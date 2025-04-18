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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_read_secret_keys() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "key1 label1").unwrap();
        writeln!(temp_file, "key2 label2").unwrap();
        writeln!(temp_file, "").unwrap(); // Empty line
        writeln!(temp_file, "# Comment").unwrap();
        writeln!(temp_file, "key3").unwrap(); // No label
        writeln!(temp_file, "key4 label4").unwrap();

        let keys = read_secret_keys(temp_file.path().to_str().unwrap()).unwrap();
        assert_eq!(keys.len(), 4);
        
        assert_eq!(keys[0].key, "key1");
        assert_eq!(keys[0].label, "label1");
        
        assert_eq!(keys[1].key, "key2");
        assert_eq!(keys[1].label, "label2");
        
        assert_eq!(keys[2].key, "key3");
        assert_eq!(keys[2].label, "key3");
        
        assert_eq!(keys[3].key, "key4");
        assert_eq!(keys[3].label, "label4");
    }
} 
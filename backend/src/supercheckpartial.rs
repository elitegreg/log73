use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

const MAX_MATCHES: usize = 100;

#[derive(Clone, Debug, Default)]
pub struct SuperCheckPartial {
    callsigns: Vec<String>,
}

impl SuperCheckPartial {
    pub fn load_dir(data_dir: &Path) -> std::io::Result<Self> {
        let path = data_dir.join("MASTER.SCP");
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut callsigns = Vec::new();

        for line in reader.lines() {
            let line = line?;
            let callsign = line.trim();
            if callsign.is_empty() || callsign.starts_with('!') || callsign.starts_with('#') {
                continue;
            }
            callsigns.push(callsign.to_uppercase());
        }

        Ok(Self { callsigns })
    }

    pub fn len(&self) -> usize {
        self.callsigns.len()
    }

    pub fn matches(&self, query: &str) -> Vec<String> {
        let query = query.trim().to_uppercase();
        if query.len() < 3 {
            return Vec::new();
        }

        self.callsigns
            .iter()
            .filter(|callsign| callsign.contains(&query))
            .take(MAX_MATCHES)
            .cloned()
            .collect()
    }
}

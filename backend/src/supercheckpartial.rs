use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

#[derive(Clone, Debug, Default)]
pub struct SuperCheckPartial {
    callsigns: Vec<String>,
}

impl SuperCheckPartial {
    pub fn load_file(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let file = File::open(path.as_ref())?;
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

    pub fn callsigns(&self) -> &[String] {
        &self.callsigns
    }
}

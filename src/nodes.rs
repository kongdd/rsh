use std::{collections::BTreeMap, fs, io::ErrorKind, path::PathBuf, time::SystemTime};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Default, Deserialize, Serialize)]
struct Record {
    last_seen: Option<u64>,
}

pub struct Nodes {
    path: PathBuf,
    records: BTreeMap<String, Record>,
}

impl Nodes {
    pub fn load(path: PathBuf) -> Result<Self> {
        let records = match fs::read(&path) {
            Ok(data) => {
                serde_json::from_slice(&data).with_context(|| path.display().to_string())?
            }
            Err(e) if e.kind() == ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(e).with_context(|| path.display().to_string()),
        };
        Ok(Self { path, records })
    }

    pub fn contains(&self, name: &str) -> bool {
        self.records.contains_key(name)
    }

    pub fn list(&self) -> impl Iterator<Item = (&String, Option<u64>)> {
        self.records
            .iter()
            .map(|(name, record)| (name, record.last_seen))
    }

    pub fn add(&mut self, name: String) -> Result<bool> {
        if self.records.contains_key(&name) {
            return Ok(false);
        }
        self.records.insert(name, Record::default());
        self.save()?;
        Ok(true)
    }

    pub fn remove(&mut self, name: &str) -> Result<bool> {
        let changed = self.records.remove(name).is_some();
        if changed {
            self.save()?;
        }
        Ok(changed)
    }

    pub fn seen(&mut self, name: &str) -> Result<()> {
        let Some(record) = self.records.get_mut(name) else {
            return Ok(());
        };
        record.last_seen = Some(SystemTime::UNIX_EPOCH.elapsed()?.as_secs());
        self.save()
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&self.records)?)?;
        fs::rename(tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Nodes;

    #[test]
    fn persists_nodes() {
        let path = std::env::temp_dir().join(format!("rsh-store-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let mut nodes = Nodes::load(path.clone()).unwrap();
        assert!(nodes.add("mac".into()).unwrap());
        nodes.seen("mac").unwrap();
        assert!(Nodes::load(path.clone()).unwrap().contains("mac"));
        assert!(nodes.remove("mac").unwrap());
        assert!(!Nodes::load(path.clone()).unwrap().contains("mac"));

        std::fs::remove_file(path).unwrap();
    }
}

use std::collections::HashMap;
use std::fs;

#[derive(Debug, Default, Clone)]
pub struct AppConfig {
    values: HashMap<String, String>,
}

impl AppConfig {
    pub fn from_file(path: &str) -> Result<Self, String> {
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut values = HashMap::new();
        for (idx, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let trimmed = trimmed.strip_prefix("export ").unwrap_or(trimmed);
            let Some((key, value)) = trimmed.split_once('=') else {
                return Err(format!("Invalid config line {}: {}", idx + 1, line));
            };
            let key = key.trim();
            let mut value = value.trim().to_string();
            if (value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\''))
            {
                value = value[1..value.len() - 1].to_string();
            }
            values.insert(key.to_string(), value);
        }
        Ok(Self { values })
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.values.get(key).cloned()
    }
}

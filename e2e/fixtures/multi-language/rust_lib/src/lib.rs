use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Greeting {
    pub message: String,
    pub language: String,
}

impl Greeting {
    pub fn new(message: &str, language: &str) -> Self {
        Self {
            message: message.to_string(),
            language: language.to_string(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_greeting() {
        let greeting = Greeting::new("Hello", "Rust");
        let json = greeting.to_json();
        assert!(json.contains("Hello"));
        assert!(json.contains("Rust"));
    }
}

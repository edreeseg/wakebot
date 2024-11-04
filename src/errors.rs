use std::error::Error;
use std::fmt;

pub struct WakeBotError {
    details: String,
}

impl WakeBotError {
    pub fn new(msg: &str) -> WakeBotError {
        WakeBotError {
            details: msg.to_string(),
        }
    }
}

impl fmt::Display for WakeBotError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.details)
    }
}

impl fmt::Debug for WakeBotError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.details)
    }
}

impl Error for WakeBotError {
    fn description(&self) -> &str {
        &self.details
    }
}

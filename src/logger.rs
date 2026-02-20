use colored::Colorize;
use std::sync::atomic::{AtomicBool, Ordering};

static VERBOSE: AtomicBool = AtomicBool::new(false);

pub struct Logger;

#[allow(dead_code)]
impl Logger {
    /// enable or disable verbose (debug) mode
    pub fn set_verbose(value: bool) {
        VERBOSE.store(value, Ordering::SeqCst);
    }

    pub fn is_verbose() -> bool {
        VERBOSE.load(Ordering::SeqCst)
    }

    pub fn info(message: &str) {
        println!("{} {}", "[INFO]".cyan().bold(), message);
    }

    pub fn warn(message: &str) {
        eprintln!("{} {}", "[WARN]".yellow().bold(), message);
    }

    pub fn error(message: &str) {
        eprintln!("{} {}", "[ERROR]".red().bold(), message);
    }

    pub fn debug(message: &str) {
        if Logger::is_verbose() {
            println!("{} {}", "[DEBUG]".blue().bold(), message);
        }
    }

    pub fn success(message: &str) {
        println!("{} {}", "[SUCCESS]".green().bold(), message);
    }
}

#[cfg(test)]
#[path = "logger.test.rs"]
mod tests;

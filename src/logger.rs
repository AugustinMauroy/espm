use colored::Colorize;

pub struct Logger;

#[allow(dead_code)]
impl Logger {
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
        println!("{} {}", "[DEBUG]".blue().bold(), message);
    }

    pub fn success(message: &str) {
        println!("{} {}", "[SUCCESS]".green().bold(), message);
    }
}

#[cfg(test)]
#[path = "logger.test.rs"]
mod tests;

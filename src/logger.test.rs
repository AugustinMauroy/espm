use super::Logger;

#[test]
fn logger_methods_are_callable() {
    Logger::info("info");
    Logger::warn("warn");
    Logger::error("error");
    Logger::debug("debug");
    Logger::success("success");
}

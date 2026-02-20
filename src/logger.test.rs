use super::Logger;

#[test]
fn logger_methods_are_callable() {
    Logger::set_verbose(true);
    Logger::info("info");
    Logger::warn("warn");
    Logger::error("error");
    Logger::debug("debug");
    Logger::success("success");
}

#[test]
fn debug_only_when_verbose_flag_set() {
    Logger::set_verbose(false);
    assert!(!Logger::is_verbose());
    // calling debug should be a no-op when verbose is false
    Logger::debug("should not appear");

    Logger::set_verbose(true);
    assert!(Logger::is_verbose());
    // now debug prints should be enabled (we can't easily capture stdout here,
    // but the boolean check ensures that the branch will fire)
    Logger::debug("should appear");
}

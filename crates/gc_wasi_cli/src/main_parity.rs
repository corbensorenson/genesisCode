fn main() -> std::process::ExitCode {
    gc_cli_driver::run_with_profile(
        gc_cli_driver::Flavor::Wasi,
        gc_cli_driver::RuntimeProfile::ParityHarness,
    )
}

fn main() -> std::process::ExitCode {
    gc_cli_driver_parity::run_with_profile(
        gc_cli_driver_parity::Flavor::Native,
        gc_cli_driver_parity::RuntimeProfile::ParityHarness,
    )
}

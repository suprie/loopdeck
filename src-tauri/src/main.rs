fn main() {
    // `loopdeck state ...` → the narrow state CLI (Phase 3 `state-command`),
    // reused by bundled skills/hooks to transition loops through the validated
    // Rust domain. Any other (or no) invocation starts the desktop app as
    // before, so a normal GUI launch (which never passes `state` first) is
    // unaffected.
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("state") {
        let code = loopdeck_lib::run_state_cli(args.collect());
        std::process::exit(code);
    }
    loopdeck_lib::run()
}

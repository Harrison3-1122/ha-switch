fn main() {
    if let Err(err) = ha_switch::run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let task = xtask::parse_task(&args).unwrap_or_else(|err| {
        eprintln!("{err}");
        std::process::exit(2);
    });

    if let Err(err) = xtask::run_task(task) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

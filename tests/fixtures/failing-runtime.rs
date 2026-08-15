fn main() {
    if std::env::args_os().any(|argument| argument == "--version") {
        println!("memory-supervisor failing activation canary");
        return;
    }
    std::process::exit(41);
}

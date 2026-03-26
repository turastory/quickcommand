use std::process;

fn main() {
    match quickcommand::run() {
        Ok(code) => process::exit(code),
        Err(error) => {
            eprintln!("Error: {error}");
            process::exit(1);
        }
    }
}

//! `guac-extract`: the extractor from a terminal. A stub until the first
//! subcommand lands (AB.4 identity, AB.5 census).

use std::process::ExitCode;

const USAGE: &str = "\
usage: guac-extract [--help | --version]

Extracts what guacamole plays from your own Skylanders disc dump.
No subcommands yet.
";

fn main() -> ExitCode {
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        None | Some("-h" | "--help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("-V" | "--version") => {
            println!(
                "guac-extract {} (layout v{}, IR v{})",
                env!("CARGO_PKG_VERSION"),
                guac_extract::LAYOUT_VERSION,
                guac_extract::IR_VERSION
            );
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprint!("guac-extract: unknown argument `{other}`\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

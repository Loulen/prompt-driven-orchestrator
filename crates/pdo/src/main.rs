//! The `pdo` binary PDO ships: the whole CLI, with no extension (ADR-0081).

fn main() -> std::process::ExitCode {
    pdo::main_with(Vec::new)
}

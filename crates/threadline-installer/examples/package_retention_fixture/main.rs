#[cfg(windows)]
mod confirm;
#[cfg(windows)]
mod console;
#[cfg(windows)]
mod fixture;
#[cfg(windows)]
mod resume;

#[cfg(windows)]
type R<T> = Result<T, Box<dyn std::error::Error>>;

#[cfg(windows)]
fn run() -> R<serde_json::Value> {
    use serde_json::json;
    use std::{
        env,
        io::{self, IsTerminal, Write},
        path::Path,
    };
    let args: Vec<_> = env::args().skip(1).collect();
    match args.as_slice() {
        [command, nonce] => fixture::run(command, nonce),
        [command, archive, nonce, digest] if command == "resume" => {
            resume::run(Path::new(archive), nonce, digest)
        }
        [command, exe, exe_hash, archive, nonce, db_hash] if command == "confirm" => {
            confirm::run(Path::new(exe), exe_hash, Path::new(archive), nonce, db_hash)
        }
        [command] if command == "check-terminal" => {
            console::invoke(
                &env::current_exe()?,
                "scratch-prompt",
                &["Type FIXTURE READY:"],
                "FIXTURE READY",
                "Harmless scratch phrase accepted.",
                || Ok(()),
            )?;
            Ok(
                json!({"status":"Pass","child_stdin_terminal":true,"prompt_and_answer_verified":true}),
            )
        }
        [command] if command == "scratch-prompt" && io::stdin().is_terminal() => {
            print!("Type FIXTURE READY: ");
            io::stdout().flush()?;
            let mut answer = String::new();
            io::stdin().read_line(&mut answer)?;
            if answer.trim_end_matches(['\r', '\n']) != "FIXTURE READY" {
                return Err("wrong fixture phrase".into());
            }
            println!("Harmless scratch phrase accepted.");
            Ok(serde_json::Value::Null)
        }
        _ => Err("expected a package retention fixture command".into()),
    }
}

#[cfg(windows)]
fn main() {
    match run() {
        Ok(result) if !result.is_null() => println!("{result}"),
        Ok(_) => (),
        Err(error) => {
            eprintln!("Package retention fixture: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The installed package retention fixture requires native Windows.");
    std::process::exit(78);
}

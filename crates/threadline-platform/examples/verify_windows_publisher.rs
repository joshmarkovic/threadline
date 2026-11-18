#[cfg(windows)]
fn main() -> std::process::ExitCode {
    use std::{env, path::Path, process::ExitCode};

    let mut arguments = env::args_os().skip(1);
    let (Some(file), Some(pin), None) = (arguments.next(), arguments.next(), arguments.next())
    else {
        return ExitCode::from(3);
    };
    let Some(pin) = pin.into_string().ok().and_then(|pin| parse_pin(&pin)) else {
        return ExitCode::from(3);
    };
    match publisher::verify(Path::new(&file), pin) {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(3)
}

#[cfg(windows)]
fn parse_pin(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut pin = [0u8; 32];
    for (index, byte) in pin.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(pin)
}
#[cfg(windows)]
#[path = "../src/publisher.rs"]
mod publisher;

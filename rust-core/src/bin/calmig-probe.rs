//! Explicit, read-only diagnostic for a user-supplied legacy session.
use calmig_core::{Client, Session};
use std::{env, fs::File, io::Read, process::ExitCode};

fn run() -> Result<(), &'static str> {
    let mut args = env::args_os();
    let _ = args.next();
    let path = args
        .next()
        .ok_or("Usage: calmig-probe <legacy-session-file>")?;
    if args.next().is_some() {
        return Err("Usage: calmig-probe <legacy-session-file>");
    }
    let mut input = Vec::new();
    File::open(path)
        .map_err(|_| "Cannot open the session file")?
        .take(2_000_001)
        .read_to_end(&mut input)
        .map_err(|_| "Cannot read the session file")?;
    let session = Session::from_legacy_json(&input).map_err(|_| "Invalid saved session")?;
    let client = Client::new(session).map_err(|_| "Cannot initialize the client")?;
    let page = client
        .following_page(None)
        .map_err(|_| "Could not fetch the first page of following")?;
    println!("Following accounts in first page: {}", page.accounts.len());
    println!("More pages available: {}", page.next_cursor.is_some());
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

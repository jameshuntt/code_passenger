//! `code-passenger`: runs the lines `code_passenger::lines` declares, and nothing else.

fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let exe = std::env::current_exe().unwrap_or_else(|_| "code-passenger".into());
    let o = code_passenger::lines::run_argv(&args, &exe);
    print!("{}", o.stdout);
    eprint!("{}", o.stderr);
    match (&o.kept, &o.receipt) {
        (Some(file), _) => eprintln!("code-passenger: {} (exit {}), receipt {}", o.word, o.code, file.display()),
        (None, Some(r)) => eprintln!("code-passenger: {} (exit {}), receipt sha256:{}", o.word, o.code, stable_digest::StableDigest::sha256(r.stable_material()).as_str()),
        (None, None) => {}
    }
    std::process::exit(o.code);
}

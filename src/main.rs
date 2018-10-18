#[macro_use]
extern crate failure;
extern crate percent_encoding;

use failure::Error;
use std::process::exit;
use std::ops::Deref;

type Result<R> = std::result::Result<R, Error>;

// TODO: handle sigint.

fn encode(s: &str) -> String {
    return percent_encoding::utf8_percent_encode(s, percent_encoding::SIMPLE_ENCODE_SET)
        .to_string();
}

fn decode<'a>(s: &'a str) -> impl AsRef<str> + 'a {
    return percent_encoding::percent_decode(s.as_bytes())
        .decode_utf8_lossy()
        .deref();
}

fn send_cmd(cmd: &str, args: &[&str]) {
    let mut output = cmd.to_owned();
    for arg in args {
        output += " ";
        output += &encode(arg);
    }
    println!("{}", output);
}

fn ack() {
    send_cmd("OK", &[]);
}

fn error(msg: &str) {
    send_cmd("ERROR", &[msg]);
    exit(1);
}

fn recv_cmd<'a>() -> Vec<&'a str> {
    // TODO: Handle EOF
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
    let mut cmd = vec![];
    for word in input.split_whitespace() {
        cmd.push(decode(word).as_ref())
    }
    cmd
}

fn fsevent_handler() {}

fn main() -> Result<()> {
    send_cmd("VERSION", &["1"]);

    let input = recv_cmd();
    if input.get(0) != Some(&"VERSION".into()) {
        bail!("unexpected version cmd: {:?}", input.get(0));
    }
    if input.get(1) != Some(&"1".into()) {
        bail!("unexpected version: {:?}", input.get(1));
    }

    let empty_string = "";

    loop {
        let input = recv_cmd();
        let cmd = input.get(0).map(String::as_str).unwrap_or(&empty_string);

        if cmd == "DEBUG" {
        } else if cmd == "START" {
        } else if cmd == "WAIT" {
        } else if cmd == "CHANGES" {
        } else if cmd == "RESET" {
        } else {
            error(&format!("unexpected root cmd: {}", cmd));
        }
    }
}

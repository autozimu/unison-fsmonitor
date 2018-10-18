#[macro_use]
extern crate failure;
extern crate percent_encoding;

use failure::Error;
use std::process::exit;

type Result<R> = std::result::Result<R, Error>;

// TODO: handle sigint.

fn encode(s: &str) -> impl AsRef<str> {
    return percent_encoding::utf8_percent_encode(s, percent_encoding::SIMPLE_ENCODE_SET)
        .to_string();
}

fn decode<'a>(s: &'a str) -> impl AsRef<str> + 'a {
    return percent_encoding::percent_decode(s.as_bytes()).decode_utf8_lossy();
}

fn send_cmd(cmd: &str, args: &[&str]) {
    let mut output = cmd.to_owned();
    for arg in args {
        output += " ";
        output += &encode(arg).as_ref();
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

fn recv_cmd() -> Vec<String> {
    // TODO: Handle EOF
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
    let mut cmd = vec![];
    for word in input.split_whitespace() {
        cmd.push(decode(word).as_ref().to_owned())
    }
    cmd
}

fn fsevent_handler() {}

fn main() -> Result<()> {
    send_cmd("VERSION", &["1"]);

    let input = recv_cmd();
    match input
        .iter()
        .map(String::as_str)
        .collect::<Vec<&str>>()
        .as_slice()
    {
        ["VERSION", "1"] => (),
        ["VERSION", _] => bail!("unexpected version: {:?}", input.get(1)),
        _ => bail!("unexpected version cmd: {:?}", input.get(0)),
    };

    loop {
        let input = recv_cmd();
        let cmd = input.get(0).cloned().unwrap_or_default();

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

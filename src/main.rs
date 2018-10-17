extern crate percent_encoding;

// TODO: handle sigint.

fn encode(s: &str) -> String {
    return percent_encoding::utf8_percent_encode(s, percent_encoding::SIMPLE_ENCODE_SET)
        .to_string();
}

fn sendCmd(cmd: &str, args: &[&str]) {
    let mut output = cmd.to_owned();
    for arg in args {
        output += " ";
        output += &encode(arg);
    }
    println!("{}", output);
}

fn ack() {
    sendCmd("OK", &[]);
}

fn error(msg: &str) {
    sendCmd("ERROR", &[msg]);
    // TODO: exit(1)
}

fn recvCmd() {
    // TODO: Handle EOF
}

fn main() {
    println!("Hello, world!");
}

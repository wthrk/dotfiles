//! GitHub SSH host key pin の外部技術検証。

use git2::cert::Cert;

const GITHUB_HOST: &str = "github.com";
const GITHUB_SSH_HOST_KEYS: [&str; 3] = [
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl",
    "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBEmKSENjQEezOmxkZMy7opKgwFB9nkt5YRrYMjNuG5N87uRgg6CLrbo5wAdT/y6v0mKV0U2w0WZ2YB/++Tpockg=",
    "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQCj7ndNxQowgcQnjshcLrqPEiiphnt+VTTvDP6mHBL9j1aNUkY4Ue1gvwnGLVlOhGeYrnZaMgRK6+PKCUXaDbC7qtbW8gIkhL7aGCsOr/C56SJMy/BCZfxd1nWzAOxSDPgVsmerOBYfNqltV9/hWCqBywINIR+5dIg6JTJ72pcEpEjcYgXkE2YEFXV1JHnsKgbLWNlhScqb2UmyRkQyytRLtL+38TGxkxCflmO+5Z8CSSNY7GidjMIZ7Q4zMjA2n1nGrlTDkzwDCsw+wqFPGQA179cnfGWOWRVruj16z6XyvxvjJwbz0wQZ75XK5tKSb7FNyeIEs4TT4jk+S4dhPeAUC5y+bDYirYgM4GC7uEnztnZyaVWQ7B381AK4Qdrwt51ZqExKbQpTUNn+EjqoTwvqNj4kqx5QUCI0ThS/YkOxJCXmPUWZbhjpCg56i+2aB6CmK2JGhn57K5mj0MNdBXA4/WnwH6XoPWJzK5Nyu2zB3nAZp+S5hpQs+p1vN1/wsjk=",
];

pub(crate) fn verify(cert: &Cert<'_>, hostname: &str) -> std::result::Result<(), String> {
    if hostname != GITHUB_HOST {
        return Err(format!(
            "refusing to clone: unexpected SSH host '{hostname}', only {GITHUB_HOST} is allowed"
        ));
    }
    let hostkey = cert
        .as_hostkey()
        .ok_or_else(|| "refusing to clone: server did not present an SSH host key".to_owned())?;
    let raw = hostkey
        .hostkey()
        .ok_or_else(|| "refusing to clone: SSH host key is unavailable for pinning".to_owned())?;
    let type_name = hostkey
        .hostkey_type()
        .ok_or_else(|| "refusing to clone: SSH host key type is unknown".to_owned())?
        .name();
    if GITHUB_SSH_HOST_KEYS.iter().any(|pinned| {
        let mut fields = pinned.split_whitespace();
        matches!((fields.next(), fields.next()), (Some(kind), Some(body)) if kind == type_name && decode(body).is_some_and(|value| value == raw))
    }) { Ok(()) } else { Err(format!("refusing to clone: {GITHUB_HOST} SSH host key did not match GitHub's published host keys (possible MITM)")) }
}

fn decode(value: &str) -> Option<Vec<u8>> {
    if value.is_empty() || !value.len().is_multiple_of(4) {
        return None;
    }
    let mut output = Vec::with_capacity(value.len() / 4 * 3);
    for (chunk_index, chunk) in value.as_bytes().chunks(4).enumerate() {
        let final_chunk = chunk_index + 1 == value.len() / 4;
        let mut accumulator = 0u32;
        let mut padding = 0usize;
        for (index, &byte) in chunk.iter().enumerate() {
            if byte == b'=' {
                if !final_chunk || index < 2 {
                    return None;
                };
                padding += 1;
                accumulator <<= 6;
            } else {
                if padding != 0 {
                    return None;
                };
                accumulator = (accumulator << 6) | u32::from(base64(byte)?);
            }
        }
        output.extend_from_slice(&accumulator.to_be_bytes()[1..=3 - padding]);
    }
    Some(output)
}

fn base64(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

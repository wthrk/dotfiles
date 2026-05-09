pub fn short(host: &str) -> &str {
    host.split('.').next().unwrap_or(host)
}

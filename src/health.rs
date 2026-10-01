use crate::error::Error;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::process::Command;
use std::thread;
use std::time::Duration;

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct HealthCheckConfig {
    pub http: Option<String>,
    pub tcp: Option<String>,
    #[serde(alias = "cmd", alias = "exec")]
    pub exec_cmd: Option<String>,
    #[serde(default = "default_interval_ms")]
    pub interval_ms: u64,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_retries")]
    pub retries: u32,
}

fn default_interval_ms() -> u64 {
    500
}

fn default_timeout_ms() -> u64 {
    2000
}

fn default_retries() -> u32 {
    10
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct RestartPolicyConfig {
    #[serde(default = "default_restart_condition")]
    pub condition: String, // "always", "on_failure", "never"
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_backoff_ms")]
    pub backoff_ms: u64,
}

fn default_restart_condition() -> String {
    "on_failure".to_string()
}

fn default_max_retries() -> u32 {
    5
}

fn default_backoff_ms() -> u64 {
    1000
}

pub fn check_tcp(addr_str: &str, timeout: Duration) -> bool {
    let addrs: Vec<_> = match addr_str.to_socket_addrs() {
        Ok(iter) => iter.collect(),
        Err(_) => return false,
    };
    for addr in addrs {
        if TcpStream::connect_timeout(&addr, timeout).is_ok() {
            return true;
        }
    }
    false
}

pub fn check_http(url: &str, timeout: Duration) -> bool {
    // Basic parser for http://host:port/path
    let stripped = url.strip_prefix("http://").unwrap_or(url);
    let (host_port, path) = match stripped.split_once('/') {
        Some((hp, p)) => (hp, format!("/{}", p)),
        None => (stripped, "/".to_string()),
    };
    let host_port_with_default = if host_port.contains(':') {
        host_port.to_string()
    } else {
        format!("{}:80", host_port)
    };

    let addrs: Vec<_> = match host_port_with_default.to_socket_addrs() {
        Ok(iter) => iter.collect(),
        Err(_) => return false,
    };

    for addr in addrs {
        if let Ok(mut stream) = TcpStream::connect_timeout(&addr, timeout) {
            stream.set_read_timeout(Some(timeout)).ok();
            stream.set_write_timeout(Some(timeout)).ok();
            let req = format!(
                "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: fates/0.2.0\r\nConnection: close\r\n\r\n",
                path, host_port
            );
            if stream.write_all(req.as_bytes()).is_ok() {
                let mut resp = [0u8; 512];
                if let Ok(n) = stream.read(&mut resp) {
                    let resp_str = String::from_utf8_lossy(&resp[..n]);
                    // e.g. HTTP/1.1 200 OK
                    if let Some(first_line) = resp_str.lines().next() {
                        let parts: Vec<&str> = first_line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            if let Ok(status) = parts[1].parse::<u16>() {
                                if (200..400).contains(&status) {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

pub fn check_exec(cmd_str: &str) -> bool {
    match Command::new("sh").arg("-c").arg(cmd_str).status() {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

pub fn probe(hc: &HealthCheckConfig) -> bool {
    let timeout = Duration::from_millis(hc.timeout_ms);
    if let Some(ref tcp) = hc.tcp {
        return check_tcp(tcp, timeout);
    }
    if let Some(ref http) = hc.http {
        return check_http(http, timeout);
    }
    if let Some(ref exec_cmd) = hc.exec_cmd {
        return check_exec(exec_cmd);
    }
    true
}

pub fn wait_for_healthy(name: &str, hc: &HealthCheckConfig) -> Result<(), Error> {
    let retries = hc.retries.max(1);
    let interval = Duration::from_millis(hc.interval_ms);

    for attempt in 1..=retries {
        if probe(hc) {
            println!("Thread '{}' is healthy (attempt {}/{})", name, attempt, retries);
            return Ok(());
        }
        thread::sleep(interval);
    }

    Err(Error::command(format!(
        "Thread '{}' failed health check after {} retries",
        name, retries
    )))
}

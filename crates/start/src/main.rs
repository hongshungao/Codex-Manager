use std::fs;
use std::io::Write;
use std::net::TcpStream;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const ENV_CANDIDATES: [&str; 3] = ["codexmanager.env", "CodexManager.env", ".env"];
const DEFAULT_SERVICE_ADDR: &str = "localhost:48760";
const DEFAULT_WEB_ADDR: &str = "localhost:48761";

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn strip_inline_comment(value: &str) -> &str {
    // 仅把 ` #` 视为注释起点，保持与常见 dotenv 行为一致
    let Some(pos) = value.find(" #") else {
        return value;
    };
    value[..pos].trim_end()
}

fn parse_dotenv_kv(line: &str) -> Option<(String, String)> {
    let mut line = line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
        return None;
    }
    if let Some(rest) = line.strip_prefix("export ") {
        line = rest.trim();
    }
    let (key, raw_value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    let mut value = raw_value.trim();
    if (value.starts_with('"') && value.ends_with('"') && value.len() >= 2)
        || (value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2)
    {
        value = &value[1..value.len() - 1];
    } else {
        value = strip_inline_comment(value);
    }
    Some((key.to_string(), value.to_string()))
}

fn find_env_file_in_dir(dir: &Path) -> Option<PathBuf> {
    for name in ENV_CANDIDATES {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn load_env_from_exe_dir_best_effort() {
    let dir = exe_dir();
    let Some(path) = find_env_file_in_dir(&dir) else {
        return;
    };

    let Ok(text) = fs::read_to_string(&path) else {
        return;
    };

    for line in text.lines() {
        let Some((key, value)) = parse_dotenv_kv(line) else {
            continue;
        };
        if std::env::var_os(&key).is_some() {
            continue;
        }
        std::env::set_var(key, value);
    }
}

fn normalize_addr(raw: &str) -> Option<String> {
    let mut value = raw.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(rest) = value.strip_prefix("http://") {
        value = rest;
    }
    if let Some(rest) = value.strip_prefix("https://") {
        value = rest;
    }
    value = value.split('/').next().unwrap_or(value);
    if value.is_empty() {
        return None;
    }
    if value.contains(':') {
        return Some(value.to_string());
    }
    Some(format!("localhost:{value}"))
}

fn resolve_addr(var: &str, default: &str) -> String {
    std::env::var(var)
        .ok()
        .and_then(|v| normalize_addr(&v))
        .unwrap_or_else(|| default.to_string())
}

fn resolve_socket_addrs_best_effort(host_port: &str) -> Vec<SocketAddr> {
    // 优先处理 localhost（避免 DNS 差异/大小写问题）
    let trimmed = host_port.trim();
    if trimmed.len() > "localhost:".len()
        && trimmed[..("localhost:".len())].eq_ignore_ascii_case("localhost:")
    {
        let port = &trimmed["localhost:".len()..];
        if let Ok(port) = port.parse::<u16>() {
            return vec![
                SocketAddr::from(([127, 0, 0, 1], port)),
                SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port)),
            ];
        }
    }

    host_port
        .to_socket_addrs()
        .ok()
        .into_iter()
        .flatten()
        .collect()
}

fn tcp_probe(addr: &str) -> bool {
    let addr = addr.trim();
    if addr.is_empty() {
        return false;
    }
    let addr = addr.strip_prefix("http://").unwrap_or(addr);
    let addr = addr.strip_prefix("https://").unwrap_or(addr);
    let addr = addr.split('/').next().unwrap_or(addr);

    for sock in resolve_socket_addrs_best_effort(addr) {
        if TcpStream::connect_timeout(&sock, Duration::from_millis(250)).is_ok() {
            return true;
        }
    }
    false
}

fn simple_get_best_effort(addr: &str, path: &str) {
    let addr_trimmed = addr.trim();
    if addr_trimmed.is_empty() {
        return;
    }
    let addr_trimmed = addr_trimmed.strip_prefix("http://").unwrap_or(addr_trimmed);
    let addr_trimmed = addr_trimmed.strip_prefix("https://").unwrap_or(addr_trimmed);
    let addr_trimmed = addr_trimmed.split('/').next().unwrap_or(addr_trimmed);
    let Some(sock) = resolve_socket_addrs_best_effort(addr_trimmed).into_iter().next() else {
        return;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&sock, Duration::from_millis(300)) else {
        return;
    };
    let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {addr_trimmed}\r\nConnection: close\r\n\r\n"
    );
    let _ = stream.write_all(req.as_bytes());
}

fn bin_path(dir: &Path, name: &str) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        return dir.join(format!("{name}.exe"));
    }
    #[cfg(not(target_os = "windows"))]
    {
        return dir.join(name);
    }
}

fn spawn_child(bin: &Path) -> std::io::Result<Child> {
    Command::new(bin).spawn()
}

fn main() {
    // 让 start.exe 也支持同目录 env 文件，保持与 service/web 一致。
    load_env_from_exe_dir_best_effort();

    let dir = exe_dir();
    let service_addr = resolve_addr("CODEXMANAGER_SERVICE_ADDR", DEFAULT_SERVICE_ADDR);
    let web_addr = resolve_addr("CODEXMANAGER_WEB_ADDR", DEFAULT_WEB_ADDR);

    let service_bin = bin_path(&dir, "codexmanager-service");
    let web_bin = bin_path(&dir, "codexmanager-web");

    println!("CodexManager 启动器");
    println!("- service: {service_addr}");
    println!("- web:     http://{web_addr}/");
    println!("按 Ctrl+C 退出");

    if !web_bin.is_file() {
        eprintln!("缺少文件：{}", web_bin.display());
        std::process::exit(1);
    }

    let mut spawned_service = false;
    let mut service_child: Option<Child> = None;
    if tcp_probe(&service_addr) {
        println!("service 已在运行，跳过拉起。");
    } else if !service_bin.is_file() {
        eprintln!("service 不可达且缺少文件：{}", service_bin.display());
        std::process::exit(1);
    } else {
        println!("正在启动 service...");
        match spawn_child(&service_bin) {
            Ok(child) => {
                service_child = Some(child);
                spawned_service = true;
            }
            Err(err) => {
                eprintln!("启动 service 失败：{err}");
                std::process::exit(1);
            }
        }
    }

    // web 若已运行：直接打开浏览器，然后退出（避免占用端口再次启动失败）。
    if tcp_probe(&web_addr) {
        println!("web 已在运行，直接打开浏览器。");
        let _ = webbrowser::open(&format!("http://{web_addr}/"));
        return;
    }

    println!("正在启动 web...");
    let mut web_cmd = Command::new(&web_bin);
    // 由 start.exe 统一管理 service，避免 web 进程重复拉起/竞态。
    web_cmd.env("CODEXMANAGER_WEB_NO_SPAWN_SERVICE", "1");
    // 让 web 使用与本进程解析到的一致地址，避免 env 文件/系统变量差异导致难以定位。
    web_cmd.env("CODEXMANAGER_SERVICE_ADDR", &service_addr);
    web_cmd.env("CODEXMANAGER_WEB_ADDR", &web_addr);

    let mut web_child = match web_cmd.spawn() {
        Ok(v) => v,
        Err(err) => {
            eprintln!("启动 web 失败：{err}");
            std::process::exit(1);
        }
    };

    let should_exit = Arc::new(AtomicBool::new(false));
    {
        let flag = Arc::clone(&should_exit);
        let _ = ctrlc::set_handler(move || {
            flag.store(true, Ordering::SeqCst);
        });
    }

    // 监督进程：Ctrl+C 或任一子进程退出则进入关闭流程。
    loop {
        if should_exit.load(Ordering::SeqCst) {
            break;
        }
        if let Ok(Some(status)) = web_child.try_wait() {
            println!("web 已退出：{status}");
            break;
        }
        if let Some(child) = service_child.as_mut() {
            if let Ok(Some(status)) = child.try_wait() {
                println!("service 已退出：{status}");
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }

    println!("正在关闭...");

    // 先关 web，再关 service；仅当本进程拉起过 service 才尝试关闭它。
    simple_get_best_effort(&web_addr, "/__quit");
    if spawned_service {
        simple_get_best_effort(&service_addr, "/__shutdown");
    }

    // 最后兜底：短等后强杀
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let web_done = web_child.try_wait().ok().flatten().is_some();
        let service_done = match service_child.as_mut() {
            Some(child) => child.try_wait().ok().flatten().is_some(),
            None => true,
        };
        if web_done && service_done {
            break;
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let _ = web_child.kill();
    if let Some(mut child) = service_child {
        let _ = child.kill();
    }
}

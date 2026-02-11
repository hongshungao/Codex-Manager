use std::io;
use tiny_http::Request;
use tiny_http::Response;
use tiny_http::Server;
use url::Url;

use crate::auth_tokens::complete_login;

pub(crate) fn resolve_redirect_uri() -> Option<String> {
    // 优先使用显式配置的回调地址
    if let Ok(uri) = std::env::var("GPTTOOLS_REDIRECT_URI") {
        if let Ok(url) = Url::parse(&uri) {
            let host = url.host_str().unwrap_or("localhost");
            let port = url.port_or_known_default().unwrap_or(1455);
            let _ = ensure_login_server_with_addr(&format!("{host}:{port}"));
        }
        return Some(uri);
    }
    let info = ensure_login_server().ok()?;
    Some(format!("http://localhost:{}/auth/callback", info.port))
}

pub(crate) fn handle_login_request(request: Request) -> Result<(), String> {
    // 解析回调地址与参数
    let url = Url::parse(&format!("http://localhost{}", request.url()))
        .map_err(|e| format!("invalid url: {e}"))?;
    if url.path() != "/auth/callback" {
        let _ = request.respond(Response::from_string("Not Found").with_status_code(404));
        return Ok(());
    }

    let code = url
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.into_owned());
    let state = url
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.into_owned());

    let (Some(code), Some(state)) = (code, state) else {
        let _ = request.respond(Response::from_string("Missing code/state").with_status_code(400));
        return Ok(());
    };

    // 完成登录流程并响应浏览器
    let result = handle_login_callback_params(&code, &state);
    match result {
        Ok(_) => {
            let _ = request.respond(Response::from_string(
                "Login success. You can close this window.",
            ));
        }
        Err(err) => {
            let _ = request.respond(
                Response::from_string(format!("Login failed: {err}")).with_status_code(500),
            );
        }
    }
    Ok(())
}

pub(crate) fn handle_login_callback_params(code: &str, state: &str) -> Result<(), String> {
    complete_login(state, code)
}

#[derive(Clone, Debug)]
pub(crate) struct LoginServerInfo {
    port: u16,
}

static LOGIN_SERVER_STATE: std::sync::OnceLock<std::sync::Mutex<Option<LoginServerInfo>>> =
    std::sync::OnceLock::new();

pub(crate) fn ensure_login_server() -> Result<LoginServerInfo, String> {
    let addr =
        std::env::var("GPTTOOLS_LOGIN_ADDR").unwrap_or_else(|_| "localhost:1455".to_string());
    ensure_login_server_with_addr(&addr)
}

fn ensure_login_server_with_addr(addr: &str) -> Result<LoginServerInfo, String> {
    let cell = LOGIN_SERVER_STATE.get_or_init(|| std::sync::Mutex::new(None));
    let mut guard = cell
        .lock()
        .map_err(|_| "login server lock poisoned".to_string())?;
    if let Some(info) = guard.as_ref() {
        return Ok(info.clone());
    }
    let (servers, info) = bind_login_server(addr)?;
    for server in servers {
        let _ = std::thread::spawn(move || run_login_server(server));
    }
    *guard = Some(info.clone());
    Ok(info)
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

fn allow_non_loopback_login_addr() -> bool {
    matches!(
        std::env::var("GPTTOOLS_ALLOW_NON_LOOPBACK_LOGIN_ADDR")
            .ok()
            .as_deref()
            .map(str::trim),
        Some("1" | "true" | "TRUE" | "yes" | "YES")
    )
}

fn server_port(server: &Server) -> Result<u16, String> {
    server
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .ok_or_else(|| "login server missing port".to_string())
}

fn try_bind_login_server(
    addr: &str,
    servers: &mut Vec<Server>,
    addr_in_use: &mut bool,
    last_err: &mut Option<String>,
) -> Result<Option<u16>, String> {
    match Server::http(addr) {
        Ok(server) => {
            let port = server_port(&server)?;
            servers.push(server);
            Ok(Some(port))
        }
        Err(err) => {
            *addr_in_use |= is_addr_in_use(err.as_ref());
            if last_err.is_none() {
                *last_err = Some(err.to_string());
            }
            Ok(None)
        }
    }
}

fn bind_localhost_login_servers(port: u16) -> Result<(Vec<Server>, LoginServerInfo), String> {
    let mut addr_in_use = false;
    let mut last_err: Option<String> = None;
    let mut servers: Vec<Server> = Vec::new();
    let mut selected_port = port;

    if port == 0 {
        if let Some(v4_port) = try_bind_login_server(
            "127.0.0.1:0",
            &mut servers,
            &mut addr_in_use,
            &mut last_err,
        )? {
            selected_port = v4_port;
            let _ = try_bind_login_server(
                &format!("[::1]:{selected_port}"),
                &mut servers,
                &mut addr_in_use,
                &mut last_err,
            )?;
        } else if let Some(v6_port) =
            try_bind_login_server("[::1]:0", &mut servers, &mut addr_in_use, &mut last_err)?
        {
            selected_port = v6_port;
            let _ = try_bind_login_server(
                &format!("127.0.0.1:{selected_port}"),
                &mut servers,
                &mut addr_in_use,
                &mut last_err,
            )?;
        }
    } else {
        let _ = try_bind_login_server(
            &format!("127.0.0.1:{port}"),
            &mut servers,
            &mut addr_in_use,
            &mut last_err,
        )?;
        let _ = try_bind_login_server(
            &format!("[::1]:{port}"),
            &mut servers,
            &mut addr_in_use,
            &mut last_err,
        )?;
    }

    if !servers.is_empty() {
        if selected_port == 0 {
            selected_port = server_port(&servers[0])?;
        }
        return Ok((servers, LoginServerInfo { port: selected_port }));
    }
    if addr_in_use {
        return Err(format!(
            "登录回调端口 {port} 已被占用，请关闭占用程序或修改 GPTTOOLS_LOGIN_ADDR"
        ));
    }
    if let Some(err) = last_err {
        return Err(err);
    }
    Err("failed to bind login server".to_string())
}

fn bind_login_server(addr: &str) -> Result<(Vec<Server>, LoginServerInfo), String> {
    if let Ok(url) = Url::parse(&format!("http://{addr}")) {
        let host = url.host_str().unwrap_or("localhost");
        let port = url.port_or_known_default().unwrap_or(1455);
        if host == "localhost" {
            // 中文注释：localhost 绑定双栈，避免浏览器在 IPv4/IPv6 间切换时回调命中失败。
            return bind_localhost_login_servers(port);
        } else if !is_loopback_host(host) && !allow_non_loopback_login_addr() {
            return Err(format!(
                "登录回调地址仅允许 loopback（localhost/127.0.0.1/::1），当前为 {host}"
            ));
        }
    }

    let server = Server::http(addr).map_err(|e| e.to_string())?;
    let port = server_port(&server)?;
    Ok((vec![server], LoginServerInfo { port }))
}

fn is_addr_in_use(err: &(dyn std::error::Error + 'static)) -> bool {
    err.downcast_ref::<io::Error>()
        .map(|io_err| io_err.kind() == io::ErrorKind::AddrInUse)
        .unwrap_or(false)
}

fn run_login_server(server: Server) {
    for request in server.incoming_requests() {
        if let Err(err) = handle_login_request(request) {
            log::warn!("login request error: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ensure_login_server_with_addr, resolve_redirect_uri, LOGIN_SERVER_STATE};
    use std::net::TcpListener;
    use std::sync::Mutex;
    use url::Url;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn reset_login_server_state() {
        if let Some(cell) = LOGIN_SERVER_STATE.get() {
            if let Ok(mut guard) = cell.lock() {
                *guard = None;
            }
        }
    }

    #[test]
    fn resolve_redirect_uri_prefers_login_server() {
        let _guard = ENV_LOCK.lock().expect("lock");
        reset_login_server_state();
        let prev_redirect = std::env::var("GPTTOOLS_REDIRECT_URI").ok();
        let prev_login = std::env::var("GPTTOOLS_LOGIN_ADDR").ok();
        let prev_service = std::env::var("GPTTOOLS_SERVICE_ADDR").ok();

        std::env::remove_var("GPTTOOLS_REDIRECT_URI");
        std::env::set_var("GPTTOOLS_LOGIN_ADDR", "localhost:0");
        std::env::set_var("GPTTOOLS_SERVICE_ADDR", "localhost:48760");

        let uri = resolve_redirect_uri().expect("redirect uri");
        let url = Url::parse(&uri).expect("parse redirect uri");
        assert_eq!(url.host_str(), Some("localhost"));
        let port = url.port_or_known_default().expect("port");
        assert_ne!(port, 48760);
        assert_eq!(url.path(), "/auth/callback");

        match prev_redirect {
            Some(value) => std::env::set_var("GPTTOOLS_REDIRECT_URI", value),
            None => std::env::remove_var("GPTTOOLS_REDIRECT_URI"),
        }
        match prev_login {
            Some(value) => std::env::set_var("GPTTOOLS_LOGIN_ADDR", value),
            None => std::env::remove_var("GPTTOOLS_LOGIN_ADDR"),
        }
        match prev_service {
            Some(value) => std::env::set_var("GPTTOOLS_SERVICE_ADDR", value),
            None => std::env::remove_var("GPTTOOLS_SERVICE_ADDR"),
        }
        reset_login_server_state();
    }

    #[test]
    fn login_server_reports_port_in_use() {
        let _guard = ENV_LOCK.lock().expect("lock");
        reset_login_server_state();
        let prev_login = std::env::var("GPTTOOLS_LOGIN_ADDR").ok();

        let listener_v6 = TcpListener::bind("[::1]:0").expect("bind v6 port");
        let port = listener_v6.local_addr().expect("addr").port();
        let listener_v4 = TcpListener::bind(format!("127.0.0.1:{port}")).ok();
        let err = match ensure_login_server_with_addr(&format!("localhost:{port}")) {
            Ok(_) => panic!("expected port in use error"),
            Err(err) => err,
        };
        assert!(err.contains("占用") || err.contains("in use"));

        drop(listener_v4);
        drop(listener_v6);
        match prev_login {
            Some(value) => std::env::set_var("GPTTOOLS_LOGIN_ADDR", value),
            None => std::env::remove_var("GPTTOOLS_LOGIN_ADDR"),
        }
        reset_login_server_state();
    }

    #[test]
    fn login_server_rejects_non_loopback_by_default() {
        let _guard = ENV_LOCK.lock().expect("lock");
        reset_login_server_state();
        let prev_allow = std::env::var("GPTTOOLS_ALLOW_NON_LOOPBACK_LOGIN_ADDR").ok();

        std::env::remove_var("GPTTOOLS_ALLOW_NON_LOOPBACK_LOGIN_ADDR");
        let err = ensure_login_server_with_addr("0.0.0.0:1455").expect_err("should reject");
        assert!(err.contains("loopback"));

        match prev_allow {
            Some(value) => std::env::set_var("GPTTOOLS_ALLOW_NON_LOOPBACK_LOGIN_ADDR", value),
            None => std::env::remove_var("GPTTOOLS_ALLOW_NON_LOOPBACK_LOGIN_ADDR"),
        }
        reset_login_server_state();
    }
}

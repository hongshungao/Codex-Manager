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
    let result = complete_login(&state, &code);
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
    let (server, info) = bind_login_server(addr)?;
    let _ = std::thread::spawn(move || run_login_server(server));
    *guard = Some(info.clone());
    Ok(info)
}

fn bind_login_server(addr: &str) -> Result<(Server, LoginServerInfo), String> {
    if let Ok(url) = Url::parse(&format!("http://{addr}")) {
        let host = url.host_str().unwrap_or("localhost");
        let port = url.port_or_known_default().unwrap_or(1455);
        if host == "localhost" {
            let mut addr_in_use = false;
            let mut last_err: Option<Box<dyn std::error::Error + Send + Sync>> = None;
            match Server::http(format!("[::]:{port}")) {
                Ok(server) => {
                    let port = server
                        .server_addr()
                        .to_ip()
                        .map(|a| a.port())
                        .ok_or_else(|| "login server missing port".to_string())?;
                    return Ok((server, LoginServerInfo { port }));
                }
                Err(err) => {
                    addr_in_use |= is_addr_in_use(err.as_ref());
                    if last_err.is_none() {
                        last_err = Some(err);
                    }
                }
            }
            match Server::http(format!("127.0.0.1:{port}")) {
                Ok(server) => {
                    let port = server
                        .server_addr()
                        .to_ip()
                        .map(|a| a.port())
                        .ok_or_else(|| "login server missing port".to_string())?;
                    return Ok((server, LoginServerInfo { port }));
                }
                Err(err) => {
                    addr_in_use |= is_addr_in_use(err.as_ref());
                    if last_err.is_none() {
                        last_err = Some(err);
                    }
                }
            }
            if addr_in_use {
                return Err(format!(
                    "登录回调端口 {port} 已被占用，请关闭占用程序或修改 GPTTOOLS_LOGIN_ADDR"
                ));
            }
            if let Some(err) = last_err {
                return Err(err.to_string());
            }
        }
    }

    let server = Server::http(addr).map_err(|e| e.to_string())?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .ok_or_else(|| "login server missing port".to_string())?;
    Ok((server, LoginServerInfo { port }))
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
    use super::{resolve_redirect_uri, ensure_login_server_with_addr, LOGIN_SERVER_STATE};
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

        let listener_v6 = TcpListener::bind("[::]:0").expect("bind v6 port");
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
}

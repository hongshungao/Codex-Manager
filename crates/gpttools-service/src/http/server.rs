use std::io;
use std::thread;
use tiny_http::Request;
use tiny_http::Server;

fn run_server(server: Server) {
    for request in server.incoming_requests() {
        if crate::shutdown_requested() || request.url() == "/__shutdown" {
            let _ = request.respond(tiny_http::Response::from_string("shutdown"));
            break;
        }
        thread::spawn(move || {
            route_request(request);
        });
    }
}

pub fn start_http(addr: &str) -> std::io::Result<()> {
    // On Windows, "localhost" may resolve to IPv6 loopback only ([::1]), while some clients
    // prefer IPv4 (127.0.0.1). To keep using "localhost" in URLs and still support both
    // families, bind BOTH loopback listeners when the caller requests localhost.
    if let Some(port) = addr.strip_prefix("localhost:") {
        let v4 = Server::http(format!("127.0.0.1:{port}"))
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err));
        let v6 = Server::http(format!("[::1]:{port}"))
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err));

        match (v4, v6) {
            (Ok(v4_server), Ok(v6_server)) => {
                let join = thread::spawn(move || run_server(v6_server));
                run_server(v4_server);
                let _ = join.join();
                return Ok(());
            }
            (Ok(server), Err(_)) | (Err(_), Ok(server)) => {
                run_server(server);
                return Ok(());
            }
            (Err(err), Err(_)) => return Err(err),
        }
    }

    let server = Server::http(addr).map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
    run_server(server);
    Ok(())
}

pub fn route_request(request: Request) {
    let path = request.url().to_string();
    if request.method().as_str() == "POST" && path == "/rpc" {
        crate::http::rpc_endpoint::handle_rpc(request);
        return;
    }
    if request.method().as_str() == "GET" && path.starts_with("/auth/callback") {
        crate::http::callback_endpoint::handle_callback(request);
        return;
    }
    crate::http::gateway_endpoint::handle_gateway(request);
}

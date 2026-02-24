use tiny_http::Request;

pub fn handle_callback(request: Request) {
    if let Err(err) = crate::auth_callback::handle_login_request(request) {
        log::warn!("callback request error: {err}");
    }
}

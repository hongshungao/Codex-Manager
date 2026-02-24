use codexmanager_core::storage::{Account, Storage, Token};
use tiny_http::{Request, Response};

pub(super) enum CandidatePrecheckResult {
    Ready {
        request: Request,
        candidates: Vec<(Account, Token)>,
    },
    Responded,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_candidates_for_proxy(
    request: Request,
    storage: &Storage,
    trace_id: &str,
    key_id: &str,
    path: &str,
    request_method: &str,
    model_for_log: Option<&str>,
    reasoning_for_log: Option<&str>,
) -> CandidatePrecheckResult {
    let candidates = match super::super::prepare_gateway_candidates(storage) {
        Ok(v) => v,
        Err(err) => {
            let err_text = format!("candidate resolve failed: {err}");
            super::super::write_request_log(
                storage,
                Some(key_id),
                path,
                request_method,
                model_for_log,
                reasoning_for_log,
                None,
                Some(500),
                Some(err_text.as_str()),
            );
            let response = Response::from_string(err_text.clone()).with_status_code(500);
            let _ = request.respond(response);
            super::super::trace_log::log_request_final(
                trace_id,
                500,
                None,
                None,
                Some(err_text.as_str()),
                0,
            );
            return CandidatePrecheckResult::Responded;
        }
    };

    if candidates.is_empty() {
        super::super::write_request_log(
            storage,
            Some(key_id),
            path,
            request_method,
            model_for_log,
            reasoning_for_log,
            None,
            Some(503),
            Some("no available account"),
        );
        let response = Response::from_string("no available account").with_status_code(503);
        let _ = request.respond(response);
        super::super::trace_log::log_request_final(
            trace_id,
            503,
            None,
            None,
            Some("no available account"),
            0,
        );
        return CandidatePrecheckResult::Responded;
    }

    CandidatePrecheckResult::Ready { request, candidates }
}



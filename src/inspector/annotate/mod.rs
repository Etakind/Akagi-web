//! 识别浏览器 HTTP 元数据的类别。注释在记录和广播前脱敏，不保存请求正文。

pub mod sls;

use crate::schema::{HttpAnnotation, HttpHeader};

/// What a recognizer gets to look at.
///
/// A struct rather than a parameter list so recognizers that need more
/// context later (a body, a header) do not force every call site to
/// change.
pub struct RequestView<'a> {
    pub method: &'a str,
    /// Absolute URL where the backend could reconstruct one.
    pub url: &'a str,
    pub headers: &'a [HttpHeader],
    /// Request body as text, when it was captured.
    pub body: Option<&'a str>,
}

impl<'a> RequestView<'a> {
    pub fn new(method: &'a str, url: &'a str, headers: &'a [HttpHeader]) -> Self {
        Self {
            method,
            url,
            headers,
            body: None,
        }
    }
}

/// Run every recognizer against a request. Usually returns empty.
pub fn annotate_request(view: &RequestView<'_>) -> Vec<HttpAnnotation> {
    let mut out = Vec::new();
    if let Some(a) = sls::annotate(view) {
        out.push(a);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_traffic_gets_no_annotations() {
        let view = RequestView::new(
            "GET",
            "https://route-5.example.com/api/clientgate/routes?platform=Steam_Win",
            &[],
        );
        assert!(annotate_request(&view).is_empty());
    }

    #[test]
    fn a_recognized_request_is_annotated() {
        let view = RequestView::new(
            "GET",
            "https://example-client.cn-hongkong.log.aliyuncs.com/logstores/client/track?log_category=login_stats",
            &[],
        );
        let annotations = annotate_request(&view);
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].kind, "sls_beacon");
    }
}

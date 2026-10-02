//! Fake secrets only. Exercise the actual disk + live observation sinks.
use akagi::{
    config::NativeApiConfig,
    logger::{LogTarget, Session},
    schema::*,
};
use base64::Engine;
use serde_json::json;
const SECRET: &str = "FAKE-password-71e9-DO-NOT-RECORD";

fn scan(path: &std::path::Path) {
    for entry in std::fs::read_dir(path).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            scan(&p);
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let bytes = std::fs::read(&p).unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(SECRET);
        let hex: String = SECRET.bytes().map(|b| format!("{b:02x}")).collect();
        for needle in [SECRET, &b64, &hex] {
            assert!(
                !bytes.windows(needle.len()).any(|w| w == needle.as_bytes()),
                "secret in output file"
            );
        }
    }
}

#[test]
fn fake_login_never_reaches_any_observation_sink() {
    let tmp = tempfile::tempdir().unwrap();
    let session = Session::init(
        tmp.path(),
        "trace",
        "trace",
        &[LogTarget::new("protocol", "akagi::bridge")],
    )
    .unwrap();
    let mut logs = session.subscribe();
    let mut frames = session.subscribe_inspector();
    let writer = session.inspector();
    // Exercise real protobuf decoding and bridge logging, not only the writer.
    use akagi::bridge::{Bridge, Direction, MajsoulBridge};
    let flow = session
        .flow_logger("flows", "actual-login.log", "login")
        .unwrap();
    let mut observed = MajsoulBridge::new(Some(flow), None);
    let mut control = MajsoulBridge::new(None, None);
    let request = wire(
        2,
        ".lq.Lobby.login",
        "lq.ReqLogin",
        json!({"account":SECRET,"password":SECRET}),
    );
    let response = wire(
        3,
        "",
        "lq.ResLogin",
        json!({"accountId":42,"accessToken":SECRET}),
    );
    for (direction, frame) in [
        (Direction::Up, request),
        (Direction::Down, response),
        (Direction::Up, SECRET.as_bytes().to_vec()),
    ] {
        let parsed = observed.parse(direction, &frame);
        let reference = control.parse(direction, &frame);
        assert_eq!(
            parsed.events, reference.events,
            "observing must not alter gameplay events"
        );
        assert_eq!(
            parsed.parsed, reference.parsed,
            "internal parser data must remain intact"
        );
        writer.record(InspectorEntry::WsFrame {
            ts_ms: 1,
            direction: FrameDirection::Up,
            flow_id: "wire:1".into(),
            size: frame.len(),
            raw: FrameRaw::Binary(base64::engine::general_purpose::STANDARD.encode(frame)),
            parsed: parsed.parsed,
            emitted: 0,
        });
    }
    drop(observed);
    for method in [".lq.Lobby.login", ".lq.Lobby.oauth2Login", SECRET] {
        for direction in [FrameDirection::Up, FrameDirection::Down] {
            writer.record(InspectorEntry::WsFrame {ts_ms:1, direction, flow_id:"test:1".into(), size:SECRET.len(),
                raw:FrameRaw::Binary(base64::engine::general_purpose::STANDARD.encode(SECRET)),
                parsed:Some(ParsedFrame {method:method.into(), args:json!({"msg_id":1,"type":"request","password":SECRET,"access_token":SECRET})}), emitted:0});
        }
    }
    writer.record(InspectorEntry::WsFrame {
        ts_ms: 1,
        direction: FrameDirection::Up,
        flow_id: "test:2".into(),
        size: SECRET.len(),
        raw: FrameRaw::Text(SECRET.into()),
        parsed: None,
        emitted: 0,
    });
    let http = InspectorEntry::Http {
        ts_ms: 1,
        source: CaptureSource::Chromium,
        exchange: HttpExchange {
            exchange_id: Some("1".into()),
            phase: HttpPhase::Request,
            method: "POST".into(),
            url: format!(
                "https://{SECRET}:{SECRET}@game.maj-soul.com/login?password={SECRET}#{SECRET}"
            ),
            host: "game.maj-soul.com".into(),
            version: "HTTP/2".into(),
            status: None,
            headers: [
                "Cookie",
                "Authorization",
                "Set-Cookie",
                "X-Secret",
                "Content-Type",
                "Content-Length",
            ]
            .map(|h| HttpHeader {
                name: h.into(),
                value: SECRET.into(),
            })
            .to_vec(),
            body: Some(HttpBody {
                text: Some(SECRET.into()),
                bytes: Some(SECRET.len()),
                skipped: None,
            }),
            annotations: vec![HttpAnnotation {
                kind: "sls_beacon".into(),
                summary: SECRET.into(),
                data: json!({"secret":SECRET}),
            }],
        },
    };
    writer.record(http.clone());
    let mut response = http;
    if let InspectorEntry::Http { exchange, .. } = &mut response {
        exchange.phase = HttpPhase::Response;
        exchange.status = Some(200);
    }
    writer.record(response);

    let cfg = NativeApiConfig {
        key: SECRET.into(),
        proxy: format!("http://{SECRET}:{SECRET}@localhost:8080"),
        ..Default::default()
    };
    assert!(!format!("{cfg:?}").contains(SECRET));
    tracing::warn!(target:"akagi::bridge::majsoul", error = SECRET, "failed login: {SECRET}");
    tracing::error!(target:"chromiumoxide::handler", "bad response {SECRET}");
    tracing::info!(target:"akagi::config", "{cfg:?}");
    session
        .binary_logger("frames")
        .unwrap()
        .log(0, SECRET.as_bytes())
        .unwrap();
    session
        .flow_logger("flows", "login.log", "test")
        .unwrap()
        .writeln(
            &json!({"payload":SECRET,"error":SECRET,"method":SECRET,"type":SECRET,"msg_id":1})
                .to_string(),
        );
    while let Ok(e) = logs.try_recv() {
        assert!(!serde_json::to_string(&e).unwrap().contains(SECRET));
    }
    while let Ok(e) = frames.try_recv() {
        assert!(!serde_json::to_string(&e).unwrap().contains(SECRET));
    }
    // Buffered text sinks flush when Session's guards drop.
    drop(writer);
    drop(session);
    scan(tmp.path());
}

fn wire(kind: u8, name: &str, message: &str, value: serde_json::Value) -> Vec<u8> {
    use prost::Message;
    #[derive(prost::Message)]
    struct Wrapper {
        #[prost(string, tag = "1")]
        name: String,
        #[prost(bytes = "vec", tag = "2")]
        data: Vec<u8>,
    }
    let descriptor = akagi::bridge::majsoul::parser::POOL
        .get_message_by_name(message)
        .unwrap();
    let inner = prost_reflect::DynamicMessage::deserialize(descriptor, value).unwrap();
    let wrapper = Wrapper {
        name: name.into(),
        data: inner.encode_to_vec(),
    };
    let mut frame = vec![kind, 1, 0];
    frame.extend(wrapper.encode_to_vec());
    frame
}

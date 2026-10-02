//! Manual macOS acceptance. No secrets in argv/env/output; no screenshots,
//! console subscribers, network dumps, root trust changes, proxy or cloud bots.
use akagi::{
    bridge::majsoul::parser::{LiqiParser, MessageType},
    capture::chromium::launch,
    config::ChromiumConfig,
    inspector::InspectorWriter,
    schema::*,
    util::{credentials::Credentials, private_fs},
};
use base64::Engine;
use chromiumoxide::{
    cdp::browser_protocol::network::{
        EnableParams, EventWebSocketFrameReceived, EventWebSocketFrameSent,
    },
    Browser,
};
use futures_util::StreamExt;
use std::{
    collections::HashMap,
    path::Path,
    time::{Duration, Instant},
};
const URL: &str = "https://game.maj-soul.com/1/";
const UNITY_FIELD: &str = include_str!("support/unity_login.js");
const CHROME: &str = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
// Only unambiguous visible fields in a same-origin login form. Canvas clients
// that expose no semantic form fail closed; never guess coordinates or type
// credentials into an unknown widget.
const FORM: &str = r#"() => {
 if(location.href !== 'https://game.maj-soul.com/1/' || window.top !== window) return 'ORIGIN_REJECTED';
 const visible = e => e && !e.disabled && e.getClientRects().length && getComputedStyle(e).visibility !== 'hidden';
 if([...document.querySelectorAll('iframe')].some(e=>/captcha|challenge/i.test(e.src))) return 'USER_VERIFICATION_REQUIRED';
 const pw=[...document.querySelectorAll('input[type=password]')].filter(visible);
 const email=[...document.querySelectorAll('input[type=email],input[autocomplete=username]')].filter(visible);
 if(pw.length!==1 || email.length!==1 || !pw[0].form || pw[0].form!==email[0].form) return 'WAIT_LOGIN_FORM';
 const form=pw[0].form;
 if(new URL(form.action,location.href).origin!==location.origin) return 'ORIGIN_REJECTED';
 const submit=[...form.querySelectorAll('button,input[type=submit]')].filter(e=>visible(e)&&e.type==='submit'&&/^(登录|登入|登錄|ログイン|log in|login|sign in)$/i.test((e.textContent||e.value||'').trim()));
 if(submit.length!==1) return 'WAIT_LOGIN_FORM';
 return 'FORM_READY';
}"#;

// A cancellation or outer timeout still scans the synchronous metadata log
// while credentials are in memory. No credential bytes escape this guard.
struct LogAudit<'a> {
    credentials: &'a Credentials,
    log: &'a Path,
    checked: Option<bool>,
}
impl LogAudit<'_> {
    fn check(&mut self) -> bool {
        if let Some(passed) = self.checked {
            return passed;
        }
        let passed = std::fs::read(self.log)
            .map(|bytes| self.credentials.absent_from(&bytes))
            .unwrap_or(false);
        println!(
            "{}",
            if passed {
                "CREDENTIAL_LEAK_CHECK_PASSED"
            } else {
                "CREDENTIAL_LEAK_CHECK_FAILED"
            }
        );
        self.checked = Some(passed);
        passed
    }
}
impl Drop for LogAudit<'_> {
    fn drop(&mut self) {
        self.check();
    }
}

#[tokio::main]
async fn main() {
    // Dependencies must never print panic payloads which could contain CDP input.
    std::panic::set_hook(Box::new(|_| eprintln!("ACCEPTANCE_PANIC")));
    #[cfg(unix)]
    unsafe {
        libc::umask(0o077);
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    let probe = args.iter().any(|arg| arg == "--probe");
    let assisted = args.iter().any(|arg| arg == "--unity-assisted");
    let status = run(probe, assisted).await.unwrap_or("ACCEPTANCE_FAILED");
    println!("{status}");
    if !matches!(status, "LOBBY_PROTOCOL_CONFIRMED" | "LOGIN_FORM_READY") {
        std::process::exit(1);
    }
}
async fn run(probe: bool, assisted: bool) -> Result<&'static str, Box<dyn std::error::Error>> {
    let tmp = tempfile::Builder::new().prefix("akagi-login-").tempdir()?;
    let profile = tmp.path().join("profile");
    private_fs::directory(&profile)?;
    let cfg = ChromiumConfig {
        start_url: "about:blank".into(),
        ..Default::default()
    };
    let mut launched = launch::spawn(Path::new(CHROME), &profile, &cfg)?;
    let result = tokio::select! {
        result=tokio::time::timeout(Duration::from_secs(600), session(&profile, launched.remote_debugging_port,tmp.path(), probe, assisted)) => result,
        _=tokio::signal::ctrl_c() => Ok(Ok("ACCEPTANCE_CANCELLED")),
    };
    launch::terminate(&mut launched.child).await;
    // No copies of the profile or credentials are retained, even on failure.
    let status = match result {
        Ok(r) => r.unwrap_or("ACCEPTANCE_FAILED"),
        Err(_) => "ACCEPTANCE_TIMEOUT",
    };
    tmp.close()?;
    Ok(status)
}
async fn session(
    profile: &Path,
    port: Option<u16>,
    root: &Path,
    probe: bool,
    assisted: bool,
) -> Result<&'static str, Box<dyn std::error::Error>> {
    let endpoint = launch::wait_for_devtools_endpoint(profile, port).await?;
    let (mut browser, mut handler) = Browser::connect(endpoint).await?;
    let pump = tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            if event.is_err() {
                break;
            }
        }
    });
    let result = exercise(&browser, root, probe, assisted).await;
    let _ = browser.close().await;
    pump.abort();
    result
}
async fn exercise(
    browser: &Browser,
    root: &Path,
    probe: bool,
    assisted: bool,
) -> Result<&'static str, Box<dyn std::error::Error>> {
    let page = browser.new_page("about:blank").await?;
    page.execute(EnableParams::default()).await?;
    let mut sent = page.event_listener::<EventWebSocketFrameSent>().await?;
    let mut received = page.event_listener::<EventWebSocketFrameReceived>().await?;
    let log = root.join("inspector.jsonl");
    let (writer, _) = InspectorWriter::open(&log, 16)?;
    let page = match page.goto(URL).await {
        Ok(page) => page,
        Err(_) => return Ok("OFFICIAL_PAGE_UNREACHABLE"),
    };
    if assisted {
        println!("USER_FOCUS_EMAIL_FIELD");
    } else {
        println!("WAIT_LOGIN_FORM");
    }
    let ready = Instant::now();
    let mut verification = false;
    loop {
        let script = if assisted {
            format!("() => ({UNITY_FIELD})('email')")
        } else {
            FORM.into()
        };
        let state: String = page.evaluate(script).await?.into_value()?;
        match state.as_str() {
            "FORM_READY" | "FIELD_READY" => break,
            "USER_VERIFICATION_REQUIRED" => {
                if !verification {
                    println!("USER_VERIFICATION_REQUIRED");
                    verification = true;
                }
            }
            "ORIGIN_REJECTED" => return Ok("ORIGIN_REJECTED"),
            _ => {}
        }
        if ready.elapsed() > Duration::from_secs(240) {
            if assisted {
                return Ok("USER_FOCUS_EMAIL_REQUIRED");
            }
            if verification {
                return Ok("USER_VERIFICATION_REQUIRED");
            }
            let client: u8 = page.evaluate("() => document.querySelector('#unity-canvas') ? (window.unityInstance ? 2 : 1) : 0").await?.into_value()?;
            return Ok(match client {
                2 => "UNITY_LOGIN_CONTROLS_UNSUPPORTED",
                1 => "UNITY_CLIENT_NOT_READY",
                _ => "LOGIN_FORM_UNSUPPORTED",
            });
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    if probe {
        return Ok("LOGIN_FORM_READY");
    }
    // This is the only credential read; only after confirming the official form.
    let credentials =
        match Credentials::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("account")) {
            Ok(credentials) => credentials,
            Err(_) => return Ok("CREDENTIAL_FILE_REJECTED"),
        };
    let mut audit = LogAudit {
        credentials: &credentials,
        log: &log,
        checked: None,
    };
    let result: Result<&'static str, Box<dyn std::error::Error>> = async {
    if assisted {
        let email = serde_json::to_string(&credentials.email)?;
        let state: String = page.evaluate(format!("() => ({UNITY_FIELD})('email', {email})")).await?.into_value()?;
        if state != "FIELD_FILLED" { return Ok("EMAIL_FIELD_REJECTED"); }
        println!("EMAIL_FILLED_USER_FOCUS_PASSWORD");
        let deadline = Instant::now();
        loop {
            let state: String = page.evaluate(format!("() => ({UNITY_FIELD})('password')")).await?.into_value()?;
            if state == "ORIGIN_REJECTED" { return Ok("ORIGIN_REJECTED"); }
            if state == "FIELD_READY" { break; }
            if state == "USER_VERIFICATION_REQUIRED" && !verification {
                println!("USER_VERIFICATION_REQUIRED"); verification = true;
            }
            if deadline.elapsed() > Duration::from_secs(180) { return Ok("USER_FOCUS_PASSWORD_REQUIRED"); }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        let password = serde_json::to_string(&credentials.password)?;
        let state: String = page.evaluate(format!("() => ({UNITY_FIELD})('password', {password})")).await?.into_value()?;
        if state != "FIELD_FILLED" { return Ok("PASSWORD_FIELD_REJECTED"); }
        println!("PASSWORD_FILLED_USER_CLICK_LOGIN");
    } else {
    let email = serde_json::to_string(&credentials.email)?;
    let password = serde_json::to_string(&credentials.password)?;
    let script = format!(
        r#"() => {{
      if (({FORM})() !== 'FORM_READY') return false;
      const visible=e=>!e.disabled&&e.getClientRects().length&&getComputedStyle(e).visibility!=='hidden';
      const p=[...document.querySelectorAll('input[type=password]')].filter(visible)[0];
      const e=[...document.querySelectorAll('input[type=email],input[autocomplete=username]')].filter(visible)[0];
      const set=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;
      set.call(e,{email}); set.call(p,{password});
      for(const input of [e,p]){{input.dispatchEvent(new Event('input',{{bubbles:true}}));input.dispatchEvent(new Event('change',{{bubbles:true}}));}}
      const submit=[...p.form.querySelectorAll('button,input[type=submit]')].find(e=>e.type==='submit'&&/^(登录|登入|登錄|ログイン|log in|login|sign in)$/i.test((e.textContent||e.value||'').trim()));
      p.form.requestSubmit(submit); return true;
    }}"#
    );
    let filled: bool = page.evaluate(script).await?.into_value()?;
    if !filled {
        return Ok("ORIGIN_REJECTED");
    }
    println!("LOGIN_SUBMITTED");
    }
    let mut parsers: HashMap<String, LiqiParser> = HashMap::new();
    let until = Instant::now();
    let mut outcome = "LOBBY_NOT_CONFIRMED";
    loop {
        if until.elapsed() > Duration::from_secs(150) { break; }
        tokio::select! {
            biased; // Correlate queued requests before their responses.
            Some(ev)=sent.next()=>{
                observe(&writer,&mut parsers,ev.request_id.inner(),&ev.response.payload_data,FrameDirection::Up);
            }
            Some(ev)=received.next()=>{
                if observe(&writer,&mut parsers,ev.request_id.inner(),&ev.response.payload_data,FrameDirection::Down) {
                    outcome="LOBBY_PROTOCOL_CONFIRMED";break;
                }
            }
            _=tokio::time::sleep(Duration::from_millis(500))=>{
                let state:String=page.evaluate(FORM).await?.into_value()?;
                if state=="USER_VERIFICATION_REQUIRED" && !verification {println!("USER_VERIFICATION_REQUIRED");verification=true;}
                if until.elapsed()>Duration::from_secs(150){break;}
            }
        }
    }
    Ok(outcome)
    }.await;
    drop(writer);
    if !audit.check() {
        return Ok("CREDENTIAL_LEAK_CHECK_FAILED");
    }
    result
}
fn observe(
    writer: &InspectorWriter,
    parsers: &mut HashMap<String, LiqiParser>,
    flow: &str,
    raw: &str,
    direction: FrameDirection,
) -> bool {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw)
        .unwrap_or_default();
    let parsed = parsers.entry(flow.into()).or_default().parse(&bytes).ok();
    let success = parsed.as_ref().is_some_and(|p| {
        p.msg_type == MessageType::Response
            && p.method_name.as_ref() == ".lq.Lobby.loginSuccess"
            && p.payload
                .get("error")
                .is_none_or(|e| e.get("code").and_then(|c| c.as_u64()).unwrap_or(0) == 0)
    });
    writer.record(InspectorEntry::WsFrame {
        ts_ms: chrono::Utc::now().timestamp_millis(),
        direction,
        flow_id: flow.into(),
        size: bytes.len(),
        raw: FrameRaw::Redacted(akagi::privacy::OMITTED.into()),
        parsed: parsed.map(|p| ParsedFrame {
            method: p.method_name.to_string(),
            args: serde_json::json!({"msg_id":p.msg_id}),
        }),
        emitted: 0,
    });
    success
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audit_rejects_plain_encoded_and_missing_logs() {
        let temp = tempfile::tempdir().unwrap();
        let log = temp.path().join("inspector.jsonl");
        let credentials = Credentials {
            email: "synthetic@example.test".into(),
            password: "synthetic-test-password".into(),
        };
        for content in [
            credentials.password.clone(),
            base64::engine::general_purpose::STANDARD.encode(&credentials.password),
        ] {
            std::fs::write(&log, content).unwrap();
            assert!(!LogAudit {
                credentials: &credentials,
                log: &log,
                checked: None
            }
            .check());
        }
        std::fs::write(&log, "{\"raw\":{\"kind\":\"redacted\"}}").unwrap();
        assert!(LogAudit {
            credentials: &credentials,
            log: &log,
            checked: None
        }
        .check());
        std::fs::remove_file(&log).unwrap();
        assert!(!LogAudit {
            credentials: &credentials,
            log: &log,
            checked: None
        }
        .check());
    }
}

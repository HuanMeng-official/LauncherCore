//! Output abstraction shared by the human CLI and the JSON server.
//!
//! In human mode, helpers print to stdout/stderr as usual. In JSON mode,
//! human-readable messages become NDJSON log events, while progress and
//! interactive events are emitted as structured objects.

use serde_json::{Value, json};
use std::io::Write;
use std::sync::{Mutex, OnceLock};

static JSON_MODE: OnceLock<()> = OnceLock::new();
static CURRENT_REQUEST: OnceLock<Mutex<Option<Value>>> = OnceLock::new();
static WRITE_LOCK: Mutex<()> = Mutex::new(());

pub fn init_json() {
    let _ = JSON_MODE.set(());
}

pub fn is_json() -> bool {
    JSON_MODE.get().is_some()
}

fn request_slot() -> &'static Mutex<Option<Value>> {
    CURRENT_REQUEST.get_or_init(|| Mutex::new(None))
}

pub fn set_request(id: Option<Value>) {
    *request_slot().lock().unwrap() = id;
}

pub fn clear_request() {
    set_request(None);
}

fn current_request() -> Value {
    request_slot()
        .lock()
        .unwrap()
        .clone()
        .unwrap_or(Value::Null)
}

/// Writes one NDJSON object to stdout. Requests are serialized with a lock so
/// concurrent download tasks cannot interleave partial lines.
pub fn write_event(mut event: Value) {
    if !is_json() {
        return;
    }

    if event.get("id").is_none() {
        event["id"] = current_request();
    }

    let Ok(line) = serde_json::to_string(&event) else {
        return;
    };

    let _guard = WRITE_LOCK.lock().unwrap();
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{}", line);
    let _ = stdout.flush();
}

pub fn info(message: impl Into<String>) {
    let message = message.into();
    if is_json() {
        write_event(json!({"type": "log", "level": "info", "message": message}));
    } else {
        println!("{}", message);
    }
}

pub fn warn(message: impl Into<String>) {
    let message = message.into();
    if is_json() {
        write_event(json!({"type": "log", "level": "warn", "message": message}));
    } else {
        eprintln!("{}", message);
    }
}

pub fn progress(phase: &str, current: u64, total: u64, message: impl Into<String>) {
    if is_json() {
        write_event(json!({
            "type": "progress",
            "phase": phase,
            "current": current,
            "total": total,
            "message": message.into()
        }));
    }
}

pub fn device_code(code: &crate::common::models::DeviceCodeResponse) {
    if is_json() {
        write_event(json!({
            "type": "device_code",
            "user_code": code.user_code,
            "verification_uri": code.verification_uri,
            "expires_in": code.expires_in,
            "message": code.message
        }));
    } else {
        println!("{}", code.message);
    }
}

pub fn profiles(profiles: &[crate::common::models::YggdrasilProfile]) {
    if is_json() {
        write_event(json!({
            "type": "profiles",
            "profiles": profiles
        }));
    }
}

#[macro_export]
macro_rules! outln {
    ($($arg:tt)*) => {
        $crate::common::output::info(format!($($arg)*))
    };
}

#[macro_export]
macro_rules! outwarn {
    ($($arg:tt)*) => {
        $crate::common::output::warn(format!($($arg)*))
    };
}

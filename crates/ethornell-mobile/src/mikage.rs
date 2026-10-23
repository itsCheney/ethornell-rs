//! Calls added for the Mikage player (itsCheney/Mikage-Next), next to the
//! host ABI. They don't change any ABI v1 behaviour.
//!
//! `ethornell_mikage_set_log_callback` sends the engine's `tracing` events
//! (warnings and errors by default) and panic messages to the host, which
//! has no other way to see them on iOS: the ABI's diagnostics are redacted
//! classes and there is no subscriber.
use std::{
    ffi::{CString, c_char},
    fmt::Write as _,
    sync::{Once, RwLock},
};
use tracing::{Event, Level, Subscriber, field::{Field, Visit}};
use tracing_subscriber::{EnvFilter, Layer, layer::Context, prelude::*};

/// `level`: 1 error, 2 warn, 3 info, 4 debug, 5 trace. `target` and
/// `message` are NUL-terminated UTF-8, valid for the duration of the call.
/// It can be called from any thread (audio workers log too).
pub type LogCallback = extern "C" fn(level: i32, target: *const c_char, message: *const c_char);

static CALLBACK: RwLock<Option<LogCallback>> = RwLock::new(None);
static INSTALL: Once = Once::new();

fn send(level: i32, target: &str, message: &str) {
    let Some(callback) = *CALLBACK.read().unwrap_or_else(|e| e.into_inner()) else {
        return;
    };
    let clean = |text: &str| CString::new(text.replace('\0', " ")).unwrap_or_default();
    let (target, message) = (clean(target), clean(message));
    callback(level, target.as_ptr(), message.as_ptr());
}

struct Message(String);
impl Visit for Message {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0.insert_str(0, value);
        } else {
            let _ = write!(self.0, " {}={value}", field.name());
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0.insert_str(0, &format!("{value:?}"));
        } else {
            let _ = write!(self.0, " {}={value:?}", field.name());
        }
    }
}

struct Forward;
impl<S: Subscriber> Layer<S> for Forward {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let metadata = event.metadata();
        let level = match *metadata.level() {
            Level::ERROR => 1,
            Level::WARN => 2,
            Level::INFO => 3,
            Level::DEBUG => 4,
            Level::TRACE => 5,
        };
        let mut message = Message(String::new());
        event.record(&mut message);
        send(level, metadata.target(), message.0.trim_start());
    }
}

/// Installs (once) a process-wide subscriber and panic hook that forward to
/// `callback`; a later call replaces the callback, NULL stops forwarding.
/// `filter` is an `EnvFilter` directive such as `ethornell=info`; NULL or
/// empty keeps the default `ethornell=warn,wgpu=warn`. Only the first call's
/// filter takes effect.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ethornell_mikage_set_log_callback(
    callback: Option<LogCallback>,
    filter: *const c_char,
) {
    *CALLBACK.write().unwrap_or_else(|e| e.into_inner()) = callback;
    let filter = (!filter.is_null())
        .then(|| unsafe { std::ffi::CStr::from_ptr(filter) }.to_string_lossy().into_owned())
        .filter(|filter| !filter.trim().is_empty())
        .unwrap_or_else(|| "ethornell=warn,wgpu=warn".into());
    INSTALL.call_once(|| {
        let filter = EnvFilter::try_new(&filter)
            .unwrap_or_else(|_| EnvFilter::new("ethornell=warn,wgpu=warn"));
        let _ = tracing_subscriber::registry().with(Forward.with_filter(filter)).try_init();
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let location = info
                .location()
                .map(|l| format!(" at {}:{}", l.file(), l.line()))
                .unwrap_or_default();
            let payload = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic".into());
            send(1, "ethornell::panic", &format!("{payload}{location}"));
            previous(info);
        }));
    });
}

/// The game's own screen size in its pixels (1280x720 until the game sets
/// one). Status as for the ABI calls: 0, or a negative error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ethornell_mikage_screen_size(
    handle: u64,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    if width.is_null() || height.is_null() {
        return super::fail(super::ARG, "null output");
    }
    let mut size = (0, 0);
    let status = super::with_session(handle, true, |runtime| {
        size = runtime.screen_size();
        Ok(())
    });
    if status == 0 {
        unsafe {
            *width = size.0.max(0) as u32;
            *height = size.1.max(0) as u32;
        }
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    static SEEN: Mutex<Vec<(i32, String, String)>> = Mutex::new(Vec::new());
    extern "C" fn record(level: i32, target: *const c_char, message: *const c_char) {
        let text = |p| unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned();
        SEEN.lock().unwrap().push((level, text(target), text(message)));
    }
    #[test]
    fn forwards_warnings_and_panics_but_not_info() {
        unsafe { ethornell_mikage_set_log_callback(Some(record), std::ptr::null()) };
        tracing::warn!(target: "ethornell::test", code = 7, "first {}", "warning");
        tracing::info!(target: "ethornell::test", "not forwarded");
        let _ = std::panic::catch_unwind(|| panic!("boom"));
        unsafe { ethornell_mikage_set_log_callback(None, std::ptr::null()) };
        tracing::error!(target: "ethornell::test", "after removal");
        // Other tests run in parallel and may panic too: keep this test's own.
        let seen = SEEN.lock().unwrap().iter()
            .filter(|(_, target, message)| target == "ethornell::test" || message.starts_with("boom"))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(seen[0], (2, "ethornell::test".into(), "first warning code=7".into()));
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert_eq!(seen[1].0, 1);
        assert!(seen[1].2.starts_with("boom at "), "{seen:?}");
    }
}

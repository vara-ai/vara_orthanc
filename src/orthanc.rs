include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

use std::ffi::CString;
use libc::c_void;
use std::env;
use std::sync::RwLock;

enum LogLevel {
    Info,
    Error,
    Warning,
}

pub struct PluginState {
    context: Option<*mut OrthancPluginContext>
}

unsafe impl Send for PluginState {}
unsafe impl Sync for PluginState {}


pub static PLUGIN_STATE: RwLock<PluginState> = RwLock::new(PluginState {
    context: None
});

pub fn set_context(context: *mut OrthancPluginContext) {
    PLUGIN_STATE.write().unwrap().context = Some(context);
}

pub fn get_context() -> *mut OrthancPluginContext {
    PLUGIN_STATE.read().unwrap().context.unwrap()
}

pub fn invoke_orthanc_service(
    service: _OrthancPluginService,
    params: *mut c_void,
) -> OrthancPluginErrorCode {
    unsafe {
        let context = get_context();
        let invoker = (*context).InvokeService.unwrap();
        invoker(context, service, params)
    }
}

pub fn free_buffer(buffer: *mut OrthancPluginMemoryBuffer) {
    let context = get_context();
    unsafe { (*context).Free.unwrap()((*buffer).data as *mut c_void) };
}

fn log(level: LogLevel, msg: &str) {
    let msg = CString::new(msg).unwrap();
    let orthanc_plugin_service = match level {
        LogLevel::Info => _OrthancPluginService__OrthancPluginService_LogInfo,
        LogLevel::Warning => _OrthancPluginService__OrthancPluginService_LogWarning,
        LogLevel::Error => _OrthancPluginService__OrthancPluginService_LogError,
    };

    invoke_orthanc_service(orthanc_plugin_service, msg.as_ptr() as *mut c_void);
}

pub fn info(msg: &str) {
    log(LogLevel::Info, msg);
}

pub fn error(msg: &str) {
    log(LogLevel::Error, msg);
}

pub fn warning(msg: &str) {
    log(LogLevel::Warning, msg);
}

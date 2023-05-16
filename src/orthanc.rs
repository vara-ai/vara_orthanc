include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

use std::ffi::CString;
use libc::c_void;
use std::env;


enum LogLevel {
    Info,
    Error,
    Warning,
}


pub struct OrthancContext(pub *mut OrthancPluginContext);
unsafe impl Send for OrthancContext {}
unsafe impl Sync for OrthancContext {}

pub static mut orthanc_context: Option<Box<OrthancContext>> = None;

pub fn set_context(context: *mut OrthancPluginContext) {
    unsafe {
        orthanc_context = Some(Box::new(OrthancContext(context)));
    }
}

pub fn invoke_orthanc_service(
    service: _OrthancPluginService,
    params: *mut c_void,
) -> OrthancPluginErrorCode {
    unsafe {
        let context = orthanc_context.as_ref().unwrap().0;
        let invoker = (*context).InvokeService.unwrap();
        invoker(context, service, params)
    }
}

pub unsafe fn free_buffer(buffer: *mut OrthancPluginMemoryBuffer) {
    let context = orthanc_context.as_ref().unwrap().0;
    (*context).Free.unwrap()((*buffer).data as *mut c_void);
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

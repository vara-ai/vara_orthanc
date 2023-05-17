include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

use libc::c_char;
use libc::c_void;
use serde_json as json;
use std::env;
use std::ffi::CStr;
use std::ffi::CString;
use std::sync::RwLock;

#[derive(Debug)]
pub struct Endpoint {
    pub url: String,
    pub username: String,
    pub password: String
}

#[derive(Debug)]
pub struct PluginState {
    context: Option<*mut OrthancPluginContext>,
    endpoints: Vec<Endpoint>
}

unsafe impl Send for PluginState {}
unsafe impl Sync for PluginState {}

pub static PLUGIN_STATE: RwLock<PluginState> = RwLock::new(PluginState {
    context: None,
    endpoints: vec![]
});

pub fn get_context() -> *mut OrthancPluginContext {
    PLUGIN_STATE.read().unwrap().context.unwrap()
}

pub fn get_endpoints() -> Vec<Endpoint> {
    let mut endpoints = vec![];
    for endpoint in &PLUGIN_STATE.read().unwrap().endpoints {
        endpoints.push(Endpoint {
            url: endpoint.url.clone(),
            username: endpoint.username.clone(),
            password: endpoint.password.clone()
        })
    }
    endpoints
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

// Logging
// ----------------------------------------------------------------------------
enum LogLevel {
    Info,
    Error,
    Warning,
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


// Initialization and State Management
// ----------------------------------------------------------------------------

fn config() -> json::Value {
    let mut config_cstr: *mut c_char = std::ptr::null_mut();
    let mut params = _OrthancPluginRetrieveDynamicString {
        result: &mut config_cstr as *mut *mut c_char,
        argument: std::ptr::null(),
    };
    invoke_orthanc_service(
        _OrthancPluginService__OrthancPluginService_GetConfiguration,
        &mut params as *mut _OrthancPluginRetrieveDynamicString as *mut c_void,
    );

    unsafe {
        info(CStr::from_ptr(*params.result).to_str().unwrap());
    };
    let config_cstr = unsafe { CStr::from_ptr(*params.result) };
    let config_str = config_cstr.to_str().unwrap().to_string();
    unsafe { (*get_context()).Free.unwrap()(*params.result as *mut c_void) };
    // If we cannot read config as JSON, it's fine to panic.
    json::from_str(&config_str).unwrap()
}

fn get_proxy_endpoint() -> Option<(String, String, String)> {
    let c = config();
    if c["VaraProxy"] != json::json!(null) && c["VaraProxy"]["Endpoint"] != json::json!(null) {
        Some((c["VaraProxy"]["Endpoint"].to_string(),
              c["VaraProxy"]["Username"].to_string(),
              c["VaraProxy"]["Password"].to_string()))
    } else {
        None
    }
}

//
// The order of operations in this function is really important. If not done
// correctly, the plugin will deadlock Orthanc. These deadlocks will happen
// mainly because of the RwLock on Plugin State read/write locks on which
// need to be carefully acquired and dropped.
//
pub fn initialize(context: *mut OrthancPluginContext) {
    // Initialize the context as the first step so that other operations work.
    let mut plugin_state = PLUGIN_STATE.write().unwrap();
    plugin_state.context = Some(context);
    drop(plugin_state);

    // Note: get the proxy endpoint configuration before taking a write lock on
    // PLUGIN_STATE.
    let proxy_endpoint = get_proxy_endpoint();
    let mut plugin_state = PLUGIN_STATE.write().unwrap();
    plugin_state.endpoints = match proxy_endpoint {
        None => vec![],
        Some((url, username, password)) => vec![Endpoint {url, username, password}]
    };
}

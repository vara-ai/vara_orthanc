include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

use libc::c_char;
use libc::c_void;
use reqwest::blocking::Client as HttpClient;
use serde_json as json;
use std::env;
use std::ffi::CStr;
use std::ffi::CString;
use std::sync::RwLock;
use threadpool::ThreadPool;

#[derive(Debug)]
pub struct PluginState {
    pub http_client: Option<HttpClient>,
    pub context: Option<*mut OrthancPluginContext>,
    pub config: Option<json::Value>,
    pub threadpool: Option<ThreadPool>,
}

unsafe impl Send for PluginState {}
unsafe impl Sync for PluginState {}

pub static PLUGIN_STATE: RwLock<PluginState> = RwLock::new(PluginState {
    http_client: None,
    context: None,
    config: None,
    threadpool: None,
});

pub fn get_context() -> *mut OrthancPluginContext {
    PLUGIN_STATE.read().unwrap().context.unwrap()
}

pub fn get_config() -> json::Value {
    // TODO: Figure out how to share config without cloning it.
    PLUGIN_STATE.read().unwrap().config.clone().unwrap()
}

pub fn get_threadpool() -> ThreadPool {
    // Cloning a ThreadPool creates a new handle from the same threadpool:
    // https://docs.rs/threadpool/latest/src/threadpool/lib.rs.html#639-682
    PLUGIN_STATE.read().unwrap().threadpool.clone().unwrap()
}

pub fn invoke_orthanc_service(
    service: _OrthancPluginService,
    params: *mut c_void,
) -> OrthancPluginErrorCode {
    unsafe {
        let context = get_context();
        let invoker = (&*context).InvokeService.unwrap();
        invoker(context, service, params)
    }
}

pub fn free_buffer(buffer: *mut OrthancPluginMemoryBuffer) {
    let context = get_context();
    unsafe { (&*context).Free.unwrap()((*buffer).data as *mut c_void) };
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

// This function takes a pointer to the context as a parameter so as to avoid
// taking a Read Lock on PLUGIN_STATE.
fn get_orthanc_config(context: *mut _OrthancPluginContext_t) -> json::Value {
    let mut config_cstr: *mut c_char = std::ptr::null_mut();
    let mut params = _OrthancPluginRetrieveDynamicString {
        result: &mut config_cstr as *mut *mut c_char,
        argument: std::ptr::null(),
    };
    unsafe {
        (&*context).InvokeService.unwrap()(
            context,
            _OrthancPluginService__OrthancPluginService_GetConfiguration,
            &mut params as *mut _OrthancPluginRetrieveDynamicString as *mut c_void,
        );
    }
    let config_cstr = unsafe { CStr::from_ptr(*params.result) };
    let config_str = config_cstr.to_str().unwrap().to_string();
    unsafe { (&*context).Free.unwrap()(*params.result as *mut c_void) };
    // If we cannot read config as JSON, it's fine to panic.
    json::from_str(&config_str).unwrap()
}

pub fn get_plugin_enabled() -> bool {
    let c = get_config();

    match c["VaraProxy"]["Enable"] {
        json::Value::Null => false,
        json::Value::Bool(b) => b,
        _ => panic!("Non-boolean provided for Enable option in VaraProxy plugin"),
    }
}

pub fn get_local_endpoint() -> super::Endpoint {
    let c = get_config();
    // An user with the username "admin" must be configured locally on the proxy
    // instance.
    let password = c["RegisteredUsers"]["admin"].as_str().unwrap().to_string();
    super::Endpoint {
        url: format!("http://localhost:{}", c["HttpPort"].to_string()),
        username: String::from("admin"),
        password,
    }
}

pub fn get_peer_identifier() -> String {
    let c = get_config();
    c["VaraProxy"]["Peer"].as_str().unwrap().to_string()
}

pub fn get_peer_endpoint() -> Option<super::Endpoint> {
    let c = get_config();
    if c["VaraProxy"] != json::json!(null) && c["VaraProxy"]["Peer"] != json::json!(null) {
        let peer_identifier = c["VaraProxy"]["Peer"].as_str().unwrap();

        if c["OrthancPeers"][peer_identifier] == json::json!(null) {
            error(&format!(
                "Please configure peer identifier: {}",
                peer_identifier
            ));
            return None;
        }

        let peer_coords: Vec<String> = c["OrthancPeers"][peer_identifier]
            .as_array()
            .unwrap()
            .into_iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        Some(super::Endpoint {
            url: peer_coords[0].to_string(),
            username: peer_coords[1].to_string(),
            password: peer_coords[2].to_string(),
        })
    } else {
        error("Please configure \"VaraProxy\" -> \"Peer\" in orthanc configuration.");
        None
    }
}

pub fn get_sync_interval() -> u64 {
    let config = get_config();
    config["VaraProxy"]["PeriodicSyncIntervalSeconds"].as_u64().unwrap_or(600)
}

//
// The order of operations in this function is really important. If not done
// correctly, the plugin will deadlock Orthanc. These deadlocks will happen
// mainly because of the RwLock on Plugin State read/write locks on which
// need to be carefully acquired and dropped.
//
// Avoid doing any read operations on the PLUGIN_STATE in this function. Even
// logging (with functions defined in this module) is an Orthanc service that
// requires reading the PLUGIN_STATE.
pub fn initialize(context: *mut OrthancPluginContext) {
    let config = get_orthanc_config(context);
    let mut plugin_state = PLUGIN_STATE.write().unwrap();
    plugin_state.context = Some(context);
    plugin_state.config = Some(config);
    plugin_state.http_client = Some(HttpClient::new());
    // Arbitrarily chosen 8 threads, most of the work happening in these threads
    // is I/O so the number can be significantly higher than the number of CPUs
    // on the machine (https://crates.io/crates/num_cpus).
    plugin_state.threadpool = Some(ThreadPool::new(8))
}

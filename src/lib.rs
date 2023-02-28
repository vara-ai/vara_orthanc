#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

extern crate reqwest;
extern crate serde_json;
extern crate tracing;

pub mod cache;
pub mod orthanc;

use crate::orthanc::OrthancPluginContext;
use crate::orthanc::OrthancPluginErrorCode;
use crate::orthanc::OrthancPluginErrorCode_OrthancPluginErrorCode_Success as OrthancCodeSuccess;
use crate::orthanc::OrthancPluginMemoryBuffer;
use crate::orthanc::OrthancPluginWorklistAnswers;
use crate::orthanc::OrthancPluginWorklistQuery;

use serde_json::Value as JsonValue;

use libc::{c_char, c_void};
use std::env;
use std::ffi::CString;
use std::path::Path;
use std::vec::Vec;

enum LogLevel {
    Info,
    Error,
    Warning,
}

struct OrthancContext(*mut OrthancPluginContext);
unsafe impl Send for OrthancContext {}
unsafe impl Sync for OrthancContext {}

static mut orthanc_context: Option<OrthancContext> = None;

#[no_mangle]
pub unsafe extern "C" fn OrthancPluginInitialize(context: *mut OrthancPluginContext) -> i32 {
    orthanc_context = Some(OrthancContext(context));
    // Before any of the services provided by Orthanc core (including logging)
    // are used, `orthanc_context` must be initialized.
    info("Initializing Vara Orthanc Worklist plugin.");
    register_on_worklist_callback(on_worklist_callback);
    info("Vara Orthanc Worklist plugin initialization complete.");
    return 0;
}

#[no_mangle]
pub unsafe extern "C" fn OrthancPluginFinalize() {
    info("Vara Ortahnc Worklist plugin finalized.");
}

#[no_mangle]
pub extern "C" fn OrthancPluginGetName() -> *const u8 {
    "Vara Orthanc\0".as_ptr()
}

#[no_mangle]
pub extern "C" fn OrthancPluginGetVersion() -> *const u8 {
    "0.1.0\0".as_ptr()
}

unsafe extern "C" fn on_worklist_callback(
    answers: *mut OrthancPluginWorklistAnswers,
    query: *const OrthancPluginWorklistQuery,
    _issuerAet: *const c_char,
    _calledAet: *const c_char,
) -> OrthancPluginErrorCode {
    let (ae_title, api_host, api_port) = peer_orthanc();
    let worklist_items: Vec<JsonValue> =
        match orthanc_modality_worklist(&ae_title, &api_host, api_port) {
            Ok(JsonValue::Array(v)) => v,
            _ => {
                error("Failed to fetch modality worklist from peer Orthanc");
                return 1;
            }
        };
    for item in worklist_items {
        let worklist_item_buffer = memory_buffer();
        create_dicom(item.to_string(), worklist_item_buffer);
        if dicom_matches_query(query, worklist_item_buffer) {
            add_worklist_query_answer(answers, query, worklist_item_buffer)
        };
        free_buffer(worklist_item_buffer);
    }
    return OrthancCodeSuccess;
}

unsafe fn orthanc_modality_worklist(
    ae_title: &str,
    host: &str,
    port: u32,
) -> Result<JsonValue, Box<dyn std::error::Error>> {
    let url = format!(
        "http://{}:{}/modalities/{}/find-worklist",
        host, port, ae_title
    );

    let http_client = reqwest::blocking::Client::new();
    //
    //  Sample JSON payload that works:
    // {
    //     "0008,0005": "ISO_IR 100",
    //     "0008,0050": "1",
    //     "0010,0010": "^Test Party",
    //     "0040,0100": [
    //         {
    //             "0008,0060": "test",
    //             "0010,0010": "^Test Party",
    //             "0040,0002": "39230313"
    //         }
    //     ],
    //     "0040,1001": "1"
    // }
    //
    // Note that
    // https://dicom.nema.org/dicom/2013/output/chtml/part18/sect_F.2.html is
    // considered invalid JSON by the Orthanc core parser.
    let workitems = http_client
        .post(url)
        .body(
            r#"{"Short": true,
                  "Query": {"AccessionNumber": "*",
                            "RequestedProcedureID": null,
                            "ScheduledProcedureStepSequence": [],
                            "PatientName": null,
                            "StudyID": null,
                            "StudyInstanceUID": null}}"#,
        )
        .basic_auth("admin", Some("password"))
        .send();

    let cache_file = Path::new("vara_orthanc.json");
    let json_response = if workitems.is_err() || !workitems.as_ref().unwrap().status().is_success()
    {
        info(&format!("Reading the cache file for MWL entries. Failure: {:?}", workitems));
        match cache::read(&cache_file) {
            Ok(contents) => contents,
            Err(error) => {
                warning("Failed to read cache file");
                return Err(Box::new(error));
            }
        }
    } else {
        let response = workitems?.text().unwrap();
        cache::write(&response, &cache_file)?;
        response
    };

    Ok(serde_json::from_str(&json_response)?)
}

unsafe fn register_on_worklist_callback(
    callback: unsafe extern "C" fn(
        answers: *mut OrthancPluginWorklistAnswers,
        query: *const OrthancPluginWorklistQuery,
        _issuerAet: *const c_char,
        _calledAet: *const c_char,
    ) -> OrthancPluginErrorCode,
) {
    #[repr(C)]
    struct OnWorklistParams {
        callback: orthanc::OrthancPluginWorklistCallback,
    }
    let params = Box::new(OnWorklistParams {
        callback: Some(callback),
    });

    invoke_orthanc_service(
        orthanc::_OrthancPluginService__OrthancPluginService_RegisterWorklistCallback,
        Box::into_raw(params) as *mut c_void,
    );
}

//
// Returns a tuple with (ae_title, ae_host, ae_port) of the Orthanc peer that we
// want to communicate with.
//
unsafe fn peer_orthanc() -> (String, String, u32) {
    // By default, we send an API request to the same Orthanc instance that
    // loads this plugin.
    let default_value = (String::from("orthanc"), String::from("localhost"), 9042);
    let ae_title;
    let api_host;
    let api_port;

    match env::var("VARA_ORTHANC_AE_TITLE") {
        Ok(ae_title_) => {
            ae_title = ae_title_;
        }
        error @ Err(_) => {
            warning(&format!("VARA_ORTHANC_AE_TITLE not defined: {:?}", error));
            return default_value;
        }
    };

    match env::var("VARA_ORTHANC_API_HOST") {
        Ok(api_host_) => {
            api_host = api_host_;
        }
        error @ Err(_) => {
            warning(&format!("VARA_ORTHANC_API_HOST not defined: {:?}", error));
            return default_value;
        }
    };

    match env::var("VARA_ORTHANC_API_PORT") {
        Ok(api_port_) => {
            api_port = api_port_;
        }
        error @ Err(_) => {
            warning(&format!("VARA_ORTHANC_API_PORT not defined: {:?}", error));
            return default_value;
        }
    };

    (ae_title, api_host, api_port.parse().unwrap())
}

//
// Returns a pointer to an OrthancPluginMemoryBuffer that can be used later by
// Orthanc core to provide or receive data. The buffer is empty and no memory is
// requested from Orthanc core.
unsafe fn memory_buffer() -> *mut OrthancPluginMemoryBuffer {
    let buffer = OrthancPluginMemoryBuffer {
        data: std::ptr::null::<c_void>() as *mut c_void,
        size: 0,
    };
    Box::into_raw(Box::new(buffer))
}

unsafe fn create_dicom(dicom_json: String, target_buffer: *mut OrthancPluginMemoryBuffer) -> i32 {
    #[repr(C)]
    struct CreateDicomParams {
        target: *mut OrthancPluginMemoryBuffer,
        json: *const c_char,
        pixel_data: *const orthanc::OrthancPluginImage,
        flags: orthanc::OrthancPluginCreateDicomFlags,
        private_creator: *const c_char,
    }

    let json_cstr = CString::new(dicom_json).unwrap();
    let private_creator = CString::new("vara").unwrap();
    let params = Box::new(CreateDicomParams {
        target: target_buffer,
        json: json_cstr.as_ptr(),
        pixel_data: std::ptr::null(),
        flags: orthanc::OrthancPluginCreateDicomFlags_OrthancPluginCreateDicomFlags_None,
        private_creator: private_creator.as_ptr() as *const c_char,
    });

    invoke_orthanc_service(
        orthanc::_OrthancPluginService__OrthancPluginService_CreateDicom2,
        Box::into_raw(params) as *mut c_void,
    )
}

unsafe fn dicom_matches_query(
    query: *const OrthancPluginWorklistQuery,
    dicom: *const OrthancPluginMemoryBuffer,
) -> bool {
    #[repr(C)]
    struct QueryWorklistOperationParams {
        query: *const OrthancPluginWorklistQuery,
        dicom: *const c_void,
        size: u32,
        is_match: *mut i32,
        target: *mut orthanc::OrthancPluginMemoryBuffer,
    }

    let is_match: i32 = 0;
    let params_ptr = Box::into_raw(Box::new(QueryWorklistOperationParams {
        query,
        dicom: (*dicom).data,
        size: (*dicom).size,
        is_match: Box::into_raw(Box::new(is_match)),
        target: std::ptr::null_mut(),
    }));

    invoke_orthanc_service(
        orthanc::_OrthancPluginService__OrthancPluginService_WorklistIsMatch,
        params_ptr as *mut c_void,
    );

    (*(*Box::from_raw(params_ptr)).is_match) != 0
}

unsafe fn add_worklist_query_answer(
    answers: *mut OrthancPluginWorklistAnswers,
    query: *const OrthancPluginWorklistQuery,
    answer: *const OrthancPluginMemoryBuffer,
) {
    let params = Box::new(orthanc::_OrthancPluginWorklistAnswersOperation {
        answers,
        query,
        dicom: (*answer).data as *mut c_void,
        size: (*answer).size as u32,
    });
    invoke_orthanc_service(
        orthanc::_OrthancPluginService__OrthancPluginService_WorklistAddAnswer,
        Box::into_raw(params) as *mut c_void,
    );
}

unsafe fn free_buffer(buffer: *mut OrthancPluginMemoryBuffer) {
    let context = orthanc_context.as_ref().unwrap().0;
    (*context).Free.unwrap()(buffer as *mut c_void);
}

unsafe fn invoke_orthanc_service(
    service: orthanc::_OrthancPluginService,
    params: *mut c_void,
) -> OrthancPluginErrorCode {
    let context = orthanc_context.as_ref().unwrap().0;
    let invoker = (*context).InvokeService.unwrap();
    invoker(context, service, params)
}

unsafe fn log(level: LogLevel, msg: &str) {
    let msg = CString::new(msg).unwrap();
    let orthanc_plugin_service = match level {
        LogLevel::Info => orthanc::_OrthancPluginService__OrthancPluginService_LogInfo,
        LogLevel::Warning => orthanc::_OrthancPluginService__OrthancPluginService_LogWarning,
        LogLevel::Error => orthanc::_OrthancPluginService__OrthancPluginService_LogError,
    };

    invoke_orthanc_service(orthanc_plugin_service, msg.as_ptr() as *mut c_void);
}

unsafe fn info(msg: &str) {
    log(LogLevel::Info, msg);
}

unsafe fn error(msg: &str) {
    log(LogLevel::Error, msg);
}

unsafe fn warning(msg: &str) {
    log(LogLevel::Warning, msg);
}

#[cfg(test)]
mod test {
    #[test]
    fn test_parsing_json_response() {
        let _example_json = r#"[{"0008,0005" : "ISO_IR 100",
                             "0008,0050" : "1",
                             "0010,0010" : "^Test Party"}
                          ]"#;
    }
}

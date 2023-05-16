#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

extern crate reqwest;
extern crate serde_json;
extern crate tracing;

pub mod cache;
pub mod orthanc;

use crate::orthanc::OrthancPluginErrorCode;
use crate::orthanc::OrthancPluginContext;
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


#[no_mangle]
pub extern "C" fn OrthancPluginInitialize(context: *mut OrthancPluginContext) -> i32 {
    orthanc::set_context(context);
    // Before any of the services provided by Orthanc core (including logging)
    // are used, `orthanc_context` must be initialized.
    orthanc::info("Initializing Vara Orthanc Worklist plugin.");
    register_on_worklist_callback(on_worklist_callback);
    orthanc::info("Vara Orthanc Worklist plugin initialization complete.");
    return 0;
}

#[no_mangle]
pub extern "C" fn OrthancPluginFinalize() {
    orthanc::info("Vara Ortahnc Worklist plugin finalized.");
}

#[no_mangle]
pub extern "C" fn OrthancPluginGetName() -> *const u8 {
    "Vara Orthanc\0".as_ptr()
}

#[no_mangle]
pub extern "C" fn OrthancPluginGetVersion() -> *const u8 {
    "0.1.0\0".as_ptr()
}

extern "C" fn on_worklist_callback(
    answers: *mut OrthancPluginWorklistAnswers,
    query: *const OrthancPluginWorklistQuery,
    _issuerAet: *const c_char,
    _calledAet: *const c_char,
) -> OrthancPluginErrorCode {
    let mwl_endpoints = orthanc_modality_endpoints();
    for endpoint in &mwl_endpoints {
        let worklist_items: Vec<JsonValue> =
            match orthanc_modality_worklist(endpoint) {
                Ok(JsonValue::Array(v)) => v,
                _ => {
                    orthanc::error("Failed to fetch modality worklist from peer Orthanc");
                    return 1;
                }
            };
        for item in worklist_items {
            let mut buffer = memory_buffer();
            let buffer_ptr = &mut buffer as *mut OrthancPluginMemoryBuffer;
            create_dicom(item.to_string(), buffer_ptr);
            if dicom_matches_query(query, buffer_ptr) {
                add_worklist_query_answer(answers, query, buffer_ptr)
            };
            orthanc::free_buffer(buffer_ptr);
        }
    }
    return OrthancCodeSuccess;
}

fn orthanc_modality_worklist(
    endpoint: &str
) -> Result<JsonValue, Box<dyn std::error::Error>> {
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
    let orthanc_api_user = env::var("VARA_ORTHANC_API_USER").unwrap_or(String::from("admin"));
    let orthanc_api_password =
        env::var("VARA_ORTHANC_API_PASSWORD").unwrap_or(String::from("password"));

    let workitems = http_client
        .post(endpoint)
        .body(
            r#"{"Short": true,
                  "Query": {"AccessionNumber": "*",
                            "RequestedProcedureID": null,
                            "ScheduledProcedureStepSequence": [],
                            "PatientName": null,
                            "StudyID": null,
                            "StudyInstanceUID": null}}"#,
        )
        .basic_auth(orthanc_api_user, Some(orthanc_api_password))
        .send();

    let cache_file = Path::new("vara_orthanc.json");
    let json_response = if workitems.is_err() || !workitems.as_ref().unwrap().status().is_success()
    {
        orthanc::info(&format!(
            "Reading the cache file for MWL entries. Failure: {:?}",
            workitems
        ));
        match cache::read(&cache_file) {
            Ok(contents) => contents,
            Err(error) => {
                orthanc::warning("Failed to read cache file");
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

fn register_on_worklist_callback(
    callback: extern "C" fn(
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
    let mut params = OnWorklistParams {
        callback: Some(callback),
    };

    orthanc::invoke_orthanc_service(
        orthanc::_OrthancPluginService__OrthancPluginService_RegisterWorklistCallback,
        &mut params as *mut OnWorklistParams as *mut c_void,
    );
}

// Returns a vector of endpoint URLs that can be queried for getting modality
// worklist items. Currently only supports a single endpoint that is configured
// by setting by environment variable: `VARA_ORTHANC_MODALITY_ENDPOINT`.
//
// TODO: Adjust everything to make use of Orthanc's configuration file.
fn orthanc_modality_endpoints() -> Vec<String> {
    // By default, we send an API request to the same Orthanc instance that
    // loads this plugin. Default endpoint
    let default_value = vec![String::from("http://localhost:9042/modalities/orthanc/find-worklist")];
    match env::var("VARA_ORTHANC_MODALITY_ENDPOINT") {
        Ok(modality_endpoint) => {
            vec![modality_endpoint.to_string()]
        }
        error @ Err(_) => {
            orthanc::warning(&format!("VARA_ORTHANC_MODALITY_ENDPOINT not defined: {:?}", error));
            default_value
        }
    }
}

//
// Returns a pointer to an OrthancPluginMemoryBuffer that can be used later by
// Orthanc core to provide or receive data. The buffer is empty and no memory is
// requested from Orthanc core.
fn memory_buffer() -> OrthancPluginMemoryBuffer {
    let buffer = OrthancPluginMemoryBuffer {
        data: std::ptr::null::<c_void>() as *mut c_void,
        size: 0,
    };
    buffer
}

fn create_dicom(dicom_json: String, target_buffer: *mut OrthancPluginMemoryBuffer) -> i32 {
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
    let mut params = CreateDicomParams {
        target: target_buffer,
        json: json_cstr.as_ptr(),
        pixel_data: std::ptr::null(),
        flags: orthanc::OrthancPluginCreateDicomFlags_OrthancPluginCreateDicomFlags_None,
        private_creator: private_creator.as_ptr() as *const c_char,
    };

    orthanc::invoke_orthanc_service(
        orthanc::_OrthancPluginService__OrthancPluginService_CreateDicom2,
        &mut params as *mut CreateDicomParams as *mut c_void,
    )
}

fn dicom_matches_query(
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

    let mut is_match: i32 = 0;
    let dicom_buffer = unsafe { &(*dicom) };
    let mut params = QueryWorklistOperationParams {
        query,
        dicom: dicom_buffer.data,
        size: dicom_buffer.size,
        is_match: &mut is_match as *mut i32,
        target: std::ptr::null_mut(),
    };

    orthanc::invoke_orthanc_service(
        orthanc::_OrthancPluginService__OrthancPluginService_WorklistIsMatch,
        &mut params as *mut QueryWorklistOperationParams as *mut c_void,
    );

    unsafe { (*params.is_match) != 0 }
}

fn add_worklist_query_answer(
    answers: *mut OrthancPluginWorklistAnswers,
    query: *const OrthancPluginWorklistQuery,
    answer: *const OrthancPluginMemoryBuffer,
) {
    // We do not want ownership of the value that this pointer points to.
    let answers_buffer = unsafe { &(*answer) };
    let mut params = orthanc::_OrthancPluginWorklistAnswersOperation {
        answers,
        query,
        dicom: answers_buffer.data as *mut c_void,
        size: answers_buffer.size as u32,
    };
    orthanc::invoke_orthanc_service(
        orthanc::_OrthancPluginService__OrthancPluginService_WorklistAddAnswer,
        &mut params as *mut orthanc::_OrthancPluginWorklistAnswersOperation as *mut c_void,
    );
}

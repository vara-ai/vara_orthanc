#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

extern crate reqwest;
extern crate serde_json;
extern crate tracing;

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
use std::vec::Vec;

struct OrthancContext(*mut OrthancPluginContext);
unsafe impl Send for OrthancContext {}
unsafe impl Sync for OrthancContext {}

static mut orthanc_context: Option<OrthancContext> = None;

#[no_mangle]
pub unsafe extern "C" fn OrthancPluginInitialize(context: *mut OrthancPluginContext) -> i32 {
    println!("Initializing Vara Orthanc Worklist plugin.");
    orthanc_context = Some(OrthancContext(context));
    register_on_worklist_callback(on_worklist_callback);
    println!("Vara Orthanc Worklist plugin initialization complete.");
    return 0;
}

#[no_mangle]
pub extern "C" fn OrthancPluginFinalize() {
    println!("Vara Ortahnc Worklist plugin finalized.");
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
    let (ae_title, ae_host, ae_port) = peer_orthanc();
    let worklist_items: Vec<JsonValue> =
        match orthanc_modality_worklist(&ae_title, &ae_host, ae_port).unwrap() {
            JsonValue::Array(v) => v,
            _ => {
                println!("Failed to fetch modality worklist from peer Orthanc");
                return 1;
            }
        };
    println!("Worklist items #2: {:?}", &worklist_items);
    for item in worklist_items {
        let buffer_size = 1000;
        let buffer = create_memory_buffer(buffer_size);
        println!("Worklist item from Orthanc: {}", &item);
        create_dicom(item.to_string(), buffer);
        add_worklist_query_answer(answers, query, buffer);
        free_buffer(buffer);
        println!(
            "Added one answer to the C-FIND query results. {:?}",
            item.to_string()
        );
    }

    return OrthancCodeSuccess;
}

fn orthanc_modality_worklist(
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
    //
    //  {"0008,0005" : "ISO_IR 100",
    //   "0008,0050" : "1",
    //   "0010,0010" : "^Test Party"}
    //
    // Note that
    // https://dicom.nema.org/dicom/2013/output/chtml/part18/sect_F.2.html is
    // considered invalid JSON by the Orthanc core parser.
    let workitems = http_client
        .post(url)
        .body(
            r#"{"Short": true,
                  "Query": {"AccessionNumber": "*",
                            "PatientName": null,
                            "StudyID": null,
                            "StudyInstanceUID": null}}"#,
        )
        .basic_auth("admin", Some("password"))
        .send()
        .unwrap();
    let json_response = workitems.text().unwrap();
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
    let context = orthanc_context.as_ref().unwrap().0;
    let invoker = (*context).InvokeService.unwrap();

    let params = Box::new(OnWorklistParams {
        callback: Some(callback),
    });

    invoker(
        context,
        orthanc::_OrthancPluginService__OrthancPluginService_RegisterWorklistCallback,
        Box::into_raw(params) as *mut c_void
    );
}


fn peer_orthanc() -> (String, String, u32) {
    let default_value = (String::from("orthanc"), String::from("orthanc"), 8042);
    let ae_title;
    let ae_host;
    let ae_port;

    match env::var("VARA_ORTHANC_AE_TITLE") {
        Ok(ae_title_) => {
            ae_title = ae_title_;
        }
        Err(_) => {
            return default_value;
        }
    };

    match env::var("VARA_ORTHANC_AE_HOST") {
        Ok(ae_host_) => {
            ae_host = ae_host_;
        }
        Err(_) => {
            return default_value;
        }
    };

    match env::var("VARA_ORTHANC_AE_PORT") {
        Ok(ae_port_) => {
            ae_port = ae_port_;
        }
        Err(_) => {
            return default_value;
        }
    };

    (ae_title, ae_host, ae_port.parse().unwrap())
}

unsafe fn create_memory_buffer(size: usize) -> *mut OrthancPluginMemoryBuffer {
    #[repr(C)]
    struct CreateMemoryBufferParams {
        target: *mut OrthancPluginMemoryBuffer,
        size: usize,
    }

    let context = orthanc_context.as_ref().unwrap().0;
    let invoker = (*context).InvokeService.unwrap();
    let buffer = OrthancPluginMemoryBuffer {
        data: std::ptr::null::<c_void>() as *mut c_void,
        size: 0,
    };

    let params = Box::into_raw(Box::new(CreateMemoryBufferParams {
        target: Box::into_raw(Box::new(buffer)) as *mut OrthancPluginMemoryBuffer,
        size,
    }));

    invoker(
        context,
        orthanc::_OrthancPluginService__OrthancPluginService_CreateMemoryBuffer,
        params as *mut c_void,
    );

    let created_buffer = Box::from_raw(params).target;
    println!(
        "Memory Buffer created with size: {}",
        &(*created_buffer).size
    );
    created_buffer
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

    let context = orthanc_context.as_ref().unwrap().0;
    let invoker = (*context).InvokeService.unwrap();

    let json_cstr = CString::new(dicom_json).unwrap();
    let private_creator = CString::new("vara").unwrap();
    let params = Box::new(CreateDicomParams {
        target: target_buffer,
        json: json_cstr.as_ptr(),
        pixel_data: std::ptr::null(),
        flags: orthanc::OrthancPluginCreateDicomFlags_OrthancPluginCreateDicomFlags_None,
        private_creator: private_creator.as_ptr() as *const c_char,
    });

    invoker(
        context,
        orthanc::_OrthancPluginService__OrthancPluginService_CreateDicom2,
        Box::into_raw(params) as *mut c_void,
    )
}

unsafe fn get_dicom_query(
    query: *const OrthancPluginWorklistQuery,
) -> *mut OrthancPluginMemoryBuffer {

    #[repr(C)]
    struct QueryWorklistOperationParams {
        query: *const OrthancPluginWorklistQuery,
        dicom: *const c_void,
        size: u32,
        is_match: *mut i32,
        target: *mut orthanc::OrthancPluginMemoryBuffer
    }

    let context = orthanc_context.as_ref().unwrap().0;
    let invoker = (*context).InvokeService.unwrap();
    let query_buffer = OrthancPluginMemoryBuffer {
        data: std::ptr::null::<c_void>() as *mut c_void,
        size: 0,
    };
    let query_buffer_ptr = Box::into_raw(Box::new(query_buffer));

    let query_params = QueryWorklistOperationParams {
        query,
        dicom: std::ptr::null(),
        size: 0,
        is_match: std::ptr::null_mut(),
        target: Box::into_raw(Box::new(&query_buffer)) as *mut OrthancPluginMemoryBuffer,
    };

    invoker(
        context,
        orthanc::_OrthancPluginService__OrthancPluginService_WorklistGetDicomQuery,
        Box::into_raw(Box::new(&query_params)) as *mut c_void,
    );

    query_buffer_ptr
}

unsafe fn add_worklist_query_answer(
    answers: *mut OrthancPluginWorklistAnswers,
    query: *const OrthancPluginWorklistQuery,
    answer: *const OrthancPluginMemoryBuffer,
) {
    let context = orthanc_context.as_ref().unwrap().0;
    let invoker = (*context).InvokeService.unwrap();

    let params = Box::new(orthanc::_OrthancPluginWorklistAnswersOperation {
        answers,
        query,
        dicom: (*answer).data as *mut c_void,
        size: (*answer).size as u32,
    });
    invoker(
        context,
        orthanc::_OrthancPluginService__OrthancPluginService_WorklistAddAnswer,
        Box::into_raw(params) as *mut c_void,
    );
}

unsafe fn free_buffer(buffer: *mut OrthancPluginMemoryBuffer) {
    let context = orthanc_context.as_ref().unwrap().0;
    (*context).Free.unwrap()(buffer as *mut c_void);
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

#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

extern crate reqwest;
extern crate tracing;
extern crate serde_json;

pub mod orthanc;

use crate::orthanc::OrthancPluginContext;
use crate::orthanc::OrthancPluginErrorCode;
use crate::orthanc::OrthancPluginErrorCode_OrthancPluginErrorCode_Success as OrthancCodeSuccess;
use crate::orthanc::OrthancPluginWorklistAnswers;
use crate::orthanc::OrthancPluginWorklistQuery;

use serde_json::Value as JsonValue;

use libc::{c_char, c_void};
use std::vec::Vec;
use std::ffi::CString;

struct OrthancContext(*mut OrthancPluginContext);
unsafe impl Send for OrthancContext {}
unsafe impl Sync for OrthancContext {}

static mut orthanc_context: Option<OrthancContext> = None;

#[repr(C)]
struct OnWorklistParams {
    callback: orthanc::OrthancPluginWorklistCallback
}

#[repr(C)]
struct CreateMemoryBufferParams {
    target: *mut orthanc::OrthancPluginMemoryBuffer,
    size: usize
}

#[repr(C)]
struct CreateDicomParams {
    target: *mut orthanc::OrthancPluginMemoryBuffer,
    json: *const c_char,
    pixel_data: *const orthanc::OrthancPluginImage,
    flags: orthanc::OrthancPluginCreateDicomFlags,
    private_creator: *const c_char
}

#[no_mangle]
pub unsafe extern "C" fn OrthancPluginInitialize(context: *mut OrthancPluginContext) -> i32 {
    println!("Initializing Vara Orthanc Worklist plugin.");
    orthanc_context = Some(OrthancContext(context));

    //
    // ----------------------------------------------------------------
    let params = Box::new(OnWorklistParams {
        callback: Some(on_worklist_callback),
    });
    let params: *const c_void = Box::into_raw(params) as *mut c_void;
    // ---------------------------------------------------------------
    //

    let invoker = (*context).InvokeService.unwrap();
    invoker(
        context,
        orthanc::_OrthancPluginService__OrthancPluginService_RegisterWorklistCallback,
        params,
    );
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

    let context = orthanc_context.as_ref().unwrap().0;
    let invoker = (*context).InvokeService.unwrap();

    // -- HTTP
    let worklist_items: Vec<JsonValue> =
        match orthanc_modality_worklist("orthanc", "127.0.0.1", 8042).unwrap() {
            JsonValue::Array(v) => v,
            _ => return 1
        };

    println!("Worklist items: {:?}", &worklist_items);

    for item in worklist_items {
        let buffer_capacity = 10000;
        let mut buffer = orthanc::OrthancPluginMemoryBuffer {
            data: std::ptr::null::<c_void>() as *mut c_void,
            size: buffer_capacity as u32,
        };

        let params = Box::new(CreateMemoryBufferParams {
            target: Box::into_raw(Box::new(buffer)) as *mut orthanc::OrthancPluginMemoryBuffer,
            size: buffer_capacity
        });

        invoker(
            context,
            orthanc::_OrthancPluginService__OrthancPluginService_CreateMemoryBuffer,
            Box::into_raw(params) as *mut c_void
        );

        println!("Worklist item from Orthanc: {}", &item);
        let json_cstr = CString::new(item.to_string()).unwrap();
        let private_creator = CString::new("vara").unwrap();
        let params = Box::new(CreateDicomParams {
            target: &mut buffer as *mut orthanc::OrthancPluginMemoryBuffer,
            json: json_cstr.as_ptr(),
            pixel_data: std::ptr::null(),
            flags: orthanc::OrthancPluginCreateDicomFlags_OrthancPluginCreateDicomFlags_None,
            private_creator: private_creator.as_ptr() as *const c_char
        });

        let create_dicom_status = invoker(
            context,
            orthanc::_OrthancPluginService__OrthancPluginService_CreateDicom2,
            Box::into_raw(params)  as *mut c_void,
        );

        if create_dicom_status != 0 {
            println!("Failed to create a DICOM from JSON text: {:?}", json_cstr);
            return 1;
        }

        let params = Box::new(orthanc::_OrthancPluginWorklistAnswersOperation {
            answers,
            query,
            dicom: buffer.data as *mut c_void,
            size: buffer.size as u32
        });
        invoker(
            context,
            orthanc::_OrthancPluginService__OrthancPluginService_WorklistAddAnswer,
            Box::into_raw(params) as *mut c_void
        );

        // Ask Orthanc core to free buffer created for the C-FIND answer.
        (*context).Free.unwrap()(buffer.data);

        println!("Added one answer to the C-FIND query results. {:?}", json_cstr);
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
        .body(r#"{"Short": true,
                  "Query": {"AccessionNumber": "*",
                            "PatientName": null,
                            "StudyID": null,
                            "StudyInstanceUID": null}}"#)
        .basic_auth("admin", Some("password"))
        .send()
        .unwrap();
    let json_response = workitems.text().unwrap();
    Ok(serde_json::from_str(&json_response)?)
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

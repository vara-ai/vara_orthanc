#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

pub mod orthanc;

use crate::orthanc::OrthancPluginContext;
use crate::orthanc::OrthancPluginWorklistAnswers;
use crate::orthanc::OrthancPluginWorklistQuery;
use crate::orthanc::OrthancPluginErrorCode;
use crate::orthanc::OrthancPluginErrorCode_OrthancPluginErrorCode_Success as OrthancCodeSuccess;


use std::os::raw::c_char;
use std::os::raw::c_void;

#[repr(C)]
struct OnWorklistParams {
    callback: orthanc::OrthancPluginWorklistCallback
}

#[no_mangle]
pub extern "C" fn OrthancPluginInitialize(
    context: *mut OrthancPluginContext,
) -> i32 {

    //
    // ----------------------------------------------------------------
    let params = Box::new(OnWorklistParams {
        callback: Some(on_worklist_callback)
    });
    let params: *const c_void = Box::into_raw(params) as *mut c_void;
    // ---------------------------------------------------------------
    //

    unsafe {
        let invoker = (*context).InvokeService.unwrap();
        invoker(
            context,
            orthanc::_OrthancPluginService__OrthancPluginService_RegisterWorklistCallback,
            params
        );
        return 0;
    }
}


extern "C" fn on_worklist_callback(
    answers: *mut OrthancPluginWorklistAnswers,
    query: *const OrthancPluginWorklistQuery,
    issuerAet: *const c_char,
    calledAet: *const c_char,
) -> OrthancPluginErrorCode {


    return OrthancCodeSuccess;
}

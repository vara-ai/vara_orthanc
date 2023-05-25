# vara_orthanc

A custom Orthanc plugin capable of 
- handling MWL C-FIND queries by proxying them to an Orthanc peer. Results are cached if the proxied peer is unavailable. 
- buffering DICOM images and forwarding them to a peer PACS `OnStableStudy` and periodically to keep the peer in sync.

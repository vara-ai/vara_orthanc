#!/usr/bin/env bash

DOCKER_HOST_GATEWAY="$(ip addr show docker0 | grep -E -o 'inet ([^/]+)' | cut -d\  -f 2)" docker-compose "$@"

# Once started, the following command should work:
# findscu -v -W -k AccessionNumber="*" -k "ScheduledProcedureStepSequence" -k "RequestedProcedureCodeSequence" -k StudyID -k PatientID -k PatientName -k RequestedProcedureID -k RequestedProcedureDescription -k RequestedProcedureCodeSequence localhost 5242

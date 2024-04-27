#!/bin/bash

NAMESPACE="rust-nextcloud-operator"
POD_TYPE=$1
POD_NAME=$(kubectl -n${NAMESPACE} get pods --no-headers=true -l "endpoint=${POD_TYPE}" -ocustom-columns="NAME:.metadata.name" | head -1)

echo "Entering: ${POD_TYPE}: ${POD_NAME}"

kubectl -n ${NAMESPACE} exec -ti ${POD_NAME} -- /bin/bash

#!/bin/bash

kubectl create secret generic $1     \
    -n $3                            \
    --from-file=.dockerconfigjson=$2 \
    --type=kubernetes.io/dockerconfigjson

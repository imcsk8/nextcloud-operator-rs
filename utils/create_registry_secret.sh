#!/bin/bash

kubectl create secret generic $1 \
    --from-file=.dockerconfigjson=$2 \
    --type=kubernetes.io/dockerconfigjson

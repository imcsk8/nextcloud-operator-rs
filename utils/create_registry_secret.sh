#!/bin/bash

# for podman:
# ${XDG_RUNTIME_DIR}/containers/auth.json
# for docker:
# $HOME/.docker/config.json

kubectl create secret generic $1     \
    -n $3                            \
    --from-file=.dockerconfigjson=$2 \
    --type=kubernetes.io/dockerconfigjson

#!/bin/bash

NAMESPACE="rust-nextcloud-operator"
# TODO: parametrize
kubectl -n ${NAMESPACE} create configmap php-www --from-file=www.conf=../containers/php-fpm/files/www.conf
